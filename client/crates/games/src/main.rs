use std::sync::Arc;

use anyhow::{Context, Result};
use axum::{
    Router,
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode},
    response::{Html, IntoResponse, Redirect, Response},
    routing::{get, post},
};
use games::{Config, Manager, Systemd, pages};

fn env(k: &str) -> Result<String> {
    std::env::var(k).with_context(|| format!("{k} is not set"))
}
fn env_or(k: &str, d: &str) -> String {
    std::env::var(k).unwrap_or_else(|_| d.to_string())
}

/// Who the gate says is asking; nginx sets it from the verifier's answer.
fn user(h: &HeaderMap) -> Option<String> {
    let u = h.get("x-dd-user")?.to_str().ok()?.trim().to_string();
    (!u.is_empty()).then_some(u)
}

#[derive(serde::Deserialize)]
struct Notice {
    n: Option<String>,
}

async fn index(State(m): State<Arc<Manager>>, h: HeaderMap, Query(q): Query<Notice>) -> Response {
    match user(&h) {
        Some(u) => Html(pages::index(&m, &u, q.n.as_deref())).into_response(),
        None => StatusCode::UNAUTHORIZED.into_response(),
    }
}

/// Back to the page, with what went wrong on it if something did.
fn back(r: Result<()>) -> Response {
    match r {
        Ok(()) => Redirect::to("/").into_response(),
        Err(e) => {
            let msg: String = format!("{e:#}")
                .bytes()
                .map(|b| match b {
                    b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' | b'-' | b'_' | b'.' => {
                        (b as char).to_string()
                    }
                    _ => format!("%{b:02X}"),
                })
                .collect();
            Redirect::to(&format!("/?n={msg}")).into_response()
        }
    }
}

async fn create(State(m): State<Arc<Manager>>, h: HeaderMap, Path(game): Path<String>) -> Response {
    let Some(u) = user(&h) else {
        return StatusCode::UNAUTHORIZED.into_response();
    };
    back(m.create(&u, &game).map(|_| ()))
}

macro_rules! act {
    ($name:ident, $call:ident) => {
        async fn $name(
            State(m): State<Arc<Manager>>,
            h: HeaderMap,
            Path(id): Path<String>,
        ) -> Response {
            let Some(u) = user(&h) else {
                return StatusCode::UNAUTHORIZED.into_response();
            };
            back(m.$call(&u, &id))
        }
    };
}
act!(start, start);
act!(stop, stop);
act!(delete, delete);

#[tokio::main]
async fn main() -> Result<()> {
    let catalogue = serde_json::from_slice(&std::fs::read(env("DD_GAMES_CATALOGUE")?)?)
        .context("DD_GAMES_CATALOGUE")?;
    let m = Manager::new(
        Config {
            dir: env_or("DD_GAMES_DIR", "/var/lib/dd-games").into(),
            catalogue,
            port_base: env_or("DD_GAMES_PORT_BASE", "27000").parse()?,
            port_count: env_or("DD_GAMES_PORT_COUNT", "200").parse()?,
            per_member: env_or("DD_GAMES_PER_MEMBER", "1").parse()?,
            memory_budget: env_or("DD_GAMES_MEMORY_MIB", "16384").parse()?,
            address: env("DD_GAMES_ADDRESS")?,
            home: env_or("DD_GAMES_HOME", "/"),
        },
        Box::new(Systemd),
    )?;
    let back_up = m.restore();
    eprintln!("games: {back_up} server(s) brought back up");
    let app = Router::new()
        .route("/", get(index))
        .route("/health", get(|| async { "ok" }))
        .route("/create/{game}", post(create))
        .route("/start/{id}", post(start))
        .route("/stop/{id}", post(stop))
        .route("/delete/{id}", post(delete))
        .with_state(m);
    let bind = env_or("DD_GAMES_BIND", "127.0.0.1:4182");
    let listener = tokio::net::TcpListener::bind(&bind).await?;
    eprintln!("games listening on {bind}");
    axum::serve(listener, app).await?;
    Ok(())
}
