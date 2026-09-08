mod ui;
mod vault;

use anyhow::{Context, Result};
use auth::{KeyStore, OsKeyring};
use clap::{Parser, Subcommand};
use ente::LoginParams;
use vault::Vault;
use zeroize::Zeroizing;

const SERVICE: &str = "distributed-datacenter";
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
    /// What is cached on this machine.
    Status,
    /// Forget the stored ente keys on this machine.
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
            // Shape only, never the token: whether it is a jwt decides how
            // a proxy in front of the llm can validate it.
            let at: &str = &session.access_token;
            let parts = at.split('.').count();
            if parts == 3 {
                use base64::Engine as _;
                let hdr = base64::engine::general_purpose::URL_SAFE_NO_PAD
                    .decode(at.split('.').next().unwrap_or(""))
                    .ok()
                    .and_then(|b| String::from_utf8(b).ok())
                    .unwrap_or_else(|| "<undecodable>".into());
                println!("access token: JWT, header {hdr}");
            } else {
                println!("access token: opaque ({parts} segment(s), {} chars)", at.len());
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

        Command::Status => {
            match keys.get("ente")? {
                Some(raw) => {
                    let v = Vault::from_json(&raw)?;
                    println!("ente:   unlocked (user_id {})", v.user_id);
                }
                None => println!("ente:   locked - run `dd unlock --email you@example.com`"),
            }
            println!("kanidm: `dd login` each session; no token is persisted yet");
        }

        Command::Lock => {
            keys.clear("ente")?;
            println!("ente keys removed from this machine's credential store");
        }
    }
    Ok(())
}
