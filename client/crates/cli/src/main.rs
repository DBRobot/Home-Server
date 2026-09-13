mod derive;
mod ui;
mod vault;

use anyhow::{Context, Result};
use auth::{KeyStore, OsKeyring};
use clap::{Parser, Subcommand};
use ente::LoginParams;
use vault::Vault;
use zeroize::Zeroizing;

const SERVICE: &str = "distributed-datacenter";
/// Keyring account holding the kanidm refresh token. Separate from "ente":
/// these authorise services, that one decrypts photos, and conflating them
/// would mean one `dd lock` throwing away more than the user asked.
const KANIDM: &str = "kanidm-refresh";
/// The archive password. Separate again: it decrypts old computers, not
/// photos, and `dd lock` should be able to forget one without the other.
const ARCHIVE: &str = "archive";
const DEFAULT_IDM: &str = "https://idm.distributed-datacenter.duckdns.org/oauth2/openid/dd";
const DEFAULT_ENTE: &str = "https://api.distributed-datacenter.duckdns.org";
const DEFAULT_SIGNUP: &str = "https://signup.distributed-datacenter.duckdns.org/";
const DEFAULT_IMAGES: &str = "https://files.distributed-datacenter.duckdns.org/images/";

#[derive(Parser)]
#[command(name = "dd", about = "distributed datacenter client")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Create an account, from nothing. Asks the signup service for the
    /// services account, then creates the end-to-end encrypted photo account,
    /// which no server-side flow can do for you.
    Signup {
        #[arg(long)]
        username: String,
        /// Your name, as other people will see it.
        #[arg(long)]
        name: String,
        #[arg(long)]
        email: String,
        #[arg(long, default_value = DEFAULT_SIGNUP)]
        url: String,
        #[arg(long, default_value = DEFAULT_ENTE)]
        ente_origin: String,
        /// Stop after the services account and leave photos for later.
        #[arg(long)]
        skip_photos: bool,
        /// Read the photo password from stdin instead of prompting.
        #[arg(long)]
        password_stdin: bool,
    },
    /// Sign in to kanidm. Proves who you are to services; touches no keys.
    Login {
        #[arg(long, default_value = DEFAULT_IDM)]
        issuer: String,
        #[arg(long, default_value = "dd")]
        client_id: String,
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
    /// Print a token for the gateways. Renews silently from the stored
    /// refresh token, so this is what scripts and rclone should call.
    Token {
        #[arg(long, default_value = DEFAULT_IDM)]
        issuer: String,
        #[arg(long, default_value = "dd")]
        client_id: String,
    },
    /// Encrypted archives of old computers. restic's format, in your own
    /// directory on the server; the password never leaves this machine.
    Image {
        #[command(subcommand)]
        cmd: ImageCmd,
        #[arg(long, default_value = DEFAULT_IMAGES, global = true)]
        repo: String,
        #[arg(long, default_value = DEFAULT_IDM, global = true)]
        issuer: String,
    },
    /// What is cached on this machine.
    Status,
    /// Forget the stored ente keys and kanidm token on this machine.
    Lock,
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

fn image(cmd: ImageCmd, repo: String, issuer: String) -> Result<()> {
    use archive::{Archive, Source, Stderr, TokenProvider};
    let keys = OsKeyring::new(SERVICE);

    if let ImageCmd::Init { rederive: true } = &cmd {
        let master = Zeroizing::new(rpassword::prompt_password("photo password: ")?);
        keys.set(ARCHIVE, &derive::archive_password(&master))?;
    }
    let password = keys.get(ARCHIVE)?.context(
        "no archive password on this machine - run `dd unlock`, or `dd image init --rederive`",
    )?;

    let tokens = std::sync::Arc::new(TokenProvider::new(
        &issuer,
        "dd",
        OsKeyring::new(SERVICE),
        KANIDM,
    )?);
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
    let keys = OsKeyring::new(SERVICE);

    match cli.command {
        Command::Signup {
            username,
            name,
            email,
            url,
            ente_origin,
            skip_photos,
            password_stdin,
        } => {
            // The signup service answers json when asked to, so this never
            // parses the html page a browser gets.
            #[derive(serde::Deserialize)]
            struct Reply {
                status: String,
                message: String,
            }

            let reply: Reply = reqwest::Client::new()
                .post(&url)
                .header(reqwest::header::ACCEPT, "application/json")
                .form(&[
                    ("username", &username),
                    ("display_name", &name),
                    ("email", &email),
                ])
                .send()
                .await
                .context("reaching the signup service")?
                .json()
                .await
                .context("the signup service did not answer json")?;

            if reply.status != "created" {
                anyhow::bail!("{}", reply.message);
            }
            println!("{}", reply.message);

            if skip_photos {
                println!(
                    "\nphotos: skipped - run `dd signup --skip-photos=false` or create it \
                     in the photos app later"
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
            println!("Once you have set a passkey from the emailed link, run `dd login`.");
        }

        Command::Login { issuer, client_id } => {
            let session = auth::login(&issuer, &client_id)
                .await
                .context("kanidm login failed")?;
            let who = session
                .preferred_username
                .clone()
                .unwrap_or_else(|| session.subject.clone());
            println!("signed in to kanidm as {who}");
            match &session.refresh_token {
                Some(rt) => {
                    keys.set(KANIDM, rt)?;
                    println!("token stored - `dd token` renews without a browser");
                }
                None => println!(
                    "no refresh token: the `offline_access` scope was not granted, \
                     so every expiry needs another login"
                ),
            }
        }

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

        Command::Token { issuer, client_id } => {
            let stored = keys
                .get(KANIDM)?
                .context("not signed in to kanidm - run `dd login`")?;
            let session = auth::refresh(&issuer, &client_id, &stored)
                .await
                .context("renewing the kanidm token failed - run `dd login` again")?;
            // Kanidm rotates: the token just used is dead, so failing to store
            // the replacement would make this the last renewal that works.
            if let Some(rt) = &session.refresh_token {
                keys.set(KANIDM, rt)?;
            }
            // The ID token, not the access token. Kanidm's access token carries
            // nothing but `sub`, and oauth2-proxy builds its identity from the
            // id token - see the note in modules/webdav-media.nix.
            let idt = session
                .id_token
                .context("kanidm returned no id token, so there is no identity to present")?;
            println!("{idt}");
        }

        // rustic_core is synchronous and the token provider blocks on its own
        // small runtime. Neither may run on a tokio worker thread, so the
        // whole command gets a plain thread of its own.
        Command::Image { cmd, repo, issuer } => {
            return std::thread::spawn(move || image(cmd, repo, issuer))
                .join()
                .map_err(|_| anyhow::anyhow!("image command panicked"))?;
        }

        Command::Status => {
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
            match keys.get(KANIDM)? {
                Some(_) => println!("kanidm: signed in - `dd token` mints one on demand"),
                None => println!("kanidm: signed out - run `dd login`"),
            }
        }

        Command::Lock => {
            keys.clear(ARCHIVE)?;
            keys.clear("ente")?;
            keys.clear(KANIDM)?;
            println!("ente keys and kanidm token removed from this machine's credential store");
        }
    }
    Ok(())
}
