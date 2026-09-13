use anyhow::{Context, Result};

fn env(k: &str) -> Result<String> {
    std::env::var(k).with_context(|| format!("{k} is not set"))
}
fn env_or(k: &str, d: &str) -> String {
    std::env::var(k).unwrap_or_else(|_| d.to_string())
}

#[tokio::main]
async fn main() -> Result<()> {
    let full = env_or("VERIFY_ROLE", "full") != "directory";
    let cfg = verify::Config {
        bind: env_or("VERIFY_BIND", "127.0.0.1:4181").parse()?,
        dir: env("VERIFY_DIR")?.into(),
        peers: env_or("VERIFY_PEERS", "")
            .split(',')
            .map(str::trim)
            .filter(|p| !p.is_empty())
            .map(str::to_string)
            .collect(),
        sync_secs: env_or("VERIFY_SYNC_SECS", "300").parse()?,
        domain: if full {
            Some(env("VERIFY_DOMAIN")?)
        } else {
            None
        },
        oidc: match std::env::var("VERIFY_OIDC_ISSUER") {
            Ok(issuer) if full => Some(verify::OidcConfig {
                issuer,
                client_id: env("VERIFY_OIDC_CLIENT_ID")?,
                client_secret: std::fs::read_to_string(env("VERIFY_OIDC_CLIENT_SECRET_FILE")?)?
                    .trim()
                    .to_string(),
                redirect: env("VERIFY_OIDC_REDIRECT")?,
            }),
            _ => None,
        },
    };
    let (_, task) = verify::start(cfg).await?;
    task.await?
}
