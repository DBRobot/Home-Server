//! The encrypted git remote, end to end: real git, the real helper, a real
//! directory box, and the hostile moves a box could make.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};

static N: AtomicUsize = AtomicUsize::new(0);

fn scratch(what: &str) -> PathBuf {
    let p = std::env::temp_dir().join(format!(
        "dd-repo-{}-{}-{what}",
        std::process::id(),
        N.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::create_dir_all(&p).unwrap();
    p
}

/// A shell with the helper on PATH, one device's keyring, the test
/// directory, and the owner pinned (the backing repo is a plain path).
struct Env {
    keyring: PathBuf,
    cache: PathBuf,
    directory: String,
}

impl Env {
    fn cmd(&self, program: &str) -> Command {
        let helper_dir = Path::new(env!("CARGO_BIN_EXE_dd")).parent().unwrap();
        let path = format!(
            "{}:{}",
            helper_dir.display(),
            std::env::var("PATH").unwrap_or_default()
        );
        let mut c = Command::new(program);
        c.env("PATH", path)
            .env("DD_KEYRING_FILE", &self.keyring)
            .env("DD_DIRECTORIES", &self.directory)
            .env("DD_REPO_OWNER", "sarah")
            .env("XDG_CACHE_HOME", &self.cache)
            .env("GIT_AUTHOR_NAME", "t")
            .env("GIT_AUTHOR_EMAIL", "t@t")
            .env("GIT_COMMITTER_NAME", "t")
            .env("GIT_COMMITTER_EMAIL", "t@t");
        c
    }
    fn run(&self, dir: &Path, program: &str, args: &[&str]) -> (bool, String) {
        let o = self
            .cmd(program)
            .args(args)
            .current_dir(dir)
            .output()
            .unwrap();
        (
            o.status.success(),
            format!(
                "{}{}",
                String::from_utf8_lossy(&o.stdout),
                String::from_utf8_lossy(&o.stderr)
            ),
        )
    }
    fn ok(&self, dir: &Path, program: &str, args: &[&str]) -> String {
        let (ok, out) = self.run(dir, program, args);
        assert!(ok, "{program} {} failed:\n{out}", args.join(" "));
        out
    }
    fn fails(&self, dir: &Path, program: &str, args: &[&str], containing: &str) {
        let (ok, out) = self.run(dir, program, args);
        assert!(
            !ok,
            "{program} {} should have failed:\n{out}",
            args.join(" ")
        );
        assert!(out.contains(containing), "wanted {containing:?} in:\n{out}");
    }
    fn git(&self, dir: &Path, args: &[&str]) -> String {
        self.ok(dir, "git", args)
    }
}

fn device(directory: &str) -> Env {
    let d = scratch("dev");
    Env {
        keyring: d.join("keys.json"),
        cache: d.join("cache"),
        directory: directory.to_string(),
    }
}

/// The helper is a separate binary of the workspace; cargo builds it for
/// this test only if it is a dependency, so build it here.
fn ensure_helper() {
    let helper_dir = Path::new(env!("CARGO_BIN_EXE_dd")).parent().unwrap();
    if helper_dir.join("git-remote-dd").exists() {
        return;
    }
    let s = Command::new(env!("CARGO"))
        .args(["build", "-q", "-p", "git-remote-dd"])
        .status()
        .unwrap();
    assert!(s.success());
}

#[tokio::test(flavor = "multi_thread")]
async fn encrypted_remote_end_to_end() {
    ensure_helper();
    let dir = scratch("box").join("keys");
    let (addr, _task) = verify::start(verify::Config {
        bind: "127.0.0.1:0".parse().unwrap(),
        dir,
        peers: vec![],
        sync_secs: 300,
        domain: None,
        oidc: None,
    })
    .await
    .unwrap();
    let directory = format!("http://{addr}/_dd/directory");

    let laptop = device(&directory);
    let cwd = scratch("cwd");
    laptop.ok(
        &cwd,
        env!("CARGO_BIN_EXE_dd"),
        &[
            "identity",
            "new",
            "--name",
            "sarah",
            "--directory",
            &directory,
        ],
    );

    let backing = scratch("backing").join("backing.git");
    laptop.git(
        &cwd,
        &[
            "init",
            "-q",
            "--bare",
            "-b",
            "dd",
            backing.to_str().unwrap(),
        ],
    );
    let url = format!("dd::{}", backing.display());

    // push
    let work = scratch("work");
    laptop.git(&work, &["init", "-q", "-b", "main"]);
    std::fs::write(work.join("plan.txt"), "secret plans\n").unwrap();
    laptop.git(&work, &["add", "plan.txt"]);
    laptop.git(&work, &["commit", "-q", "-m", "one"]);
    laptop.git(&work, &["remote", "add", "origin", &url]);
    laptop.git(&work, &["push", "-q", "origin", "main"]);

    // the store holds opaque files only
    let files = laptop.git(&backing, &["ls-tree", "-r", "--name-only", "dd"]);
    assert!(
        files.contains("keys.json") && files.contains("manifest.enc") && files.contains("packs/"),
        "{files}"
    );
    let pack = files
        .lines()
        .find(|l| l.starts_with("packs/"))
        .unwrap()
        .to_string();
    let bytes = laptop
        .cmd("git")
        .args(["cat-file", "-p", &format!("dd:{pack}")])
        .current_dir(&backing)
        .output()
        .unwrap()
        .stdout;
    assert!(
        !bytes.windows(12).any(|w| w == b"secret plans"),
        "plaintext in the pack"
    );
    let manifest = laptop
        .cmd("git")
        .args(["cat-file", "-p", "dd:manifest.enc"])
        .current_dir(&backing)
        .output()
        .unwrap()
        .stdout;
    assert!(
        !manifest.windows(10).any(|w| w == b"refs/heads"),
        "plaintext in the manifest"
    );

    // clone and pull on the same device
    let clone = scratch("clone");
    laptop.git(&cwd, &["clone", "-q", &url, clone.to_str().unwrap()]);
    assert_eq!(
        std::fs::read_to_string(clone.join("plan.txt")).unwrap(),
        "secret plans\n"
    );
    std::fs::write(work.join("plan.txt"), "secret plans\nmore\n").unwrap();
    laptop.git(&work, &["commit", "-qam", "two"]);
    laptop.git(&work, &["push", "-q", "origin", "main"]);
    laptop.git(&clone, &["pull", "-q", "--ff-only", "origin", "main"]);
    assert!(
        laptop
            .git(&clone, &["log", "--oneline", "-1"])
            .contains("two")
    );

    // a device that is not a reader
    let phone = device(&directory);
    let phone_pub = phone
        .ok(&cwd, env!("CARGO_BIN_EXE_dd"), &["device", "show"])
        .lines()
        .nth(1)
        .unwrap()
        .trim()
        .to_string();
    {
        use std::io::Write as _;
        let root = laptop.ok(&cwd, env!("CARGO_BIN_EXE_dd"), &["identity", "export"]);
        let mut c = phone
            .cmd(env!("CARGO_BIN_EXE_dd"))
            .args(["identity", "import", "--name", "sarah"])
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::null())
            .spawn()
            .unwrap();
        c.stdin.take().unwrap().write_all(root.as_bytes()).unwrap();
        assert!(c.wait().unwrap().success());
    }
    let pc = scratch("pc");
    phone.fails(
        &cwd,
        "git",
        &["clone", "-q", &url, pc.to_str().unwrap()],
        "is not a reader",
    );

    // shared in, the phone reads and pushes; the laptop pulls it
    laptop.ok(
        &cwd,
        env!("CARGO_BIN_EXE_dd"),
        &["repo", "share", &url, &phone_pub],
    );
    let pc = scratch("pc2");
    phone.git(&cwd, &["clone", "-q", &url, pc.to_str().unwrap()]);
    assert!(
        std::fs::read_to_string(pc.join("plan.txt"))
            .unwrap()
            .contains("secret plans")
    );
    std::fs::write(pc.join("plan.txt"), "from the phone\n").unwrap();
    phone.git(&pc, &["commit", "-qam", "three"]);
    phone.git(&pc, &["push", "-q", "origin", "main"]);
    laptop.git(&work, &["pull", "-q", "--ff-only", "origin", "main"]);
    assert_eq!(
        std::fs::read_to_string(work.join("plan.txt")).unwrap(),
        "from the phone\n"
    );
    let readers = phone.ok(&cwd, env!("CARGO_BIN_EXE_dd"), &["repo", "readers", &url]);
    assert!(readers.contains("state 4"), "{readers}");

    // a stranger holding the name but no key cannot share themselves in
    let stranger = device(&directory);
    let st_pub = stranger
        .ok(&cwd, env!("CARGO_BIN_EXE_dd"), &["device", "show"])
        .lines()
        .nth(1)
        .unwrap()
        .trim()
        .to_string();
    {
        use std::io::Write as _;
        let root = laptop.ok(&cwd, env!("CARGO_BIN_EXE_dd"), &["identity", "export"]);
        let mut c = stranger
            .cmd(env!("CARGO_BIN_EXE_dd"))
            .args(["identity", "import", "--name", "sarah"])
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::null())
            .spawn()
            .unwrap();
        c.stdin.take().unwrap().write_all(root.as_bytes()).unwrap();
        c.wait().unwrap();
    }
    stranger.fails(
        &cwd,
        env!("CARGO_BIN_EXE_dd"),
        &["repo", "share", &url, &st_pub],
        "is not a reader",
    );

    // non-fast-forward refused
    laptop.git(&clone, &["fetch", "-q", "origin"]);
    laptop.git(&clone, &["reset", "-q", "--hard", "HEAD"]);
    laptop.git(&clone, &["commit", "-q", "--allow-empty", "-m", "fork"]);
    let (ok, out) = laptop.run(&clone, "git", &["push", "origin", "main"]);
    assert!(!ok && out.contains("rejected"), "{out}");

    // the box serves an older state
    let head = laptop
        .git(&backing, &["rev-parse", "dd"])
        .trim()
        .to_string();
    laptop.git(&backing, &["update-ref", "refs/heads/dd", "dd~1"]);
    laptop.fails(&work, "git", &["fetch", "origin"], "refusing");
    laptop.git(&backing, &["update-ref", "refs/heads/dd", &head]);

    // the box forges a reader list
    let keys = laptop.git(&backing, &["cat-file", "-p", "dd:keys.json"]);
    let mut forged: serde_json::Value = serde_json::from_str(&keys).unwrap();
    let sealed = forged["signers"][0]["sealed_key"].clone();
    forged["signers"]
        .as_array_mut()
        .unwrap()
        .push(serde_json::json!({
            "fingerprint": "stranger", "public_key": st_pub, "sealed_key": sealed
        }));
    let oid = {
        use std::io::Write as _;
        let mut c = laptop
            .cmd("git")
            .args(["hash-object", "-w", "--stdin"])
            .current_dir(&backing)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .spawn()
            .unwrap();
        c.stdin
            .take()
            .unwrap()
            .write_all(forged.to_string().as_bytes())
            .unwrap();
        String::from_utf8(c.wait_with_output().unwrap().stdout)
            .unwrap()
            .trim()
            .to_string()
    };
    let top = laptop.git(&backing, &["ls-tree", "dd"]);
    let listing: String = top
        .lines()
        .filter(|l| !l.ends_with("\tkeys.json"))
        .map(|l| format!("{l}\n"))
        .chain(std::iter::once(format!("100644 blob {oid}\tkeys.json\n")))
        .collect();
    let tree = {
        use std::io::Write as _;
        let mut c = laptop
            .cmd("git")
            .args(["mktree"])
            .current_dir(&backing)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .spawn()
            .unwrap();
        c.stdin
            .take()
            .unwrap()
            .write_all(listing.as_bytes())
            .unwrap();
        String::from_utf8(c.wait_with_output().unwrap().stdout)
            .unwrap()
            .trim()
            .to_string()
    };
    let commit = laptop
        .git(
            &backing,
            &["commit-tree", &tree, "-p", &head, "-m", "forge"],
        )
        .trim()
        .to_string();
    laptop.git(&backing, &["update-ref", "refs/heads/dd", &commit]);
    let fresh = scratch("fresh");
    stranger.fails(
        &cwd,
        "git",
        &["clone", "-q", &url, fresh.to_str().unwrap()],
        "does not cover",
    );
}
