//! The verifier: answers nginx's auth_request for every service on this box.
//!
//! Two credentials exist and nothing else: a biscuit signed by one of the
//! devices in the person's own entry, or this box's session cookie from a
//! passkey login. The passkey itself was enrolled on a link that a device
//! signed. Nothing here can sign as anyone, so nothing here is worth taking;
//! no identity server is asked, because there is none.
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
use base64::engine::general_purpose::URL_SAFE_NO_PAD as B64_URL;
use biscuit_auth::{Biscuit, PublicKey, UnverifiedBiscuit, builder::Algorithm};
use serde::{Deserialize, Serialize};
use webauthn_rs::prelude::*;

struct App {
    directory: Arc<directory::Directory>,
    sessions: session::Sessions,
    webauthn: Webauthn,
    ceremonies: Mutex<HashMap<String, (Instant, Ceremony)>>,
    /// a passkey the browser just made, waiting for `dd enrol` to collect
    /// it and sign it into the entry. Keyed by the enrol token, so only the
    /// terminal that printed the link can pick it up. Never stored.
    pending: Mutex<HashMap<String, (Instant, identity::Passkey)>>,
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

/// Everything a box needs to know to run. main.rs reads it from the
/// environment; the tests build it directly and run boxes in-process.
pub struct Config {
    pub bind: std::net::SocketAddr,
    pub dir: PathBuf,
    /// the other boxes' directory urls
    pub peers: Vec<String>,
    pub sync_secs: u64,
    /// None: the directory alone. Some(domain): the full verifier, with the
    /// browser login scoped to that domain.
    pub domain: Option<String>,
    pub oidc: Option<OidcConfig>,
}

pub struct OidcConfig {
    pub issuer: String,
    pub client_id: String,
    pub client_secret: String,
    pub redirect: String,
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

    /// The passkeys in the user's signed entry, as webauthn-rs credentials.
    /// A record that does not parse is skipped, never fatal: one odd entry
    /// must not lock the others out.
    fn passkeys(&self, user: &str) -> Vec<Passkey> {
        self.entry(user)
            .ok()
            .flatten()
            .map(|e| {
                e.entry
                    .passkeys
                    .iter()
                    .filter_map(|p| serde_json::from_value(p.cred.clone()).ok())
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Verify a biscuit against the devices in the user's entry. `operation`
    /// is what the request is for: a token minted for enrolment carries a
    /// check that only "enrol" satisfies, so a leaked enrol link cannot read
    /// a file, and an access token is not an enrol link.
    fn verify_biscuit(&self, token: &str, operation: &str) -> Result<String> {
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
            .fact(format!("operation({operation:?})").as_str())
            .map_err(|e| anyhow!("{e}"))?
            // access takes any token of the user's; anything else demands a
            // token minted for exactly that, so an hour-long access token in
            // a script cannot enrol a passkey that outlives it
            .policy(
                if operation == "access" {
                    format!("allow if user({user:?})")
                } else {
                    format!("allow if user({user:?}), purpose({operation:?})")
                }
                .as_str(),
            )
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

    /// Who this request is, if anyone: a device-signed biscuit or this box's
    /// own session cookie. Nothing else counts.
    fn identify(&self, headers: &HeaderMap, operation: &str) -> Option<String> {
        if let Some(tok) = bearer(headers)
            && UnverifiedBiscuit::from_base64(tok).is_ok()
        {
            return match self.verify_biscuit(tok, operation) {
                Ok(u) => Some(u),
                Err(e) => {
                    eprintln!("biscuit refused: {e:#}");
                    None
                }
            };
        }
        let cookie = headers.get("cookie").and_then(|v| v.to_str().ok());
        self.sessions.user(cookie)
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
    match app.identify(&headers, "access") {
        Some(user) => {
            let mut r = StatusCode::OK.into_response();
            let v = HeaderValue::from_str(&user).unwrap();
            r.headers_mut()
                .insert("X-Auth-Request-Preferred-Username", v.clone());
            r.headers_mut().insert("X-Auth-Request-User", v);
            r
        }
        None => StatusCode::UNAUTHORIZED.into_response(),
    }
}

fn with_challenge(v: impl Serialize, ceremony: String) -> Response {
    let mut j = serde_json::to_value(v).unwrap_or_default();
    j["ceremony"] = serde_json::Value::String(ceremony);
    Json(j).into_response()
}

async fn enrol_start(State(app): State<Arc<App>>, headers: HeaderMap) -> Response {
    let Some(user) = app.identify(&headers, "enrol") else {
        return StatusCode::UNAUTHORIZED.into_response();
    };
    let existing = app.passkeys(&user);
    // a stable per-user id for the authenticator: derived from the name, so
    // the same person enrolling twice is the same user to the passkey
    let uid = {
        use sha2::Digest as _;
        let h = sha2::Sha256::digest(format!("dd-user:{user}").as_bytes());
        let mut b = [0u8; 16];
        b.copy_from_slice(&h[..16]);
        Uuid::from_bytes(b)
    };
    let exclude: Vec<CredentialID> = existing.iter().map(|p| p.cred_id().clone()).collect();
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
    // Not stored here. The browser made it; only the person's root can put
    // it in the entry, and that key is with the terminal that printed the
    // link. It collects the credential with the same token.
    let Some(key) = bearer(&headers).map(pending_key) else {
        return StatusCode::UNAUTHORIZED.into_response();
    };
    let id = B64_URL.encode(passkey.cred_id().as_slice());
    let cred = match serde_json::to_value(&passkey) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("enrol: {e}");
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    };
    let mut m = app.pending.lock().unwrap();
    m.retain(|_, (t, _)| t.elapsed() < Duration::from_secs(600));
    m.insert(
        key,
        (
            Instant::now(),
            identity::Passkey {
                id: id.clone(),
                cred,
                added: identity::now(),
            },
        ),
    );
    eprintln!("passkey {id} made for {user}; waiting for dd to sign it in");
    Json(serde_json::json!({ "id": id, "user": user })).into_response()
}

/// `dd enrol` polls this with its enrol token until the browser is done.
async fn enrol_result(State(app): State<Arc<App>>, headers: HeaderMap) -> Response {
    if app.identify(&headers, "enrol").is_none() {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    let Some(key) = bearer(&headers).map(pending_key) else {
        return StatusCode::UNAUTHORIZED.into_response();
    };
    match app.pending.lock().unwrap().remove(&key) {
        Some((_, pk)) => Json(pk).into_response(),
        None => StatusCode::NO_CONTENT.into_response(),
    }
}

fn pending_key(token: &str) -> String {
    use sha2::Digest as _;
    format!("{:x}", sha2::Sha256::digest(token.as_bytes()))
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
    let passkeys = app.passkeys(&user);
    if passkeys.is_empty() {
        return (
            StatusCode::NOT_FOUND,
            "no passkey in that name's entry - `dd enrol` adds one",
        )
            .into_response();
    }
    match app.webauthn.start_passkey_authentication(&passkeys) {
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
    // The signature counter is not written back: the credential lives in
    // the signed entry, which this box cannot update. Clone detection by
    // counter is given up for that; the passkey's private key never leaves
    // the authenticator either way.
    if let Err(e) = app.webauthn.finish_passkey_authentication(&cred, &state) {
        return (StatusCode::UNAUTHORIZED, format!("refused: {e}")).into_response();
    }
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

/// Bind, then serve forever. Returns once bound with the address (so a
/// test can bind port 0 and learn the port) and the task serving it.
pub async fn start(
    cfg: Config,
) -> Result<(std::net::SocketAddr, tokio::task::JoinHandle<Result<()>>)> {
    std::fs::create_dir_all(&cfg.dir)?;
    let directory = Arc::new(directory::Directory::open(
        cfg.dir.clone(),
        cfg.peers.clone(),
    )?);
    tokio::spawn(directory.clone().sync_forever(cfg.sync_secs));
    let listener = tokio::net::TcpListener::bind(cfg.bind).await?;
    let addr = listener.local_addr()?;
    // a box with nothing else on it: no domain, no sessions, no secrets. It
    // serves entries and accepts the ones that verify, and that is all.
    let Some(domain) = cfg.domain else {
        let router = Router::new()
            .route("/health", get(|| async { "ok" }))
            .merge(directory::router(directory));
        eprintln!("directory listening on {addr}");
        let task =
            tokio::spawn(async move { axum::serve(listener, router).await.map_err(Into::into) });
        return Ok((addr, task));
    };
    let state_dir = cfg
        .dir
        .parent()
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| cfg.dir.clone());
    // the browser login's scope: the whole domain, so one passkey login covers
    // every service on this box, and the cookie rides along to all of them
    let rp_origin = Url::parse(&format!("https://{domain}"))?;
    let webauthn = WebauthnBuilder::new(&domain, &rp_origin)?
        .rp_name("Distributed Datacenter")
        .allow_subdomains(true)
        .build()?;
    let oidc = match cfg.oidc {
        Some(o) => Some(oidc::Issuer::open(
            &state_dir,
            o.issuer,
            o.client_id,
            o.client_secret,
            o.redirect,
        )?),
        None => None,
    };
    let app = Arc::new(App {
        directory: directory.clone(),
        sessions: session::Sessions::open(&state_dir, &domain)?,
        webauthn,
        ceremonies: Mutex::new(HashMap::new()),
        pending: Mutex::new(HashMap::new()),
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
        .route("/_dd/enrol/result", get(enrol_result))
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
    eprintln!("verify listening on {addr}");
    let task = tokio::spawn(async move { axum::serve(listener, router).await.map_err(Into::into) });
    Ok((addr, task))
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
