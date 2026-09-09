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
const DEFAULT_IDM: &str = "https://idm.distributed-datacenter.duckdns.org/oauth2/openid/dd";
const DEFAULT_ENTE: &str = "https://api.distributed-datacenter.duckdns.org";

#[derive(Parser)]
#[command(name = "dd", about = "distributed datacenter client")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
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
    /// What is cached on this machine.
    Status,
    /// Forget the stored ente keys and kanidm token on this machine.
    Lock,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    let keys = OsKeyring::new(SERVICE);

    match cli.command {
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
            let client = ente::client(&origin)?;
            let mut ui = ui::Term;
            let mut flow = ente::AuthFlow::new(&client, &mut ui);
            let account = flow.login(LoginParams { email, password }).await?;

            let v = Vault::from_secrets(account.user_id, &account.secrets);
            keys.set("ente", &v.to_json()?)?;
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

        Command::Status => {
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
            keys.clear("ente")?;
            keys.clear(KANIDM)?;
            println!("ente keys and kanidm token removed from this machine's credential store");
        }
    }
    Ok(())
}
