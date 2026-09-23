//! dd-transcode: a box doing compute on one file for one viewer.
//!
//! The rule of the fleet is ciphertext at rest, plaintext only in memory
//! while working. A member's device asks this box to play something its
//! own player cannot: it sends the urls of the file's chunks (which it got
//! from the gate with its own token; this box holds no key to the bucket)
//! and the file key sealed to this box's ephemeral key. The box fetches
//! and decrypts chunks in memory as ffmpeg asks for them (containers keep
//! their index at the end, so ffmpeg must seek: the plain bytes are served
//! to it over a second listener on localhost, with ranges, that nothing
//! else reaches), and serves the HLS it makes from a runtime directory
//! that is wiped when the session ends. The library key is never here;
//! the file key lives as long as the session and is zeroed with it.
//!
//!   GET  /key                    the box's current public key to seal to
//!   POST /session                { chunks: [url...], key: sealed, size }
//!                                -> { id, playlist }
//!   GET  /session/{id}/{file}    the playlist and its segments
//!   DELETE /session/{id}         over, wiped
//!
//!   GET  /plain/{id}             (the ffmpeg listener only) the file

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use axum::body::Body;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Json, Response};
use axum::routing::{get, post};
use serde::Deserialize;
use tokio::sync::Mutex;

/// a session nobody has fetched from in this long is over
const IDLE: Duration = Duration::from_secs(600);
/// decrypted chunks kept per session, for ffmpeg's seeks back
const KEEP: usize = 8;

struct Session {
    dir: PathBuf,
    last: Mutex<Instant>,
    child: Mutex<Option<tokio::process::Child>>,
    chunks: Vec<String>,
    key: library::Key,
    size: u64,
    cache: Mutex<HashMap<u64, Arc<Vec<u8>>>>,
}

impl Session {
    /// chunk n, plain, from the cache or the bucket
    async fn chunk(&self, http: &reqwest::Client, n: u64) -> Result<Arc<Vec<u8>>> {
        if let Some(c) = self.cache.lock().await.get(&n) {
            return Ok(c.clone());
        }
        let url = self.chunks.get(n as usize).context("no such chunk")?;
        let sealed = http
            .get(url)
            .send()
            .await?
            .error_for_status()?
            .bytes()
            .await?;
        let plain =
            Arc::new(library::open_chunk(&self.key, n, &sealed).context("chunk does not open")?);
        let mut cache = self.cache.lock().await;
        if cache.len() >= KEEP {
            // the furthest from this one goes; ffmpeg mostly moves forward
            let far = cache.keys().copied().max_by_key(|k| k.abs_diff(n));
            if let Some(k) = far {
                cache.remove(&k);
            }
        }
        cache.insert(n, plain.clone());
        Ok(plain)
    }
}

struct App {
    public: String,
    secret: zeroize::Zeroizing<[u8; 32]>,
    root: PathBuf,
    ffmpeg: PathBuf,
    plain: std::net::SocketAddr,
    http: reqwest::Client,
    sessions: Mutex<HashMap<String, Arc<Session>>>,
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
    if s.size > (s.chunks.len() as u64) * (library::CHUNK as u64) {
        return (StatusCode::BAD_REQUEST, "size does not fit the chunks").into_response();
    }
    let id = library::random_id();
    let dir = app.root.join(&id);
    if let Err(e) = std::fs::create_dir_all(&dir) {
        return (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response();
    }
    // ffmpeg reads the plain file from this process, seeking as it likes,
    // and writes HLS into the session directory; the first segments appear
    // within seconds, the playlist grows as it goes
    let child = match tokio::process::Command::new(&app.ffmpeg)
        .args([
            "-hide_banner",
            "-loglevel",
            "error",
            "-i",
            &format!("http://{}/plain/{id}", app.plain),
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
        .stdin(std::process::Stdio::null())
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
    app.sessions.lock().await.insert(
        id.clone(),
        Arc::new(Session {
            dir,
            last: Mutex::new(Instant::now()),
            child: Mutex::new(Some(child)),
            chunks: s.chunks,
            key: file_key,
            size: s.size,
            cache: Mutex::new(HashMap::new()),
        }),
    );
    Json(serde_json::json!({ "id": id, "playlist": format!("/session/{id}/index.m3u8") }))
        .into_response()
}

/// the plain bytes of a session's file for ffmpeg: whole, or a range
async fn plain(
    State(app): State<Arc<App>>,
    Path(id): Path<String>,
    headers: HeaderMap,
) -> Response {
    let Some(sess) = app.sessions.lock().await.get(&id).cloned() else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let size = sess.size;
    let range = headers
        .get("range")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("bytes="))
        .and_then(|v| {
            let (a, b) = v.split_once('-')?;
            let start: u64 = a.parse().ok()?;
            let end: u64 = if b.is_empty() {
                size.saturating_sub(1)
            } else {
                b.parse().ok()?
            };
            Some((start, end.min(size.saturating_sub(1))))
        });
    let (start, end) = match range {
        Some((s, e)) if s <= e && s < size => (s, e),
        Some(_) => {
            return (
                StatusCode::RANGE_NOT_SATISFIABLE,
                [("content-range", format!("bytes */{size}"))],
            )
                .into_response();
        }
        None if size == 0 => {
            return (StatusCode::OK, [("accept-ranges", "bytes")], Vec::new()).into_response();
        }
        None => (0, size - 1),
    };
    let http = app.http.clone();
    let stream = futures::stream::try_unfold((sess, start), move |(sess, at)| {
        let http = http.clone();
        async move {
            if at > end {
                return Ok::<_, anyhow::Error>(None);
            }
            let n = at / library::CHUNK as u64;
            let plain = sess.chunk(&http, n).await?;
            let from = (at - n * library::CHUNK as u64) as usize;
            let to = ((end + 1 - n * library::CHUNK as u64) as usize).min(plain.len());
            let piece = bytes::Bytes::copy_from_slice(&plain[from..to]);
            let next = at + (to - from) as u64;
            Ok(Some((piece, (sess, next))))
        }
    });
    let len = end + 1 - start;
    let status = if range.is_some() {
        StatusCode::PARTIAL_CONTENT
    } else {
        StatusCode::OK
    };
    let mut r = Response::builder()
        .status(status)
        .header("accept-ranges", "bytes")
        .header("content-length", len)
        .header("content-type", "application/octet-stream");
    if range.is_some() {
        r = r.header("content-range", format!("bytes {start}-{end}/{size}"));
    }
    r.body(Body::from_stream(stream))
        .unwrap_or_else(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response())
}

async fn serve(State(app): State<Arc<App>>, Path((id, file)): Path<(String, String)>) -> Response {
    if !file.chars().all(|c| c.is_ascii_alphanumeric() || c == '.') {
        return StatusCode::NOT_FOUND.into_response();
    }
    let Some(sess) = app.sessions.lock().await.get(&id).cloned() else {
        return StatusCode::NOT_FOUND.into_response();
    };
    *sess.last.lock().await = Instant::now();
    let dir = sess.dir.clone();
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

async fn wipe(s: Arc<Session>) {
    if let Some(mut c) = s.child.lock().await.take() {
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
    // the listener ffmpeg reads plaintext from: localhost, never proxied
    let plain_bind: std::net::SocketAddr = std::env::var("TRANSCODE_PLAIN_BIND")
        .unwrap_or_else(|_| "127.0.0.1:4191".into())
        .parse()
        .context("TRANSCODE_PLAIN_BIND")?;
    std::fs::create_dir_all(&root)?;
    // a fresh key every start: a session outlives nothing
    let (public, secret) = library::ephemeral();
    let app = Arc::new(App {
        public,
        secret,
        root,
        ffmpeg,
        plain: plain_bind,
        http: reqwest::Client::new(),
        sessions: Mutex::new(HashMap::new()),
    });
    let sweeper = app.clone();
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(Duration::from_secs(60)).await;
            let mut s = sweeper.sessions.lock().await;
            let mut gone = Vec::new();
            for (k, v) in s.iter() {
                if v.last.lock().await.elapsed() > IDLE {
                    gone.push(k.clone());
                }
            }
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
        .with_state(app.clone());
    let inner = axum::Router::new()
        .route("/plain/{id}", get(plain))
        .with_state(app);
    let listener = tokio::net::TcpListener::bind(bind).await?;
    let plain_listener = tokio::net::TcpListener::bind(plain_bind).await?;
    eprintln!("dd-transcode on {bind}, plain for ffmpeg on {plain_bind}");
    tokio::spawn(async move {
        if let Err(e) = axum::serve(plain_listener, inner).await {
            eprintln!("plain listener: {e}");
        }
    });
    axum::serve(listener, router).await?;
    Ok(())
}
