mod derive;
mod ui;
mod vault;
mod who;

use anyhow::{Context, Result};
use auth::{KeyStore, OsKeyring};
use clap::{Parser, Subcommand};
use ente::LoginParams;
use vault::Vault;
use zeroize::Zeroizing;

const SERVICE: &str = "distributed-datacenter";
/// DD_KEYRING names a different credential-store service: a second "device"
/// on one machine, for trying the identity flow end to end.
fn service() -> String {
    std::env::var("DD_KEYRING").unwrap_or_else(|_| SERVICE.to_string())
}
/// The archive password. Separate again: it decrypts old computers, not
/// photos, and `dd lock` should be able to forget one without the other.
const ARCHIVE: &str = "archive";
const DEFAULT_ENTE: &str = "https://api.distributed-datacenter.duckdns.org";
const DEFAULT_IMAGES: &str = "https://files.distributed-datacenter.duckdns.org/images/";
/// Any browser-facing host does; the session cookie covers the whole domain.
const DEFAULT_ENROL: &str = "https://files.distributed-datacenter.duckdns.org/_dd/enrol";
/// Where a person's signed entry lives. Any box can hold one; a client that
/// names several sees whether they agree. node2 has no public name yet, so
/// its copy is reachable on the tailnet only.
const DEFAULT_DIRECTORIES: [&str; 2] = [
    "https://files.distributed-datacenter.duckdns.org/_dd/directory",
    "http://100.95.10.10:4181/_dd/directory",
];
/// keyring account holding the name the device key signs for
const USER: &str = "user";

#[derive(Parser)]
#[command(name = "dd", about = "distributed datacenter client")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Join, from nothing. Publishes your identity with this device as its
    /// first (that IS the account - nobody approves it), then creates the
    /// end-to-end encrypted photo account, which no server can do for you.
    Signup {
        #[arg(long)]
        username: String,
        /// For the photo account only; nothing else here has an email.
        #[arg(long)]
        email: String,
        #[arg(long = "directory", default_values = DEFAULT_DIRECTORIES)]
        directories: Vec<String>,
        #[arg(long, default_value = DEFAULT_ENTE)]
        ente_origin: String,
        /// Stop after the services account and leave photos for later.
        #[arg(long)]
        skip_photos: bool,
        /// Read the photo password from stdin instead of prompting.
        #[arg(long)]
        password_stdin: bool,
    },
    /// A browser login: prints a link, signed by this device and good for ten
    /// minutes, that sets up a passkey on the box it points at. No server
    /// vouches for you; your own key does.
    Enrol {
        #[arg(long, default_value = DEFAULT_ENROL)]
        url: String,
    },
    /// Who you are: a root key here, a recovery key on paper, and the entry
    /// you sign listing your devices. No server issues it.
    Identity {
        #[command(subcommand)]
        cmd: IdentityCmd,
        #[arg(long = "directory", default_values = DEFAULT_DIRECTORIES, global = true)]
        directories: Vec<String>,
    },
    /// This device's key.
    Device {
        #[command(subcommand)]
        cmd: DeviceCmd,
        #[arg(long = "directory", default_values = DEFAULT_DIRECTORIES, global = true)]
        directories: Vec<String>,
    },
    /// Unlock ente. Asks for the ente password once, then stores the derived
    /// keys in the OS credential store so it is not asked again.
    Unlock {
        #[arg(long, default_value = DEFAULT_ENTE)]
        origin: String,
        #[arg(long)]
        email: String,
        /// Read the password from stdin instead of prompting. rpassword needs
        /// a controlling terminal, so the prompt cannot work when piped.
        #[arg(long)]
        password_stdin: bool,
    },
    /// Print a token for the services, signed by this device. Good for an
    /// hour, made offline; this is what scripts and rclone should call.
    Token,
    /// Encrypted archives of old computers. restic's format, in your own
    /// directory on the server; the password never leaves this machine.
    Image {
        #[command(subcommand)]
        cmd: ImageCmd,
        #[arg(long, default_value = DEFAULT_IMAGES, global = true)]
        repo: String,
    },
    /// What is cached on this machine.
    Status,
    /// Forget the stored ente keys and device key on this machine.
    Lock {
        /// Also remove the identity root. Not undoable except by recovery.
        #[arg(long)]
        forget_identity: bool,
    },
}

#[derive(Subcommand)]
enum IdentityCmd {
    /// Create your identity and publish it, with this device as the first.
    /// Prints the recovery key once.
    New {
        /// Your name; defaults to the one already on this machine.
        #[arg(long)]
        name: Option<String>,
    },
    /// What every directory has for you, and whether it is yours.
    Show,
    /// Bring every directory up to the newest entry any of them holds. No
    /// signing: the entry carries its own, so a copy needs no key here.
    Publish,
    /// Lost every device? The paper key installs a new root and starts the
    /// device list over with this one.
    Recover {
        #[arg(long)]
        name: Option<String>,
        /// Read the recovery key from stdin instead of prompting.
        #[arg(long)]
        key_stdin: bool,
    },
    /// Print the root secret, to move it to another device you own.
    Export,
    /// Read a root secret from stdin and keep it here.
    Import {
        #[arg(long)]
        name: String,
    },
}

#[derive(Subcommand)]
enum DeviceCmd {
    /// Show this device's key: fingerprint and the public key to admit.
    Show,
    /// Admit a device to your identity by its public key, from `dd device
    /// show` there. Needs the root key on this machine.
    Admit { public_key: String },
}

#[derive(Subcommand)]
enum ImageCmd {
    /// Create your repository. Once; `push` does it too if it is missing.
    /// The archive password is derived from the photo password by
    /// `dd unlock` / `dd signup` - there is nothing new to choose here.
    Init {
        /// This machine unlocked before archives existed: ask for the photo
        /// password once more to derive the archive password from it.
        #[arg(long)]
        rederive: bool,
    },
    /// Archive a file, or stdin with `-`. For a whole drive:
    /// `sudo cat /dev/sdX | dd image push - --name old-laptop`
    /// so only cat runs as root and the keyring stays yours.
    Push {
        source: String,
        #[arg(long)]
        name: String,
    },
    /// Every archive in your repository.
    List,
    /// Rebuild the index from what is on the server. After an interrupted
    /// push, this lets the rerun skip everything already uploaded.
    Repair,
    /// Write an archive back out to stdout.
    Pull {
        name: String,
        /// Snapshot id prefix; the newest if omitted.
        #[arg(long, default_value = "latest")]
        snapshot: String,
    },
}

fn image(cmd: ImageCmd, repo: String) -> Result<()> {
    use archive::{Archive, Source, Stderr, TokenProvider};
    let keys = OsKeyring::new(service());

    if let ImageCmd::Init { rederive: true } = &cmd {
        let master = Zeroizing::new(rpassword::prompt_password("photo password: ")?);
        keys.set(ARCHIVE, &derive::archive_password(&master))?;
    }
    let password = keys.get(ARCHIVE)?.context(
        "no archive password on this machine - run `dd unlock`, or `dd image init --rederive`",
    )?;

    let tokens = std::sync::Arc::new(TokenProvider::new(OsKeyring::new(service())));
    let archive = Archive::new(url::Url::parse(&repo)?, tokens, password, Stderr)?;

    match cmd {
        ImageCmd::Init { .. } => {
            archive.init()?;
            println!("repository created at {repo}");
        }
        ImageCmd::Push { source, name } => {
            let src = if source == "-" {
                Source::Stdin
            } else {
                Source::File(std::path::PathBuf::from(source))
            };
            let e = archive.push(src, &name)?;
            println!(
                "archived {} as {}  ({} bytes)  snapshot {}",
                name, e.name, e.bytes, e.id
            );
        }
        ImageCmd::Repair => {
            archive.repair()?;
            println!("index rebuilt - rerun the push, it will skip what is already there");
        }
        ImageCmd::List => {
            let entries = archive.list()?;
            if entries.is_empty() {
                println!("nothing archived yet");
            }
            for e in entries {
                println!(
                    "{:<10} {:<28} {:>14}  {}",
                    &e.id[..8.min(e.id.len())],
                    e.name,
                    e.bytes,
                    e.time
                );
            }
        }
        ImageCmd::Pull { name, snapshot } => {
            let mut out = std::io::stdout().lock();
            archive.pull(&snapshot, &name, &mut out)?;
        }
    }
    Ok(())
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    let keys = OsKeyring::new(service());

    match cli.command {
        Command::Signup {
            username,
            email,
            directories,
            ente_origin,
            skip_photos,
            password_stdin,
        } => {
            let (kp, _) = auth::device::load_or_create(&keys)?;
            let recovery = who::create(&keys, &directories, &username, &kp).await?;
            keys.set(USER, &username)?;
            println!(
                "identity {username} published; device {} is its first",
                auth::device::fingerprint(&kp)
            );
            who::print_recovery(&recovery);

            if skip_photos {
                println!(
                    "\nphotos: skipped - `dd unlock --email you@example.com` once you have \
                     made the account in the photos app"
                );
                return Ok(());
            }
            if keys.get("ente")?.is_some() {
                println!("\nphotos: already unlocked on this machine, leaving it alone");
                return Ok(());
            }

            // A SECOND password, deliberately. This one is never sent anywhere:
            // it derives the key that decrypts the photos, which is why no
            // amount of sso can create this account for you.
            println!(
                "\nNow the photo account. This password is separate and cannot be \
                 recovered by anyone here - it derives the key your photos are \
                 encrypted with."
            );
            let password = if password_stdin {
                let mut line = String::new();
                std::io::stdin().read_line(&mut line)?;
                Zeroizing::new(line.trim_end_matches(['\n', '\r']).to_string())
            } else {
                let first = Zeroizing::new(rpassword::prompt_password("photo password: ")?);
                let again = Zeroizing::new(rpassword::prompt_password("again: ")?);
                anyhow::ensure!(first == again, "those did not match");
                anyhow::ensure!(first.len() >= 8, "use at least 8 characters");
                first
            };

            let archive_pw = derive::archive_password(&password);
            let client = ente::client(&ente_origin)?;
            let mut ui = ui::Term;
            let mut flow = ente::AuthFlow::new(&client, &mut ui);
            let account = flow
                .create_account(ente::CreateAccountParams {
                    email: email.clone(),
                    password,
                    source: None,
                })
                .await?;

            // Generated now rather than offered later, because the moment an
            // account exists is the only moment its owner has nothing to lose
            // by writing this down.
            let recovery = match &account.recovery_key {
                Some(k) => k.clone(),
                None => {
                    flow.create_recovery_key(&account.secrets.master_key, &account.key_attributes)
                        .await?
                        .recovery_key
                }
            };

            let v = Vault::from_secrets(account.user_id, &account.secrets);
            keys.set("ente", &v.to_json()?)?;
            keys.set(ARCHIVE, &archive_pw)?;

            println!(
                "\nphotos: created and unlocked (user_id {})",
                account.user_id
            );
            println!("\n  RECOVERY KEY - write this down, on paper, now.");
            println!("  It is the only way back in if you forget the photo password.");
            println!("  Nobody running this service can recover it for you.\n");
            println!("    {recovery}\n");
            println!("For a browser: `dd enrol` prints a link that sets up a passkey.");
        }

        Command::Enrol { url } => {
            let kp = auth::device::load(&keys)?.context("no device key here - `dd device show`")?;
            let user = keys
                .get(USER)?
                .context("no name on this machine - `dd identity new` or `dd identity import`")?;
            let tok = auth::device::mint_for(
                &kp,
                &user,
                std::time::Duration::from_secs(600),
                Some("enrol"),
            )?;
            println!(
                "open this within ten minutes, in the browser that should get the passkey:
"
            );
            println!(
                "  {url}?t={tok}
"
            );
            println!(
                "it can set up a passkey and nothing else; afterwards the passkey signs you in."
            );
        }

        Command::Identity { cmd, directories } => match cmd {
            IdentityCmd::New { name } => {
                let name = match name.or(keys.get(USER)?.map(|z| z.to_string())) {
                    Some(n) => n,
                    None => anyhow::bail!("no name: pass --name"),
                };
                let (kp, _) = auth::device::load_or_create(&keys)?;
                let recovery = who::create(&keys, &directories, &name, &kp).await?;
                keys.set(USER, &name)?;
                println!(
                    "identity {name} published; device {} is its first",
                    auth::device::fingerprint(&kp)
                );
                who::print_recovery(&recovery);
            }
            IdentityCmd::Show => {
                let name = keys
                    .get(USER)?
                    .context("no name here - `dd identity new` or `dd identity import`")?
                    .to_string();
                let mine =
                    who::load_root(&keys)?.map(|r| identity::encode_public(&r.verifying_key()));
                let here = auth::device::load(&keys)?.map(|kp| auth::device::public_b64(&kp));
                println!("{name}");
                println!(
                    "root here:   {}",
                    mine.as_deref()
                        .map(identity::fingerprint)
                        .unwrap_or("none".into())
                );
                for (d, r) in who::fetch(&directories, &name).await {
                    match r {
                        Ok(None) => println!("{d}: no entry"),
                        Err(e) => println!("{d}: unreachable ({e})"),
                        Ok(Some(e)) => {
                            let owner = match &mine {
                                Some(m) if *m == e.entry.root => "yours",
                                Some(_) => "NOT YOURS - different root",
                                None => "root not held here",
                            };
                            println!(
                                "{d}: version {} root {} ({owner})",
                                e.entry.version,
                                identity::fingerprint(&e.entry.root)
                            );
                            for dev in &e.entry.devices {
                                let this = here.as_deref() == Some(dev.public_key.as_str());
                                println!(
                                    "  device {}{}",
                                    dev.fingerprint,
                                    if this { " (this one)" } else { "" }
                                );
                            }
                        }
                    }
                }
            }
            IdentityCmd::Publish => {
                let name = keys
                    .get(USER)?
                    .context("no name here - `dd identity new` or `dd identity import`")?
                    .to_string();
                let found = who::fetch(&directories, &name).await;
                let newest = who::newest(&found).context("no directory has an entry")?;
                let behind: Vec<String> = found
                    .iter()
                    .filter(|(_, r)| !matches!(r, Ok(Some(e)) if e.entry.version >= newest.entry.version))
                    .map(|(d, _)| d.clone())
                    .collect();
                if behind.is_empty() {
                    println!("every directory has version {}", newest.entry.version);
                } else {
                    who::publish(&behind, &newest).await?;
                }
            }
            IdentityCmd::Recover { name, key_stdin } => {
                let name = match name.or(keys.get(USER)?.map(|z| z.to_string())) {
                    Some(n) => n,
                    None => anyhow::bail!("no name: pass --name"),
                };
                let key = if key_stdin {
                    let mut line = String::new();
                    std::io::stdin().read_line(&mut line)?;
                    Zeroizing::new(line.trim().to_string())
                } else {
                    Zeroizing::new(rpassword::prompt_password("recovery key: ")?)
                };
                let (kp, _) = auth::device::load_or_create(&keys)?;
                let next = who::recover(&keys, &directories, &name, &key, &kp).await?;
                keys.set(USER, &name)?;
                println!("recovered {name}: new root here, every other device dropped");
                who::print_recovery(&next);
            }
            IdentityCmd::Export => {
                let root = who::load_root(&keys)?.context("no identity here")?;
                println!("{}", *identity::encode_secret(&root));
            }
            IdentityCmd::Import { name } => {
                let mut line = String::new();
                std::io::stdin().read_line(&mut line)?;
                let root = identity::decode_secret(line.trim()).context("bad root secret")?;
                keys.set(who::ROOT, &identity::encode_secret(&root))?;
                keys.set(USER, &name)?;
                println!(
                    "root for {name} stored ({})",
                    identity::fingerprint(&identity::encode_public(&root.verifying_key()))
                );
            }
        },

        Command::Device { cmd, directories } => match cmd {
            DeviceCmd::Show => {
                // made on first sight: a new device's first step is to show
                // its key to one that can admit it, before any login
                let (kp, _) = auth::device::load_or_create(&keys)?;
                println!("{}", auth::device::fingerprint(&kp));
                println!("{}", auth::device::public_b64(&kp));
            }
            DeviceCmd::Admit { public_key } => {
                let root = who::load_root(&keys)?.context(
                    "no root key on this machine - `dd identity export` on one that has it",
                )?;
                let name = keys
                    .get(USER)?
                    .context("no name here - `dd identity new` or `dd identity import`")?;
                let signed = who::admit(&directories, &name, &root, &public_key).await?;
                println!(
                    "device {} admitted; version {}",
                    identity::fingerprint(&public_key),
                    signed.entry.version
                );
            }
        },

        Command::Unlock {
            origin,
            email,
            password_stdin,
        } => {
            if keys.get("ente")?.is_some() {
                println!("already unlocked on this machine - `dd lock` first to redo it");
                return Ok(());
            }
            let password = if password_stdin {
                let mut line = String::new();
                std::io::stdin().read_line(&mut line)?;
                Zeroizing::new(line.trim_end_matches(['\n', '\r']).to_string())
            } else {
                Zeroizing::new(rpassword::prompt_password("ente password: ")?)
            };
            let archive_pw = derive::archive_password(&password);
            let client = ente::client(&origin)?;
            let mut ui = ui::Term;
            let mut flow = ente::AuthFlow::new(&client, &mut ui);
            let account = flow.login(LoginParams { email, password }).await?;

            let v = Vault::from_secrets(account.user_id, &account.secrets);
            keys.set("ente", &v.to_json()?)?;
            // the same typed password also yields the archive password, so
            // `dd image` never asks for one
            keys.set(ARCHIVE, &archive_pw)?;
            println!("unlocked and stored for user_id {}", account.user_id);
        }

        Command::Token => {
            let kp = auth::device::load(&keys)?.context("no device key here - `dd device show`")?;
            let user = keys
                .get(USER)?
                .context("no name on this machine - `dd identity new` or `dd identity import`")?;
            println!(
                "{}",
                auth::device::mint(&kp, &user, std::time::Duration::from_secs(3600))?
            );
        }

        // rustic_core is synchronous and the token provider blocks on its own
        // small runtime. Neither may run on a tokio worker thread, so the
        // whole command gets a plain thread of its own.
        Command::Image { cmd, repo } => {
            return std::thread::spawn(move || image(cmd, repo))
                .join()
                .map_err(|_| anyhow::anyhow!("image command panicked"))?;
        }

        Command::Status => {
            match who::load_root(&keys)? {
                Some(r) => println!(
                    "identity: {} root held here",
                    identity::fingerprint(&identity::encode_public(&r.verifying_key()))
                ),
                None => {
                    println!("identity: no root here - `dd identity new` or `dd identity import`")
                }
            }
            match auth::device::load(&keys)? {
                Some(kp) => println!(
                    "device:  {} - signs its own tokens",
                    auth::device::fingerprint(&kp)
                ),
                None => println!("device:  none - `dd device show` makes one"),
            }
            match keys.get(ARCHIVE)? {
                Some(_) => println!("archive: password stored - `dd image push` works"),
                None => println!("archive: none - run `dd image init`"),
            }
            match keys.get("ente")? {
                Some(raw) => {
                    let v = Vault::from_json(&raw)?;
                    println!("ente:   unlocked (user_id {})", v.user_id);
                }
                None => println!("ente:   locked - run `dd unlock --email you@example.com`"),
            }
        }

        Command::Lock { forget_identity } => {
            keys.clear(auth::device::ACCOUNT)?;
            keys.clear(USER)?;
            keys.clear(ARCHIVE)?;
            keys.clear("ente")?;
            println!("ente keys and device key removed from this machine's credential store");
            if keys.get(who::ROOT)?.is_some() {
                if forget_identity {
                    keys.clear(who::ROOT)?;
                    println!("identity root removed too - only the paper key gets it back");
                } else {
                    println!("the identity root stays; `dd lock --forget-identity` removes it");
                }
            }
        }
    }
    Ok(())
}
