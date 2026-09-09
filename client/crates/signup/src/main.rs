//! Self-service signup.
//!
//! What this service can do is create an account and ask kanidm to mail an
//! enrolment link. What it deliberately cannot do is authenticate as anyone: it
//! never sees, sets or carries a credential. The password or passkey is set by
//! the person, in kanidm's own ui, behind the link kanidm sends them.
//!
//! New accounts land in a group that no oauth2 scope map mentions, so they can
//! log in and reach nothing until something adds them to `users`. That group
//! membership is the entire access decision - see modules/kanidm.nix.

mod kanidm;

use std::net::SocketAddr;

use anyhow::{Context, Result};
use axum::{
    Form, Router,
    extract::State,
    http::StatusCode,
    response::{Html, IntoResponse},
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

fn page(title: &str, body: String, photos_url: &str) -> Html<String> {
    Html(
        DONE.replace("{{TITLE}}", title)
            .replace("{{BODY}}", &body)
            .replace("{{PHOTOS_URL}}", photos_url),
    )
}

async fn form() -> Html<&'static str> {
    Html(FORM)
}

async fn signup(
    State(app): State<std::sync::Arc<App>>,
    Form(f): Form<SignupForm>,
) -> impl IntoResponse {
    let username = f.username.trim().to_lowercase();
    let display_name = f.display_name.trim();
    let email = f.email.trim();

    if !valid_username(&username) {
        return (
            StatusCode::BAD_REQUEST,
            page(
                "That username will not work",
                "Usernames start with a letter and use only lowercase letters, \
                 numbers, dots, dashes and underscores."
                    .to_string(),
                &app.cfg.photos_url,
            ),
        );
    }
    if display_name.is_empty() || display_name.chars().count() > 128 {
        return (
            StatusCode::BAD_REQUEST,
            page(
                "That name will not work",
                "A display name is required, up to 128 characters.".to_string(),
                &app.cfg.photos_url,
            ),
        );
    }
    if !valid_email(email) {
        return (
            StatusCode::BAD_REQUEST,
            page(
                "That email address will not work",
                "The enrolment link is sent to this address, so it has to be one \
                 you can read."
                    .to_string(),
                &app.cfg.photos_url,
            ),
        );
    }

    match app
        .kanidm
        .sign_up(
            &app.cfg.group,
            &username,
            display_name,
            email,
            app.cfg.intent_ttl,
        )
        .await
    {
        Ok(kanidm::Outcome::Created) | Ok(kanidm::Outcome::Resent) => (
            StatusCode::OK,
            page(
                "Check your email",
                format!(
                    "An enrolment link is on its way to <strong>{}</strong>. Open it \
                     to set a passkey or password for <strong>{}</strong>. The link \
                     expires in {} hours.",
                    escape(email),
                    escape(&username),
                    app.cfg.intent_ttl / 3600
                ),
                &app.cfg.photos_url,
            ),
        ),
        Ok(kanidm::Outcome::NameTaken) => (
            StatusCode::CONFLICT,
            page(
                "That username is taken",
                format!(
                    "Someone already has <strong>{}</strong>. Go back and pick another.",
                    escape(&username)
                ),
                &app.cfg.photos_url,
            ),
        ),
        Err(e) => {
            // The reason goes to the journal, not to the page: it names
            // internal endpoints and status codes.
            eprintln!("signup failed for {username}: {e:#}");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                page(
                    "Something went wrong",
                    "The account was not created. Try again in a few minutes."
                        .to_string(),
                    &app.cfg.photos_url,
                ),
            )
        }
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
    };

    let app = std::sync::Arc::new(App {
        kanidm: kanidm::Client::new(kanidm_url, token)?,
        cfg,
    });

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
        assert!(!valid_username("Alice"), "uppercase is normalised away first");
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
