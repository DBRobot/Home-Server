//! dd-transcode: a box doing compute on one file for one viewer.
//!
//! The rule of the fleet is ciphertext at rest, plaintext only in memory
//! while working. A member's device asks this box to play something its
//! own player cannot: it sends the urls of the file's chunks (which it got
//! from the gate with its own token; this box holds no key to the bucket)
//! and the file key sealed to this box's ephemeral key. The box fetches,
//! decrypts in memory, feeds ffmpeg, and serves the HLS it makes from a
//! runtime directory that is wiped when the session ends. The library key
//! is never here; the file key lives as long as the session and is zeroed
//! with it.
//!
//!   GET  /key                    the box's current public key to seal to
//!   POST /session                { chunks: [url...], key: sealed, size }
//!                                -> { id, playlist }
//!   GET  /session/{id}/{file}    the playlist and its segments
//!   DELETE /session/{id}         over, wiped

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Json, Response};
use axum::routing::{get, post};
use serde::Deserialize;
use tokio::io::AsyncWriteExt as _;
use tokio::sync::Mutex;

/// a session nobody has fetched from in this long is over
const IDLE: Duration = Duration::from_secs(600);

struct Session {
    dir: PathBuf,
    last: Instant,
    child: Option<tokio::process::Child>,
}

struct App {
    public: String,
    secret: zeroize::Zeroizing<[u8; 32]>,
    root: PathBuf,
    ffmpeg: PathBuf,
    sessions: Mutex<HashMap<String, Session>>,
}

#[derive(Deserialize)]
struct Start {
    chunks: Vec<String>,
    /// the file key, sealed to this box's public key
    key: String,
    size: u64,
}

async fn key(State(app): State<Arc<App>>) -> Json<serde_json::Value> {
    Json(serde_json::json!({ "key": app.public }))
}

async fn start(State(app): State<Arc<App>>, Json(s): Json<Start>) -> Response {
    let file_key = match library::open_x25519(&app.secret, &s.key) {
        Ok(k) => k,
        Err(_) => {
            return (StatusCode::BAD_REQUEST, "the key is not sealed to this box").into_response();
        }
    };
    if s.chunks.is_empty() || s.chunks.len() > 100_000 {
        return (StatusCode::BAD_REQUEST, "no chunks").into_response();
    }
    let id = library::random_id();
    let dir = app.root.join(&id);
    if let Err(e) = std::fs::create_dir_all(&dir) {
        return (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response();
    }
    // ffmpeg reads the plain stream on stdin and writes HLS into the session
    // directory; the first segments appear within seconds, the playlist
    // grows as it goes
    let mut child = match tokio::process::Command::new(&app.ffmpeg)
        .args([
            "-hide_banner",
            "-loglevel",
            "error",
            "-i",
            "pipe:0",
            "-c:v",
            "libx264",
            "-preset",
            "veryfast",
            "-crf",
            "23",
            "-c:a",
            "aac",
            "-ac",
            "2",
            "-f",
            "hls",
            "-hls_time",
            "4",
            "-hls_playlist_type",
            "event",
            "-hls_segment_filename",
        ])
        .arg(dir.join("%05d.ts"))
        .arg(dir.join("index.m3u8"))
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::inherit())
        .kill_on_drop(true)
        .spawn()
    {
        Ok(c) => c,
        Err(e) => {
            return (StatusCode::INTERNAL_SERVER_ERROR, format!("ffmpeg: {e}")).into_response();
        }
    };
    let mut stdin = child.stdin.take().expect("piped stdin");
    let chunks = s.chunks.clone();
    let expect = s.size;
    let sid = id.clone();
    // the feeder: fetch, decrypt, write; the key dies with this task
    tokio::spawn(async move {
        let http = reqwest::Client::new();
        let mut fed = 0u64;
        for (n, url) in chunks.iter().enumerate() {
            let sealed = match http
                .get(url)
                .send()
                .await
                .and_then(|r| r.error_for_status())
            {
                Ok(r) => match r.bytes().await {
                    Ok(b) => b,
                    Err(e) => {
                        eprintln!("transcode {sid}: chunk {n}: {e}");
                        break;
                    }
                },
                Err(e) => {
                    eprintln!("transcode {sid}: chunk {n}: {e}");
                    break;
                }
            };
            let plain = match library::open_chunk(&file_key, n as u64, &sealed) {
                Ok(p) => p,
                Err(_) => {
                    eprintln!("transcode {sid}: chunk {n} does not open");
                    break;
                }
            };
            fed += plain.len() as u64;
            if stdin.write_all(&plain).await.is_err() {
                break;
            }
        }
        let _ = stdin.shutdown().await;
        if fed != expect {
            eprintln!("transcode {sid}: fed {fed} of {expect} bytes");
        }
        drop(file_key);
    });
    app.sessions.lock().await.insert(
        id.clone(),
        Session {
            dir,
            last: Instant::now(),
            child: Some(child),
        },
    );
    Json(serde_json::json!({ "id": id, "playlist": format!("/session/{id}/index.m3u8") }))
        .into_response()
}

async fn serve(State(app): State<Arc<App>>, Path((id, file)): Path<(String, String)>) -> Response {
    if !file.chars().all(|c| c.is_ascii_alphanumeric() || c == '.') {
        return StatusCode::NOT_FOUND.into_response();
    }
    let dir = {
        let mut s = app.sessions.lock().await;
        match s.get_mut(&id) {
            Some(sess) => {
                sess.last = Instant::now();
                sess.dir.clone()
            }
            None => return StatusCode::NOT_FOUND.into_response(),
        }
    };
    let ty = if file.ends_with(".m3u8") {
        "application/vnd.apple.mpegurl"
    } else {
        "video/mp2t"
    };
    match tokio::fs::read(dir.join(&file)).await {
        Ok(b) => ([("content-type", ty), ("cache-control", "no-store")], b).into_response(),
        Err(_) => StatusCode::NOT_FOUND.into_response(),
    }
}

async fn end(State(app): State<Arc<App>>, Path(id): Path<String>) -> StatusCode {
    match app.sessions.lock().await.remove(&id) {
        Some(s) => {
            wipe(s).await;
            StatusCode::NO_CONTENT
        }
        None => StatusCode::NOT_FOUND,
    }
}

async fn wipe(mut s: Session) {
    if let Some(mut c) = s.child.take() {
        let _ = c.kill().await;
    }
    let _ = tokio::fs::remove_dir_all(&s.dir).await;
}

#[tokio::main]
async fn main() -> Result<()> {
    let bind: std::net::SocketAddr = std::env::var("TRANSCODE_BIND")
        .unwrap_or_else(|_| "127.0.0.1:4190".into())
        .parse()
        .context("TRANSCODE_BIND")?;
    let root = PathBuf::from(
        std::env::var("TRANSCODE_DIR").unwrap_or_else(|_| "/run/dd-transcode".into()),
    );
    let ffmpeg =
        PathBuf::from(std::env::var("TRANSCODE_FFMPEG").unwrap_or_else(|_| "ffmpeg".into()));
    std::fs::create_dir_all(&root)?;
    // a fresh key every start: a session outlives nothing
    let (public, secret) = library::ephemeral();
    let app = Arc::new(App {
        public,
        secret,
        root,
        ffmpeg,
        sessions: Mutex::new(HashMap::new()),
    });
    let sweeper = app.clone();
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(Duration::from_secs(60)).await;
            let mut s = sweeper.sessions.lock().await;
            let gone: Vec<String> = s
                .iter()
                .filter(|(_, v)| v.last.elapsed() > IDLE)
                .map(|(k, _)| k.clone())
                .collect();
            for id in gone {
                if let Some(sess) = s.remove(&id) {
                    wipe(sess).await;
                }
            }
        }
    });
    let router = axum::Router::new()
        .route("/key", get(key))
        .route("/session", post(start))
        .route("/session/{id}/{file}", get(serve))
        .route("/session/{id}", axum::routing::delete(end))
        .with_state(app);
    let listener = tokio::net::TcpListener::bind(bind).await?;
    eprintln!("dd-transcode on {bind}");
    axum::serve(listener, router).await?;
    Ok(())
}
