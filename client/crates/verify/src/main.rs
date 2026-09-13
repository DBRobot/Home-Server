//! The verifier: answers nginx's auth_request for every service on this box.
//!
//! A bearer token that is a biscuit is checked against the public keys the
//! user's own devices registered - nothing here can sign, so nothing here can
//! be stolen to become someone. Anything else (a browser cookie, a legacy
//! kanidm id token) is handed to oauth2-proxy unchanged, so the two can
//! coexist while services move over.
//!
//! Which keys are a user's is the directory (directory.rs): an entry the
//! person signs with a key they alone hold. This box stores and serves it and
//! can add nothing to it. With VERIFY_ROLE=directory that is all a box does.

mod directory;
mod oidc;
mod pages;
mod session;

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, anyhow};
use axum::{
    Form, Json, Router,
    extract::{Query, State},
    http::{HeaderMap, HeaderValue, StatusCode, header::AUTHORIZATION},
    response::{Html, IntoResponse, Redirect, Response},
    routing::{get, post},
};
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as B64;
use biscuit_auth::{Biscuit, PublicKey, UnverifiedBiscuit, builder::Algorithm};
use serde::{Deserialize, Serialize};
use webauthn_rs::prelude::*;

struct App {
    http: reqwest::Client,
    dir: PathBuf,
    directory: Arc<directory::Directory>,
    upstream_auth: String,
    sessions: session::Sessions,
    webauthn: Webauthn,
    ceremonies: Mutex<HashMap<String, (Instant, Ceremony)>>,
    oidc: Option<oidc::Issuer>,
}

enum Ceremony {
    Enrol {
        user: String,
        state: PasskeyRegistration,
    },
    Login {
        user: String,
        state: PasskeyAuthentication,
    },
}

/// Browser passkeys enrolled on THIS box. Box-local consent state, not part of
/// the identity: a passkey enrolled through a page this box served is a
/// decision to trust this box for browser sessions, nothing more.
#[derive(Serialize, Deserialize, Default)]
struct Passkeys {
    passkeys: Vec<Passkey>,
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
    fn entry(&self, user: &str) -> Result<Option<identity::SignedEntry>> {
        self.directory.entry(user)
    }

    fn load(&self, user: &str) -> Result<Passkeys> {
        let p = self.dir.join(format!("{user}.passkeys.json"));
        match std::fs::read(&p) {
            Ok(b) => Ok(serde_json::from_slice(&b)?),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Passkeys::default()),
            Err(e) => Err(e.into()),
        }
    }

    fn store(&self, user: &str, d: &Passkeys) -> Result<()> {
        let p = self.dir.join(format!("{user}.passkeys.json"));
        let tmp = self.dir.join(format!(".{user}.passkeys.tmp"));
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
        let signed = self
            .entry(&user)?
            .context("no identity published for that name")?;
        let mut verified: Option<Biscuit> = None;
        for k in &signed.entry.devices {
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
}

impl App {
    fn ceremony_put(&self, c: Ceremony) -> String {
        let id = session::random_id();
        let mut m = self.ceremonies.lock().unwrap();
        m.retain(|_, (t, _)| t.elapsed() < Duration::from_secs(300));
        m.insert(id.clone(), (Instant::now(), c));
        id
    }
    fn ceremony_take(&self, id: &str) -> Option<Ceremony> {
        self.ceremonies.lock().unwrap().remove(id).map(|(_, c)| c)
    }

    /// Who this request is, if anyone: a device-signed biscuit, this box's
    /// own session cookie, or - the bootstrap - a session oauth2-proxy vouches
    /// for. Used for /verify and for deciding who may enrol a passkey.
    async fn identify(&self, headers: &HeaderMap) -> Option<String> {
        if let Some(tok) = bearer(headers)
            && UnverifiedBiscuit::from_base64(tok).is_ok()
        {
            return self.verify_biscuit(tok).ok();
        }
        let cookie = headers.get("cookie").and_then(|v| v.to_str().ok());
        if let Some(u) = self.sessions.user(cookie) {
            return Some(u);
        }
        let mut req = self.http.get(&self.upstream_auth);
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
        let up = req.send().await.ok()?;
        if !up.status().is_success() {
            return None;
        }
        let u = up
            .headers()
            .get("x-auth-request-preferred-username")?
            .to_str()
            .ok()?
            .to_string();
        valid_user(&u).then_some(u)
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
    // this box's own session cookie, from a passkey login at the verifier
    let cookie = headers.get("cookie").and_then(|v| v.to_str().ok());
    if let Some(user) = app.sessions.user(cookie) {
        let mut r = StatusCode::OK.into_response();
        let v = HeaderValue::from_str(&user).unwrap();
        r.headers_mut()
            .insert("X-Auth-Request-Preferred-Username", v.clone());
        r.headers_mut().insert("X-Auth-Request-User", v);
        return r;
    }
    // not ours: an oauth2-proxy cookie or a legacy kanidm token. oauth2-proxy's
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

fn with_challenge(v: impl Serialize, ceremony: String) -> Response {
    let mut j = serde_json::to_value(v).unwrap_or_default();
    j["ceremony"] = serde_json::Value::String(ceremony);
    Json(j).into_response()
}

async fn enrol_start(State(app): State<Arc<App>>, headers: HeaderMap) -> Response {
    let Some(user) = app.identify(&headers).await else {
        return StatusCode::UNAUTHORIZED.into_response();
    };
    let dir = match app.load(&user) {
        Ok(d) => d,
        Err(e) => {
            eprintln!("enrol: {e:#}");
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    };
    // a stable per-user id for the authenticator: derived from the name, so
    // the same person enrolling twice is the same user to the passkey
    let uid = {
        use sha2::Digest as _;
        let h = sha2::Sha256::digest(format!("dd-user:{user}").as_bytes());
        let mut b = [0u8; 16];
        b.copy_from_slice(&h[..16]);
        Uuid::from_bytes(b)
    };
    let exclude: Vec<CredentialID> = dir.passkeys.iter().map(|p| p.cred_id().clone()).collect();
    match app
        .webauthn
        .start_passkey_registration(uid, &user, &user, Some(exclude))
    {
        Ok((ccr, state)) => {
            let id = app.ceremony_put(Ceremony::Enrol { user, state });
            with_challenge(ccr, id)
        }
        Err(e) => {
            eprintln!("enrol start: {e}");
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}

async fn enrol_finish(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    Json(reg): Json<RegisterPublicKeyCredential>,
) -> Response {
    let Some(Ceremony::Enrol { user, state }) = headers
        .get("x-dd-ceremony")
        .and_then(|v| v.to_str().ok())
        .and_then(|id| app.ceremony_take(id))
    else {
        return (StatusCode::BAD_REQUEST, "no ceremony").into_response();
    };
    let passkey = match app.webauthn.finish_passkey_registration(&reg, &state) {
        Ok(p) => p,
        Err(e) => {
            return (StatusCode::BAD_REQUEST, format!("passkey rejected: {e}")).into_response();
        }
    };
    let mut dir = app.load(&user).unwrap_or_default();
    dir.passkeys.push(passkey);
    if let Err(e) = app.store(&user, &dir) {
        eprintln!("enrol: {e:#}");
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    }
    eprintln!(
        "passkey enrolled for {user} ({} on file)",
        dir.passkeys.len()
    );
    let mut r = StatusCode::OK.into_response();
    r.headers_mut().insert(
        "set-cookie",
        HeaderValue::from_str(&app.sessions.issue(&user)).unwrap(),
    );
    r
}

#[derive(Deserialize)]
struct LoginStart {
    username: String,
}

async fn login_start(State(app): State<Arc<App>>, Json(q): Json<LoginStart>) -> Response {
    let user = q.username.trim().to_lowercase();
    if !valid_user(&user) {
        return (StatusCode::BAD_REQUEST, "bad username").into_response();
    }
    let dir = app.load(&user).unwrap_or_default();
    if dir.passkeys.is_empty() {
        return (
            StatusCode::NOT_FOUND,
            "no passkey enrolled for that name here",
        )
            .into_response();
    }
    match app.webauthn.start_passkey_authentication(&dir.passkeys) {
        Ok((rcr, state)) => {
            let id = app.ceremony_put(Ceremony::Login { user, state });
            with_challenge(rcr, id)
        }
        Err(e) => {
            eprintln!("login start: {e}");
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}

async fn login_finish(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    Json(cred): Json<PublicKeyCredential>,
) -> Response {
    let Some(Ceremony::Login { user, state }) = headers
        .get("x-dd-ceremony")
        .and_then(|v| v.to_str().ok())
        .and_then(|id| app.ceremony_take(id))
    else {
        return (StatusCode::BAD_REQUEST, "no ceremony").into_response();
    };
    let result = match app.webauthn.finish_passkey_authentication(&cred, &state) {
        Ok(r) => r,
        Err(e) => return (StatusCode::UNAUTHORIZED, format!("refused: {e}")).into_response(),
    };
    // the signature counter moves; keep it, so a cloned authenticator shows
    let mut dir = app.load(&user).unwrap_or_default();
    for pk in dir.passkeys.iter_mut() {
        pk.update_credential(&result);
    }
    let _ = app.store(&user, &dir);
    let mut r = StatusCode::OK.into_response();
    r.headers_mut().insert(
        "set-cookie",
        HeaderValue::from_str(&app.sessions.issue(&user)).unwrap(),
    );
    r
}

async fn logout(State(app): State<Arc<App>>) -> Response {
    let mut r = Redirect::to("/").into_response();
    r.headers_mut().insert(
        "set-cookie",
        HeaderValue::from_str(&app.sessions.clear()).unwrap(),
    );
    r
}

#[derive(Deserialize)]
struct Authorize {
    client_id: String,
    redirect_uri: String,
    state: Option<String>,
    nonce: Option<String>,
    response_type: Option<String>,
}

async fn oidc_authorize(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    Query(q): Query<Authorize>,
) -> Response {
    let Some(issuer) = app.oidc.as_ref() else {
        return StatusCode::NOT_FOUND.into_response();
    };
    if q.client_id != issuer.client_id
        || q.redirect_uri != issuer.redirect_uri
        || q.response_type.as_deref() != Some("code")
    {
        return (StatusCode::BAD_REQUEST, "unknown client or redirect").into_response();
    }
    let cookie = headers.get("cookie").and_then(|v| v.to_str().ok());
    let Some(user) = app.sessions.user(cookie) else {
        // no session here yet: passkey first, then back to this exact url
        let here = headers
            .get("x-original-uri")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("/")
            .to_string();
        return Redirect::to(&format!("/_dd/login?rd={}", urlencode(&here))).into_response();
    };
    let code = issuer.code(&user, q.nonce);
    let mut to = format!("{}?code={}", q.redirect_uri, urlencode(&code));
    if let Some(st) = q.state {
        to.push_str(&format!("&state={}", urlencode(&st)));
    }
    Redirect::to(&to).into_response()
}

#[derive(Deserialize)]
struct TokenReq {
    grant_type: String,
    code: String,
    client_id: Option<String>,
    client_secret: Option<String>,
}

async fn oidc_token(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    Form(f): Form<TokenReq>,
) -> Response {
    let Some(issuer) = app.oidc.as_ref() else {
        return StatusCode::NOT_FOUND.into_response();
    };
    // client_secret_basic or _post, whichever the plugin picks
    let (id, secret) = match headers
        .get(AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Basic "))
        .and_then(|b| B64.decode(b).ok())
        .and_then(|b| String::from_utf8(b).ok())
        .and_then(|s| {
            s.split_once(':')
                .map(|(a, b)| (a.to_string(), b.to_string()))
        }) {
        Some(p) => p,
        None => (
            f.client_id.unwrap_or_default(),
            f.client_secret.unwrap_or_default(),
        ),
    };
    if f.grant_type != "authorization_code" || !issuer.client_ok(&id, &secret) {
        return (
            StatusCode::UNAUTHORIZED,
            Json(serde_json::json!({"error": "invalid_client"})),
        )
            .into_response();
    }
    match issuer.redeem(&f.code) {
        Some(v) => Json(v).into_response(),
        None => (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "invalid_grant"})),
        )
            .into_response(),
    }
}

async fn oidc_userinfo(State(app): State<Arc<App>>, headers: HeaderMap) -> Response {
    let Some(issuer) = app.oidc.as_ref() else {
        return StatusCode::NOT_FOUND.into_response();
    };
    match bearer(&headers).and_then(|t| issuer.userinfo(t)) {
        Some(v) => Json(v).into_response(),
        None => StatusCode::UNAUTHORIZED.into_response(),
    }
}

async fn oidc_discovery(State(app): State<Arc<App>>) -> Response {
    match app.oidc.as_ref() {
        Some(i) => Json(i.discovery()).into_response(),
        None => StatusCode::NOT_FOUND.into_response(),
    }
}

async fn oidc_jwks(State(app): State<Arc<App>>) -> Response {
    match app.oidc.as_ref() {
        Some(i) => Json(i.jwks()).into_response(),
        None => StatusCode::NOT_FOUND.into_response(),
    }
}

fn urlencode(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

#[tokio::main]
async fn main() -> Result<()> {
    let bind: std::net::SocketAddr = env_or("VERIFY_BIND", "127.0.0.1:4181").parse()?;
    let dir: PathBuf = env("VERIFY_DIR")?.into();
    std::fs::create_dir_all(&dir)?;
    let directory = Arc::new(directory::Directory::open(dir.clone())?);
    // a box with nothing else on it: no domain, no sessions, no secrets. It
    // serves entries and accepts the ones that verify, and that is all.
    if env_or("VERIFY_ROLE", "full") == "directory" {
        let router = Router::new()
            .route("/health", get(|| async { "ok" }))
            .merge(directory::router(directory));
        let listener = tokio::net::TcpListener::bind(bind).await?;
        eprintln!("directory listening on {bind}");
        axum::serve(listener, router).await?;
        return Ok(());
    }
    let state_dir = dir
        .parent()
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| dir.clone());
    // the browser login's scope: the whole domain, so one passkey login covers
    // every service on this box, and the cookie rides along to all of them
    let domain = env("VERIFY_DOMAIN")?;
    let rp_origin = Url::parse(&format!("https://{domain}"))?;
    let webauthn = WebauthnBuilder::new(&domain, &rp_origin)?
        .rp_name("Distributed Datacenter")
        .allow_subdomains(true)
        .build()?;
    let oidc = match std::env::var("VERIFY_OIDC_ISSUER") {
        Ok(iss) => Some(oidc::Issuer::open(
            &state_dir,
            iss,
            env("VERIFY_OIDC_CLIENT_ID")?,
            std::fs::read_to_string(env("VERIFY_OIDC_CLIENT_SECRET_FILE")?)?
                .trim()
                .to_string(),
            env("VERIFY_OIDC_REDIRECT")?,
        )?),
        Err(_) => None,
    };
    let app = Arc::new(App {
        http: reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .build()?,
        dir,
        directory: directory.clone(),
        upstream_auth: env("VERIFY_UPSTREAM_AUTH")?,
        sessions: session::Sessions::open(&state_dir, &domain)?,
        webauthn,
        ceremonies: Mutex::new(HashMap::new()),
        oidc,
    });
    let router = Router::new()
        .route("/verify", get(verify))
        .route("/health", get(|| async { "ok" }))
        // the browser side, served under /_dd/ on every vhost
        .route("/_dd/login", get(|| async { Html(pages::LOGIN) }))
        .route("/_dd/login/start", post(login_start))
        .route("/_dd/login/finish", post(login_finish))
        .route("/_dd/logout", get(logout))
        .route("/_dd/enrol", get(|| async { Html(pages::ENROL) }))
        .route("/_dd/enrol/start", post(enrol_start))
        .route("/_dd/enrol/finish", post(enrol_finish))
        // the per-box issuer for jellyfin
        .route(
            "/_dd/oidc/.well-known/openid-configuration",
            get(oidc_discovery),
        )
        .route("/_dd/oidc/authorize", get(oidc_authorize))
        .route("/_dd/oidc/token", post(oidc_token))
        .route("/_dd/oidc/userinfo", get(oidc_userinfo))
        .route("/_dd/oidc/jwks", get(oidc_jwks))
        .with_state(app)
        .merge(directory::router(directory));
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
