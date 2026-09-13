//! The verifier: answers nginx's auth_request for every service on this box.
//!
//! A bearer token that is a biscuit is checked against the public keys the
//! user's own devices registered - nothing here can sign, so nothing here can
//! be stolen to become someone. Anything else (a browser cookie, a legacy
//! kanidm id token) is handed to oauth2-proxy unchanged, so the two can
//! coexist while services move over.
//!
//! `/register` records a device's public key. The first device for a user is
//! admitted on a kanidm id token - the bootstrap, the single point where an
//! identity server is trusted. Every later device must be vouched for by an
//! existing one, so from then on no server can add a key that speaks for you.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, anyhow};
use axum::{
    Json, Router,
    extract::State,
    http::{HeaderMap, HeaderValue, StatusCode, header::AUTHORIZATION},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as B64;
use biscuit_auth::{Biscuit, PublicKey, UnverifiedBiscuit, builder::Algorithm};
use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;

struct App {
    http: reqwest::Client,
    dir: PathBuf,
    upstream_auth: String,
    issuer: String,
    audience: String,
    jwks_url: String,
    jwks: RwLock<Option<(jsonwebtoken::jwk::JwkSet, SystemTime)>>,
}

#[derive(Serialize, Deserialize, Default)]
struct Directory {
    keys: Vec<DeviceKey>,
}

#[derive(Serialize, Deserialize, Clone)]
struct DeviceKey {
    fingerprint: String,
    public_key: String, // base64 ed25519
    added: u64,
}

fn env(k: &str) -> Result<String> {
    std::env::var(k).with_context(|| format!("{k} is not set"))
}
fn env_or(k: &str, d: &str) -> String {
    std::env::var(k).unwrap_or_else(|_| d.to_string())
}

/// Usernames are also filenames here, so the whitelist is strict.
fn valid_user(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 64
        && s.chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '-' | '_' | '.'))
        && !s.starts_with('.')
}

impl App {
    fn load(&self, user: &str) -> Result<Directory> {
        let p = self.dir.join(format!("{user}.json"));
        match std::fs::read(&p) {
            Ok(b) => Ok(serde_json::from_slice(&b)?),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Directory::default()),
            Err(e) => Err(e.into()),
        }
    }

    fn store(&self, user: &str, d: &Directory) -> Result<()> {
        let p = self.dir.join(format!("{user}.json"));
        let tmp = self.dir.join(format!(".{user}.tmp"));
        std::fs::write(&tmp, serde_json::to_vec_pretty(d)?)?;
        std::fs::rename(&tmp, &p)?;
        Ok(())
    }

    /// Verify a biscuit against the user's registered keys. Returns the user.
    fn verify_biscuit(&self, token: &str) -> Result<String> {
        let unverified = UnverifiedBiscuit::from_base64(token).context("not a biscuit")?;
        // the user is named in the authority block; it has to be read before the
        // signature can be checked, because the key to check with depends on it
        let source = unverified
            .print_block_source(0)
            .context("unreadable authority block")?;
        let user = peek_user(&source).context("no user fact")?;
        anyhow::ensure!(valid_user(&user), "bad user in token");
        let dir = self.load(&user)?;
        anyhow::ensure!(!dir.keys.is_empty(), "no devices registered for {user}");

        let mut verified: Option<Biscuit> = None;
        for k in &dir.keys {
            let pk = match B64
                .decode(&k.public_key)
                .ok()
                .and_then(|b| PublicKey::from_bytes(&b, Algorithm::Ed25519).ok())
            {
                Some(pk) => pk,
                None => continue,
            };
            if let Ok(b) = unverified.clone().verify(|_| Ok(pk)) {
                verified = Some(b);
                break;
            }
        }
        let biscuit = verified.ok_or_else(|| anyhow!("signature matches no device of {user}"))?;

        let now = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs();
        let mut authorizer = biscuit_auth::builder::AuthorizerBuilder::new()
            .fact(format!("time({now})").as_str())
            .map_err(|e| anyhow!("{e}"))?
            .policy(format!("allow if user({:?})", user).as_str())
            .map_err(|e| anyhow!("{e}"))?
            .build(&biscuit)
            .map_err(|e| anyhow!("{e}"))?;
        authorizer
            .authorize()
            .map_err(|e| anyhow!("refused: {e}"))?;
        Ok(user)
    }

    /// Check a kanidm id token the way oauth2-proxy would: ES256 against the
    /// client's published jwks, issuer and audience pinned. Returns the user.
    async fn verify_id_token(&self, token: &str) -> Result<String> {
        #[derive(Deserialize)]
        struct Claims {
            preferred_username: String,
        }
        let header = jsonwebtoken::decode_header(token).context("not a jwt")?;
        let kid = header.kid.context("jwt without kid")?;
        let jwks = self.jwks().await?;
        let jwk = jwks.find(&kid).context("unknown signing key")?;
        let key = jsonwebtoken::DecodingKey::from_jwk(jwk)?;
        let mut v = jsonwebtoken::Validation::new(jsonwebtoken::Algorithm::ES256);
        v.set_issuer(&[&self.issuer]);
        v.set_audience(&[&self.audience]);
        let data = jsonwebtoken::decode::<Claims>(token, &key, &v).context("id token rejected")?;
        anyhow::ensure!(
            valid_user(&data.claims.preferred_username),
            "bad username claim"
        );
        Ok(data.claims.preferred_username)
    }

    async fn jwks(&self) -> Result<jsonwebtoken::jwk::JwkSet> {
        if let Some((set, at)) = self.jwks.read().await.as_ref()
            && at.elapsed().unwrap_or(Duration::MAX) < Duration::from_secs(600)
        {
            return Ok(set.clone());
        }
        let set: jsonwebtoken::jwk::JwkSet = self
            .http
            .get(&self.jwks_url)
            .send()
            .await?
            .error_for_status()?
            .json()
            .await
            .context("fetching jwks")?;
        *self.jwks.write().await = Some((set.clone(), SystemTime::now()));
        Ok(set)
    }
}

/// `user("david")` out of a block's datalog source, without a parser.
fn peek_user(source: &str) -> Option<String> {
    let i = source.find("user(\"")? + 6;
    let rest = &source[i..];
    let j = rest.find('"')?;
    Some(rest[..j].to_string())
}

fn bearer(headers: &HeaderMap) -> Option<&str> {
    headers
        .get(AUTHORIZATION)?
        .to_str()
        .ok()?
        .strip_prefix("Bearer ")
        .map(str::trim)
}

/// nginx auth_request lands here for every request to a protected service.
async fn verify(State(app): State<Arc<App>>, headers: HeaderMap) -> Response {
    if let Some(tok) = bearer(&headers)
        && UnverifiedBiscuit::from_base64(tok).is_ok()
    {
        return match app.verify_biscuit(tok) {
            Ok(user) => {
                let mut r = StatusCode::OK.into_response();
                let v = HeaderValue::from_str(&user).unwrap();
                r.headers_mut()
                    .insert("X-Auth-Request-Preferred-Username", v.clone());
                r.headers_mut().insert("X-Auth-Request-User", v);
                r
            }
            Err(e) => {
                eprintln!("biscuit refused: {e:#}");
                StatusCode::UNAUTHORIZED.into_response()
            }
        };
    }
    // not ours: a cookie session or a legacy kanidm token. oauth2-proxy's
    // answer is relayed as-is, headers included, so nginx sees no difference.
    let mut req = app.http.get(&app.upstream_auth);
    for name in [
        "authorization",
        "cookie",
        "x-original-uri",
        "x-forwarded-for",
        "x-forwarded-proto",
        "host",
    ] {
        if let Some(v) = headers.get(name) {
            req = req.header(name, v);
        }
    }
    match req.send().await {
        Ok(up) => {
            let mut r = StatusCode::from_u16(up.status().as_u16())
                .unwrap_or(StatusCode::BAD_GATEWAY)
                .into_response();
            for (k, v) in up.headers() {
                if k.as_str().starts_with("x-auth-request-") {
                    r.headers_mut().insert(k.clone(), v.clone());
                }
            }
            r
        }
        Err(e) => {
            eprintln!("upstream auth unreachable: {e}");
            StatusCode::BAD_GATEWAY.into_response()
        }
    }
}

#[derive(Deserialize)]
struct Registration {
    public_key: String,
    fingerprint: String,
    voucher: Option<String>,
}

async fn register(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    Json(reg): Json<Registration>,
) -> Response {
    let Some(tok) = bearer(&headers) else {
        return (StatusCode::UNAUTHORIZED, "no id token").into_response();
    };
    let user = match app.verify_id_token(tok).await {
        Ok(u) => u,
        Err(e) => {
            eprintln!("register: id token refused: {e:#}");
            return (StatusCode::UNAUTHORIZED, "id token refused").into_response();
        }
    };
    let Some(pk_bytes) = B64.decode(&reg.public_key).ok() else {
        return (StatusCode::BAD_REQUEST, "public_key is not base64").into_response();
    };
    if PublicKey::from_bytes(&pk_bytes, Algorithm::Ed25519).is_err() {
        return (StatusCode::BAD_REQUEST, "public_key is not ed25519").into_response();
    }
    if !valid_user(&reg.fingerprint) {
        return (StatusCode::BAD_REQUEST, "bad fingerprint").into_response();
    }
    let mut dir = match app.load(&user) {
        Ok(d) => d,
        Err(e) => {
            eprintln!("register: {e:#}");
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    };
    if dir.keys.iter().any(|k| k.public_key == reg.public_key) {
        return Json(serde_json::json!({ "registered": true, "existing": true })).into_response();
    }
    // The rule that makes the directory the user's, not the server's: after
    // the first device, an id token alone adds nothing. A voucher is a biscuit
    // signed by a device already on file, naming the newcomer.
    if !dir.keys.is_empty() {
        let Some(voucher) = reg.voucher.as_deref() else {
            return (
                StatusCode::FORBIDDEN,
                "an existing device must vouch for a new one",
            )
                .into_response();
        };
        let ok = app
            .verify_biscuit(voucher)
            .map(|u| u == user)
            .unwrap_or(false)
            && UnverifiedBiscuit::from_base64(voucher)
                .and_then(|b| b.print_block_source(0))
                .map(|s| s.contains(&format!("vouch({:?})", reg.fingerprint)))
                .unwrap_or(false);
        if !ok {
            return (StatusCode::FORBIDDEN, "voucher rejected").into_response();
        }
    }
    dir.keys.push(DeviceKey {
        fingerprint: reg.fingerprint,
        public_key: reg.public_key,
        added: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0),
    });
    if let Err(e) = app.store(&user, &dir) {
        eprintln!("register: {e:#}");
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    }
    eprintln!(
        "registered a device for {user} ({} on file)",
        dir.keys.len()
    );
    Json(serde_json::json!({ "registered": true, "existing": false })).into_response()
}

#[tokio::main]
async fn main() -> Result<()> {
    let bind: std::net::SocketAddr = env_or("VERIFY_BIND", "127.0.0.1:4181").parse()?;
    let issuer = env("VERIFY_ISSUER")?;
    let app = Arc::new(App {
        http: reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .build()?,
        dir: env("VERIFY_DIR")?.into(),
        upstream_auth: env("VERIFY_UPSTREAM_AUTH")?,
        jwks_url: env_or("VERIFY_JWKS_URL", &format!("{issuer}/public_key.jwk")),
        issuer,
        audience: env_or("VERIFY_AUDIENCE", "dd"),
        jwks: RwLock::new(None),
    });
    std::fs::create_dir_all(&app.dir)?;
    let router = Router::new()
        .route("/verify", get(verify))
        .route("/register", post(register))
        .route("/health", get(|| async { "ok" }))
        .with_state(app);
    let listener = tokio::net::TcpListener::bind(bind).await?;
    eprintln!("verify listening on {bind}");
    axum::serve(listener, router).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn peeks_the_user() {
        assert_eq!(
            peek_user("user(\"sarah\");\ndevice(\"ab\");\n").as_deref(),
            Some("sarah")
        );
        assert_eq!(peek_user("device(\"ab\");\n"), None);
    }
    #[test]
    fn usernames_are_filenames() {
        assert!(valid_user("david"));
        assert!(!valid_user("../etc"));
        assert!(!valid_user(".hidden"));
        assert!(!valid_user("Bad"));
    }
}
