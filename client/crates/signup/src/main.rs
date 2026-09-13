//! Self-service signup, in two processes.
//!
//! `signup serve` faces the network. It holds a READ-ONLY kanidm token: enough
//! to say "that name is taken", not enough to change anything. A valid request
//! becomes a file in a local spool. `signup work` faces nothing: it reads the
//! spool with the read-write token, checks each request against kanidm itself,
//! and refuses any whose target already carries a credential. So there is no
//! path from the network to an existing account - kanidm cannot scope a token
//! to "pending accounts only", but a process boundary can.
//!
//! Neither half ever sees, sets or carries a credential. The password or
//! passkey is set by the person, in kanidm's own ui, behind the link kanidm
//! sends them.
//!
//! New accounts land in a group that no oauth2 scope map mentions, so they can
//! log in and reach nothing until something adds them to `users`. That group
//! membership is the entire access decision - see modules/kanidm.nix.

mod kanidm;

use std::net::SocketAddr;

use anyhow::{Context, Result};
use axum::{
    Form, Json, Router,
    extract::State,
    http::{HeaderMap, StatusCode, header::ACCEPT},
    response::{Html, IntoResponse, Response},
    routing::get,
};
use serde::Deserialize;

const FORM: &str = include_str!("form.html");
const DONE: &str = include_str!("done.html");

#[derive(Clone)]
struct Config {
    group: String,
    photos_url: String,
    intent_ttl: u64,
    spool: std::path::PathBuf,
}

/// What crosses the process boundary. Plain data; the worker validates it
/// all over again and trusts none of it.
#[derive(serde::Serialize, serde::Deserialize)]
struct Request {
    username: String,
    display_name: String,
    email: String,
}

struct App {
    kanidm: kanidm::Client,
    cfg: Config,
}

#[derive(Deserialize)]
struct SignupForm {
    username: String,
    display_name: String,
    email: String,
}

fn env(key: &str) -> Result<String> {
    std::env::var(key).with_context(|| format!("{key} is not set"))
}

fn env_or(key: &str, default: &str) -> String {
    std::env::var(key).unwrap_or_else(|_| default.to_string())
}

/// Anything echoed back into a page goes through this. The form is the one
/// place an attacker chooses the bytes, and the result page repeats them.
fn escape(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            '&' => "&amp;".to_string(),
            '<' => "&lt;".to_string(),
            '>' => "&gt;".to_string(),
            '"' => "&quot;".to_string(),
            '\'' => "&#39;".to_string(),
            c => c.to_string(),
        })
        .collect()
}

/// Kanidm has its own name rules and its own denied-names list, and it is the
/// authority on both - this only rejects what is obviously not a username, so
/// the common mistakes get a useful answer without a round trip.
fn valid_username(s: &str) -> bool {
    let ok_len = (2..=62).contains(&s.chars().count());
    let first_is_alpha = s.chars().next().is_some_and(|c| c.is_ascii_lowercase());
    let body_ok = s
        .chars()
        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '-' | '_' | '.'));
    ok_len && first_is_alpha && body_ok
}

/// Not an attempt at RFC 5321. The address only has to be good enough to be
/// worth handing to a mail server; the enrolment link is what actually proves
/// the person can read it.
fn valid_email(s: &str) -> bool {
    let mut parts = s.split('@');
    let (Some(local), Some(domain), None) = (parts.next(), parts.next(), parts.next()) else {
        return false;
    };
    !local.is_empty()
        && domain.contains('.')
        && !domain.starts_with('.')
        && !domain.ends_with('.')
        && s.len() <= 254
        && !s.chars().any(|c| c.is_whitespace())
}

/// One answer, rendered as a page for a browser and as json for `dd signup`.
/// The machine-readable `status` is what the cli branches on, so it never has
/// to scrape the html.
struct Reply {
    code: StatusCode,
    status: &'static str,
    title: &'static str,
    body: String,
}

fn wants_json(headers: &HeaderMap) -> bool {
    headers
        .get(ACCEPT)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v.contains("application/json"))
}

fn render(headers: &HeaderMap, r: Reply, photos_url: &str) -> Response {
    if wants_json(headers) {
        return (
            r.code,
            Json(serde_json::json!({ "status": r.status, "message": strip_tags(&r.body) })),
        )
            .into_response();
    }
    (
        r.code,
        Html(
            DONE.replace("{{TITLE}}", r.title)
                .replace("{{BODY}}", &r.body)
                .replace("{{PHOTOS_URL}}", photos_url),
        ),
    )
        .into_response()
}

/// The message bodies carry <strong> for the page; json consumers want the
/// text. Only tags this file writes are involved - user input reaching a body
/// has already been through escape(), so there is nothing here to sanitise.
fn strip_tags(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut in_tag = false;
    for c in s.chars() {
        match c {
            '<' => in_tag = true,
            '>' => in_tag = false,
            c if !in_tag => out.push(c),
            _ => {}
        }
    }
    out
}

async fn form() -> Html<&'static str> {
    Html(FORM)
}

async fn signup(
    State(app): State<std::sync::Arc<App>>,
    headers: HeaderMap,
    Form(f): Form<SignupForm>,
) -> Response {
    let username = f.username.trim().to_lowercase();
    let display_name = f.display_name.trim();
    let email = f.email.trim();

    let invalid = |title, body: &str| Reply {
        code: StatusCode::BAD_REQUEST,
        status: "invalid",
        title,
        body: body.to_string(),
    };

    let reply = if !valid_username(&username) {
        invalid(
            "That username will not work",
            "Usernames start with a letter and use only lowercase letters, \
             numbers, dots, dashes and underscores.",
        )
    } else if display_name.is_empty() || display_name.chars().count() > 128 {
        invalid(
            "That name will not work",
            "A display name is required, up to 128 characters.",
        )
    } else if !valid_email(email) {
        invalid(
            "That email address will not work",
            "The enrolment link is sent to this address, so it has to be one you can read.",
        )
    } else {
        match app
            .kanidm
            .classify(&app.cfg.group, &username)
            .await
            .and_then(|outcome| {
                if outcome != kanidm::Outcome::NameTaken {
                    spool(
                        &app.cfg.spool,
                        &Request {
                            username: username.clone(),
                            display_name: display_name.to_string(),
                            email: email.to_string(),
                        },
                    )?;
                }
                Ok(outcome)
            }) {
            Ok(kanidm::Outcome::Created) | Ok(kanidm::Outcome::Resent) => Reply {
                code: StatusCode::OK,
                status: "created",
                title: "Check your email",
                body: format!(
                    "An enrolment link is on its way to <strong>{}</strong>. Open it to \
                     set a passkey or password for <strong>{}</strong>. The link expires \
                     in {} hours.",
                    escape(email),
                    escape(&username),
                    app.cfg.intent_ttl / 3600
                ),
            },
            Ok(kanidm::Outcome::NameTaken) => Reply {
                code: StatusCode::CONFLICT,
                status: "name_taken",
                title: "That username is taken",
                body: format!(
                    "Someone already has <strong>{}</strong>. Go back and pick another.",
                    escape(&username)
                ),
            },
            Err(e) => {
                // The reason goes to the journal, not to the caller: it names
                // internal endpoints and status codes.
                eprintln!("signup failed for {username}: {e:#}");
                Reply {
                    code: StatusCode::INTERNAL_SERVER_ERROR,
                    status: "error",
                    title: "Something went wrong",
                    body: "The account was not created. Try again in a few minutes.".to_string(),
                }
            }
        }
    };

    render(&headers, reply, &app.cfg.photos_url)
}

/// One file per request, written whole then renamed, so the worker never
/// reads a half-written one.
fn spool(dir: &std::path::Path, req: &Request) -> Result<()> {
    let id = format!(
        "{}-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_nanos(),
        std::process::id()
    );
    let tmp = dir.join(format!(".{id}.tmp"));
    let fin = dir.join(format!("{id}.json"));
    std::fs::write(&tmp, serde_json::to_vec(req)?).context("writing to the spool")?;
    std::fs::rename(&tmp, &fin).context("filing in the spool")?;
    Ok(())
}

/// The privileged half. Polls the spool; every request is re-validated and
/// re-classified with kanidm before anything is written, so a forged or stale
/// file can at most create a fresh empty account.
async fn work(app: std::sync::Arc<App>) -> Result<()> {
    eprintln!("worker watching {}", app.cfg.spool.display());
    loop {
        let mut entries: Vec<_> = std::fs::read_dir(&app.cfg.spool)
            .context("reading the spool")?
            .filter_map(Result::ok)
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|x| x == "json"))
            .collect();
        entries.sort();
        for path in entries {
            let outcome = process(&app, &path).await;
            match &outcome {
                Ok(o) => eprintln!("{}: {o}", path.display()),
                Err(e) => eprintln!("{}: FAILED: {e:#}", path.display()),
            }
            // consumed either way: a failure is logged, and the person can
            // simply sign up again - that is the retry path, by design
            let _ = std::fs::remove_file(&path);
        }
        tokio::time::sleep(std::time::Duration::from_secs(2)).await;
    }
}

async fn process(app: &App, path: &std::path::Path) -> Result<String> {
    let req: Request = serde_json::from_slice(&std::fs::read(path)?).context("parsing")?;
    let username = req.username.trim().to_lowercase();
    let display_name = req.display_name.trim();
    let email = req.email.trim();
    anyhow::ensure!(valid_username(&username), "rejected username");
    anyhow::ensure!(
        !display_name.is_empty() && display_name.chars().count() <= 128,
        "rejected display name"
    );
    anyhow::ensure!(valid_email(email), "rejected email");

    // The rule that makes the split worth anything: a target with a credential
    // is never touched, whatever the request claims. sign_up re-classifies
    // with this token and only acts on Created or Resent.
    match app
        .kanidm
        .sign_up(
            &app.cfg.group,
            &username,
            display_name,
            email,
            app.cfg.intent_ttl,
        )
        .await?
    {
        kanidm::Outcome::Created => Ok(format!("created {username}, enrolment mail queued")),
        kanidm::Outcome::Resent => Ok(format!("resent enrolment mail for {username}")),
        kanidm::Outcome::NameTaken => Ok(format!("ignored: {username} is taken")),
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let bind: SocketAddr = env_or("SIGNUP_BIND", "127.0.0.1:8083").parse()?;
    let kanidm_url = env("SIGNUP_KANIDM_URL")?;
    let token_file = env("SIGNUP_TOKEN_FILE")?;
    let token = std::fs::read_to_string(&token_file)
        .with_context(|| format!("reading {token_file}"))?
        .trim()
        .to_string();
    anyhow::ensure!(!token.is_empty(), "{token_file} is empty");

    let cfg = Config {
        group: env_or("SIGNUP_GROUP", "pending"),
        photos_url: env_or("SIGNUP_PHOTOS_URL", ""),
        // A day, not kanidm's default hour: the mail has to survive someone
        // signing up in the evening and reading it the next morning.
        intent_ttl: env_or("SIGNUP_INTENT_TTL", "86400").parse()?,
        spool: env("SIGNUP_SPOOL")?.into(),
    };

    let app = std::sync::Arc::new(App {
        kanidm: kanidm::Client::new(kanidm_url, token)?,
        cfg,
    });

    // `signup work` is the privileged half; everything else serves the page.
    if std::env::args().nth(1).as_deref() == Some("work") {
        return work(app).await;
    }

    let router = Router::new()
        .route("/", get(form).post(signup))
        .route("/health", get(|| async { "ok" }))
        .with_state(app);

    let listener = tokio::net::TcpListener::bind(bind).await?;
    eprintln!("signup listening on {bind}");
    axum::serve(listener, router).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escapes_everything_a_page_could_reinterpret() {
        assert_eq!(
            escape(r#"a"<script>x</script>&'b"#),
            "a&quot;&lt;script&gt;x&lt;/script&gt;&amp;&#39;b"
        );
        assert_eq!(escape("plain@example.com"), "plain@example.com");
    }

    #[test]
    fn usernames() {
        assert!(valid_username("alice"));
        assert!(valid_username("a.b-c_1"));
        assert!(!valid_username("9alice"), "must start with a letter");
        assert!(
            !valid_username("Alice"),
            "uppercase is normalised away first"
        );
        assert!(!valid_username("a"), "too short");
        assert!(!valid_username("al ice"));
        assert!(!valid_username("al/ice"));
        assert!(!valid_username(&"a".repeat(63)), "too long");
    }

    #[test]
    fn emails() {
        assert!(valid_email("a@b.com"));
        assert!(!valid_email("nope"));
        assert!(!valid_email("a@b"), "needs a dot in the domain");
        assert!(!valid_email("@b.com"));
        assert!(!valid_email("a@b.com."));
        assert!(!valid_email("a@.com"));
        assert!(!valid_email("a b@c.com"));
        assert!(!valid_email("a@b.com@c.com"));
    }
}
