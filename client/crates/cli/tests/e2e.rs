//! The identity model, end to end: real boxes in-process, the real `dd`
//! binary against them, and every hostile move we know of. This is the
//! test a change to any of it has to pass before it merges.

use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use webauthn_authenticator_rs::WebauthnAuthenticator;
use webauthn_authenticator_rs::softpasskey::SoftPasskey;
use webauthn_rs::prelude::{CreationChallengeResponse, RequestChallengeResponse, Url};

static N: AtomicUsize = AtomicUsize::new(0);

fn scratch(what: &str) -> PathBuf {
    let p = std::env::temp_dir().join(format!(
        "dd-e2e-{}-{}-{what}",
        std::process::id(),
        N.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::create_dir_all(&p).unwrap();
    p
}

/// A box. `full` runs the verifier with the browser login; otherwise the
/// directory alone.
struct Box_ {
    addr: std::net::SocketAddr,
    dir: PathBuf,
}

impl Box_ {
    async fn start(full: bool, peers: Vec<String>, sync_secs: u64) -> Self {
        let dir = scratch("box").join("keys");
        let (addr, _task) = verify::start(verify::Config {
            bind: "127.0.0.1:0".parse().unwrap(),
            dir: dir.clone(),
            peers,
            sync_secs,
            domain: full.then(|| "localhost".to_string()),
            oidc: None,
        })
        .await
        .unwrap();
        Self { addr, dir }
    }
    fn url(&self, path: &str) -> String {
        format!("http://{}{path}", self.addr)
    }
    fn directory(&self) -> String {
        self.url("/_dd/directory")
    }
}

/// One "device": its own key store file.
struct Device {
    keyring: PathBuf,
}

struct Ran {
    ok: bool,
    out: String,
}

impl Device {
    fn new() -> Self {
        Self {
            keyring: scratch("device").join("keys.json"),
        }
    }
    fn dd(&self, args: &[&str]) -> Ran {
        let o = Command::new(env!("CARGO_BIN_EXE_dd"))
            .args(args)
            .env("DD_KEYRING_FILE", &self.keyring)
            .output()
            .unwrap();
        Ran {
            ok: o.status.success(),
            out: format!(
                "{}{}",
                String::from_utf8_lossy(&o.stdout),
                String::from_utf8_lossy(&o.stderr)
            ),
        }
    }
    fn dd_ok(&self, args: &[&str]) -> String {
        let r = self.dd(args);
        assert!(r.ok, "dd {} failed:\n{}", args.join(" "), r.out);
        r.out
    }
    fn dd_fails(&self, args: &[&str], containing: &str) -> String {
        let r = self.dd(args);
        assert!(
            !r.ok,
            "dd {} should have failed:\n{}",
            args.join(" "),
            r.out
        );
        assert!(
            r.out.contains(containing),
            "dd {} failed for the wrong reason - wanted {containing:?} in:\n{}",
            args.join(" "),
            r.out
        );
        r.out
    }
    fn token(&self) -> String {
        self.dd_ok(&["token"]).trim().to_string()
    }
    fn public_key(&self) -> String {
        self.dd_ok(&["device", "show"])
            .lines()
            .nth(1)
            .unwrap()
            .trim()
            .to_string()
    }
}

fn dirs<'a>(boxes: impl IntoIterator<Item = &'a Box_>) -> Vec<String> {
    boxes
        .into_iter()
        .flat_map(|b| ["--directory".to_string(), b.directory()])
        .collect()
}

fn args<'a>(fixed: &[&'a str], extra: &'a [String]) -> Vec<&'a str> {
    fixed
        .iter()
        .copied()
        .chain(extra.iter().map(String::as_str))
        .collect()
}

/// The paper key out of `dd identity new` / `recover` output.
fn recovery_key(out: &str) -> String {
    out.lines()
        .map(str::trim)
        .find(|l| l.len() == 44 && l.ends_with('=') && !l.contains(' '))
        .expect("no recovery key printed")
        .to_string()
}

async fn verify_status(b: &Box_, token: &str) -> u16 {
    reqwest::Client::new()
        .get(b.url("/verify"))
        .bearer_auth(token)
        .send()
        .await
        .unwrap()
        .status()
        .as_u16()
}

async fn entry(b: &Box_, name: &str) -> Option<serde_json::Value> {
    let r = reqwest::get(format!("{}/{name}", b.directory()))
        .await
        .unwrap();
    if r.status().as_u16() == 404 {
        return None;
    }
    Some(r.json().await.unwrap())
}

async fn put_raw(b: &Box_, name: &str, body: &serde_json::Value) -> (u16, String) {
    let r = reqwest::Client::new()
        .put(format!("{}/{name}", b.directory()))
        .json(body)
        .send()
        .await
        .unwrap();
    let s = r.status().as_u16();
    (s, r.text().await.unwrap_or_default())
}

#[tokio::test(flavor = "multi_thread")]
async fn identity_devices_tokens_and_recovery() {
    let a = Box_::start(true, vec![], 300).await;
    let b = Box_::start(false, vec![], 300).await;
    let d = dirs([&a, &b]);
    let laptop = Device::new();
    let phone = Device::new();
    let stranger = Device::new();

    // birth: published to both, recovery key printed once
    let out = laptop.dd_ok(&args(&["identity", "new", "--name", "sarah"], &d));
    let paper = recovery_key(&out);
    assert_eq!(entry(&a, "sarah").await.unwrap()["entry"]["version"], 1);
    assert_eq!(entry(&b, "sarah").await.unwrap()["entry"]["version"], 1);
    assert_eq!(verify_status(&a, &laptop.token()).await, 200);

    // a second device: admitted by the root, signs its own tokens after
    let pk = phone.public_key();
    laptop.dd_ok(&args(&["device", "admit", &pk], &d));
    // the phone also gets the root, so it could admit devices itself
    let root = laptop.dd_ok(&["identity", "export"]);
    {
        use std::io::Write as _;
        let mut c = Command::new(env!("CARGO_BIN_EXE_dd"))
            .args(["identity", "import", "--name", "sarah"])
            .env("DD_KEYRING_FILE", &phone.keyring)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::null())
            .spawn()
            .unwrap();
        c.stdin.take().unwrap().write_all(root.as_bytes()).unwrap();
        assert!(c.wait().unwrap().success());
    }
    assert_eq!(verify_status(&a, &phone.token()).await, 200);
    assert_eq!(entry(&a, "sarah").await.unwrap()["entry"]["version"], 2);

    // a device nobody admitted, even one holding the name, is refused
    stranger.dd_ok(&["device", "show"]);
    {
        use std::io::Write as _;
        let mut c = Command::new(env!("CARGO_BIN_EXE_dd"))
            .args(["identity", "import", "--name", "sarah"])
            .env("DD_KEYRING_FILE", &stranger.keyring)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::null())
            .spawn()
            .unwrap();
        c.stdin.take().unwrap().write_all(root.as_bytes()).unwrap();
        c.wait().unwrap();
    }
    assert_eq!(verify_status(&a, &stranger.token()).await, 401);

    // forgery and replay at the directory
    let mut forged = entry(&a, "sarah").await.unwrap();
    forged["entry"]["version"] = serde_json::json!(9);
    forged["signature"] = serde_json::json!(base64_zero());
    let (s, why) = put_raw(&a, "sarah", &forged).await;
    assert_eq!(s, 403, "{why}");
    let mut replay = entry(&a, "sarah").await.unwrap();
    replay["entry"]["version"] = serde_json::json!(1);
    let (s, why) = put_raw(&a, "sarah", &replay).await;
    assert_eq!(s, 403, "{why}");

    // recovery: wrong paper key refused, right one installs a new root and
    // drops every device
    let recovered = Device::new();
    recovered.dd_ok(&["device", "show"]);
    run_with_stdin(
        &recovered,
        &args(
            &["identity", "recover", "--name", "sarah", "--key-stdin"],
            &d,
        ),
        "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=\n",
        false,
    );
    let out = run_with_stdin(
        &recovered,
        &args(
            &["identity", "recover", "--name", "sarah", "--key-stdin"],
            &d,
        ),
        &format!("{paper}\n"),
        true,
    );
    let _next_paper = recovery_key(&out);
    assert_eq!(entry(&b, "sarah").await.unwrap()["entry"]["version"], 3);
    assert_eq!(verify_status(&a, &laptop.token()).await, 401);
    assert_eq!(verify_status(&a, &phone.token()).await, 401);
    assert_eq!(verify_status(&a, &recovered.token()).await, 200);
    // the old root can do nothing any more
    laptop.dd_fails(&args(&["device", "admit", &pk], &d), "different root");
}

fn run_with_stdin(dev: &Device, a: &[&str], stdin: &str, want_ok: bool) -> String {
    use std::io::Write as _;
    let mut c = Command::new(env!("CARGO_BIN_EXE_dd"))
        .args(a)
        .env("DD_KEYRING_FILE", &dev.keyring)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    c.stdin.take().unwrap().write_all(stdin.as_bytes()).unwrap();
    let o = c.wait_with_output().unwrap();
    let out = format!(
        "{}{}",
        String::from_utf8_lossy(&o.stdout),
        String::from_utf8_lossy(&o.stderr)
    );
    assert_eq!(o.status.success(), want_ok, "dd {}:\n{out}", a.join(" "));
    out
}

fn base64_zero() -> String {
    use base64::Engine as _;
    base64::engine::general_purpose::STANDARD.encode([0u8; 64])
}

#[tokio::test(flavor = "multi_thread")]
async fn enrol_links_and_access_tokens_do_not_swap() {
    let a = Box_::start(true, vec![], 300).await;
    let d = dirs([&a]);
    let dev = Device::new();
    dev.dd_ok(&args(&["identity", "new", "--name", "tom"], &d));
    let access = dev.token();
    // dd enrol prints the link then waits; take the link and stop it
    let enrol = enrol_token(&dev, &a);
    let http = reqwest::Client::new();
    let post = |path: &str, tok: &str| http.post(a.url(path)).bearer_auth(tok.to_string()).send();
    assert_eq!(verify_status(&a, &access).await, 200);
    assert_eq!(
        verify_status(&a, &enrol).await,
        401,
        "an enrol link opens no file"
    );
    assert_eq!(
        post("/_dd/enrol/start", &enrol).await.unwrap().status(),
        200
    );
    assert_eq!(
        post("/_dd/enrol/start", &access).await.unwrap().status(),
        401,
        "an access token cannot enrol a passkey"
    );
    assert_eq!(
        http.get(a.url("/_dd/enrol/result"))
            .bearer_auth(&access)
            .send()
            .await
            .unwrap()
            .status(),
        401
    );
}

/// Start `dd enrol`, read the link it prints, kill it. Just the token.
fn enrol_token(dev: &Device, b: &Box_) -> String {
    use std::io::BufRead as _;
    let mut c = Command::new(env!("CARGO_BIN_EXE_dd"))
        .args([
            "enrol",
            "--url",
            &b.url("/_dd/enrol"),
            "--directory",
            &b.directory(),
        ])
        .env("DD_KEYRING_FILE", &dev.keyring)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn()
        .unwrap();
    let out = std::io::BufReader::new(c.stdout.take().unwrap());
    let mut tok = None;
    for line in out.lines() {
        let line = line.unwrap();
        if let Some(i) = line.find("?t=") {
            tok = Some(line[i + 3..].trim().to_string());
            break;
        }
    }
    let _ = c.kill();
    let _ = c.wait();
    tok.expect("dd enrol printed no link")
}

#[tokio::test(flavor = "multi_thread")]
async fn passkey_made_in_a_browser_is_signed_into_the_entry_and_logs_in() {
    let a = Box_::start(true, vec![], 300).await;
    let d = dirs([&a]);
    let dev = Device::new();
    dev.dd_ok(&args(&["identity", "new", "--name", "amy"], &d));

    // dd enrol runs for real this time: it prints the link, waits for the
    // credential, signs it in, publishes
    let mut enrol = Command::new(env!("CARGO_BIN_EXE_dd"))
        .args([
            "enrol",
            "--url",
            &a.url("/_dd/enrol"),
            "--directory",
            &a.directory(),
        ])
        .env("DD_KEYRING_FILE", &dev.keyring)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    // keep reading its stdout for the whole run: dropping the pipe would
    // make dd's later println fail
    use std::io::BufRead as _;
    let mut stdout = std::io::BufReader::new(enrol.stdout.take().unwrap());
    let mut line = String::new();
    let tok = loop {
        line.clear();
        assert!(
            stdout.read_line(&mut line).unwrap() > 0,
            "dd enrol ended early"
        );
        if let Some(i) = line.find("?t=") {
            break line[i + 3..].trim().to_string();
        }
    };

    // the browser: a software authenticator against the box's pages
    let origin = Url::parse("https://localhost").unwrap();
    let mut authenticator = SoftPasskey::new(true);
    let http = reqwest::Client::new();
    let r = http
        .post(a.url("/_dd/enrol/start"))
        .bearer_auth(&tok)
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 200);
    let mut j: serde_json::Value = r.json().await.unwrap();
    let ceremony = j["ceremony"].as_str().unwrap().to_string();
    j.as_object_mut().unwrap().remove("ceremony");
    let ccr: CreationChallengeResponse = serde_json::from_value(j).unwrap();
    let reg = authenticator
        .do_registration(origin.clone(), ccr)
        .expect("software authenticator registration");
    let r = http
        .post(a.url("/_dd/enrol/finish"))
        .bearer_auth(&tok)
        .header("x-dd-ceremony", &ceremony)
        .json(&reg)
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 200, "{}", r.text().await.unwrap_or_default());

    // dd collects it and signs it in
    let mut out = String::new();
    std::io::Read::read_to_string(&mut stdout, &mut out).unwrap();
    let status = enrol.wait().unwrap();
    let mut err = String::new();
    std::io::Read::read_to_string(&mut enrol.stderr.take().unwrap(), &mut err).unwrap();
    assert!(status.success(), "dd enrol:\n{out}{err}");
    assert!(out.contains("signed into your entry"), "{out}");
    let e = entry(&a, "amy").await.unwrap();
    assert_eq!(e["entry"]["version"], 2);
    assert_eq!(e["entry"]["passkeys"].as_array().unwrap().len(), 1);

    // login with it: the box builds the challenge from the entry
    let r = http
        .post(a.url("/_dd/login/start"))
        .json(&serde_json::json!({ "username": "amy" }))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 200);
    let mut j: serde_json::Value = r.json().await.unwrap();
    let ceremony = j["ceremony"].as_str().unwrap().to_string();
    j.as_object_mut().unwrap().remove("ceremony");
    let rcr: RequestChallengeResponse = serde_json::from_value(j).unwrap();
    let cred = authenticator
        .do_authentication(origin, rcr)
        .expect("software authenticator assertion");
    let r = http
        .post(a.url("/_dd/login/finish"))
        .header("x-dd-ceremony", &ceremony)
        .json(&cred)
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 200, "{}", r.text().await.unwrap_or_default());
    let cookie = r
        .headers()
        .get("set-cookie")
        .unwrap()
        .to_str()
        .unwrap()
        .split(';')
        .next()
        .unwrap()
        .to_string();
    let r = http
        .get(a.url("/verify"))
        .header("cookie", &cookie)
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 200);
    assert_eq!(
        r.headers()["x-auth-request-preferred-username"]
            .to_str()
            .unwrap(),
        "amy"
    );

    // removed from the entry, the passkey is gone everywhere at once
    dev.dd_ok(&args(
        &[
            "passkey",
            "remove",
            e["entry"]["passkeys"][0]["id"].as_str().unwrap(),
        ],
        &d,
    ));
    let r = http
        .post(a.url("/_dd/login/start"))
        .json(&serde_json::json!({ "username": "amy" }))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 404);
    // the cookie it earned is still this box's for its lifetime - that is
    // the stated trade; a stolen session is bounded by the box and the clock
    assert_eq!(verify_status(&a, "").await, 401);
}

#[tokio::test(flavor = "multi_thread")]
async fn boxes_share_the_directory_and_refuse_squats() {
    // a and b peer with each other, quickly
    let a_dir = scratch("a").join("keys");
    let b_dir = scratch("b").join("keys");
    // we need each other's addresses before starting: bind first
    let la = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let lb = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let (pa, pb) = (la.local_addr().unwrap(), lb.local_addr().unwrap());
    drop((la, lb));
    let (a, b) = tokio::join!(
        start_at(
            pa,
            a_dir,
            true,
            vec![format!("http://{pb}/_dd/directory")],
            1
        ),
        start_at(
            pb,
            b_dir,
            false,
            vec![format!("http://{pa}/_dd/directory")],
            1
        )
    );
    tokio::time::sleep(Duration::from_millis(1500)).await; // first pulls

    let sarah = Device::new();
    sarah.dd_ok(&[
        "identity",
        "new",
        "--name",
        "sarah",
        "--directory",
        &a.directory(),
    ]);
    // b pulls it
    wait_for(|| async { entry(&b, "sarah").await.is_some() }).await;

    // a stranger tries the name on b: the client sees it is taken
    let stranger = Device::new();
    stranger.dd_fails(
        &[
            "identity",
            "new",
            "--name",
            "sarah",
            "--directory",
            &b.directory(),
        ],
        "already has an entry",
    );

    // a box that never managed to pull its peer takes no new names
    let lonely = Box_::start(false, vec!["http://127.0.0.1:1/_dd/directory".into()], 1).await;
    stranger.dd_fails(
        &[
            "identity",
            "new",
            "--name",
            "tom",
            "--directory",
            &lonely.directory(),
        ],
        "did not take the update",
    );

    // an update via b alone reaches a
    let phone = Device::new();
    let pk = phone.public_key();
    sarah.dd_ok(&["device", "admit", &pk, "--directory", &b.directory()]);
    wait_for(|| async { entry(&a, "sarah").await.unwrap()["entry"]["version"] == 2 }).await;

    // a recovery published to a reaches b, which checks the old paper key
    let amy = Device::new();
    let out = amy.dd_ok(&[
        "identity",
        "new",
        "--name",
        "amy",
        "--directory",
        &a.directory(),
    ]);
    let paper = recovery_key(&out);
    wait_for(|| async { entry(&b, "amy").await.is_some() }).await;
    let amy2 = Device::new();
    amy2.dd_ok(&["device", "show"]);
    run_with_stdin(
        &amy2,
        &[
            "identity",
            "recover",
            "--name",
            "amy",
            "--key-stdin",
            "--directory",
            &a.directory(),
        ],
        &format!("{paper}\n"),
        true,
    );
    wait_for(|| async { entry(&b, "amy").await.unwrap()["entry"]["version"] == 2 }).await;
    assert!(entry(&b, "amy").await.unwrap()["recovery_signature"].is_string());

    // a brand-new box pointed at a fills itself, at whatever version things are
    let fresh = Box_::start(false, vec![a.directory()], 1).await;
    wait_for(|| async {
        entry(&fresh, "amy").await.is_some() && entry(&fresh, "sarah").await.is_some()
    })
    .await;
    assert_eq!(entry(&fresh, "amy").await.unwrap()["entry"]["version"], 2);

    // bob is born on a. the fresh box has not pulled yet, but it asks its
    // peer before taking a new name: a stranger's bob is refused there
    let bob = Device::new();
    bob.dd_ok(&[
        "identity",
        "new",
        "--name",
        "bob",
        "--directory",
        &a.directory(),
    ]);
    let sq = Device::new();
    sq.dd_fails(
        &[
            "identity",
            "new",
            "--name",
            "bob",
            "--directory",
            &fresh.directory(),
        ],
        "409",
    );

    // a forged newer entry pushed straight at the fresh box
    let mut forged = entry(&fresh, "amy").await.unwrap();
    forged["entry"]["version"] = serde_json::json!(9);
    forged["signature"] = serde_json::json!(base64_zero());
    assert_eq!(put_raw(&fresh, "amy", &forged).await.0, 403);
}

async fn start_at(
    addr: std::net::SocketAddr,
    dir: PathBuf,
    full: bool,
    peers: Vec<String>,
    sync_secs: u64,
) -> Box_ {
    let (addr, _task) = verify::start(verify::Config {
        bind: addr,
        dir: dir.clone(),
        peers,
        sync_secs,
        domain: full.then(|| "localhost".to_string()),
        oidc: None,
    })
    .await
    .unwrap();
    Box_ { addr, dir }
}

async fn wait_for<F, Fut>(mut cond: F)
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = bool>,
{
    for _ in 0..100 {
        if cond().await {
            return;
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
    panic!("condition never became true");
}

#[tokio::test(flavor = "multi_thread")]
async fn publish_brings_a_lagging_box_up() {
    let a = Box_::start(false, vec![], 300).await;
    let b = Box_::start(false, vec![], 300).await;
    let dev = Device::new();
    dev.dd_ok(&[
        "identity",
        "new",
        "--name",
        "kim",
        "--directory",
        &a.directory(),
    ]);
    assert!(entry(&b, "kim").await.is_none());
    let out = dev.dd_ok(&args(&["identity", "publish"], &dirs([&a, &b])));
    assert!(out.contains("accepted version 1"), "{out}");
    assert!(entry(&b, "kim").await.is_some());
    let _ = &a.dir;
}
