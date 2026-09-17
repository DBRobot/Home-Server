//! `dd release`: the fleet moves when this machine says so.
//!
//! `publish` builds every box in fleet/boxes.json from one commit, puts the
//! closures where the boxes fetch from, and pushes one signed file naming
//! the commit, a counter and every box's store path and nar hash. The
//! agent on each box does the rest. CI never does this: a box's runner
//! would be signing what a box built.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, bail, ensure};
use auth::KeyStore as _;
use clap::Subcommand;

/// keyring account holding the release key, base64
const ACCOUNT: &str = "release-key";
pub const DEFAULT_URL: &str = "https://git.distributed-datacenter.duckdns.org/david/Home-Server/raw/branch/releases/current.json";
const DEFAULT_CACHE_HOST: &str = "admin@10.10.10.2";

#[derive(Subcommand)]
pub enum ReleaseCmd {
    /// Make the release key here and write its public half to
    /// fleet/release.pub, which every box is built with.
    Init,
    /// Build every box from a ref, push the closures to the cache, sign
    /// and publish the release.
    Publish {
        /// a local branch that matches the forge's
        #[arg(long, default_value = "main")]
        r#ref: String,
        /// where the closures go; harmonia serves this box's store
        #[arg(long, default_value = DEFAULT_CACHE_HOST)]
        cache_host: String,
        /// where boxes read the release from
        #[arg(long, default_value = DEFAULT_URL)]
        url: String,
        /// build and sign, print the release, push nothing
        #[arg(long)]
        dry_run: bool,
    },
    /// Sign a payload file with the release key. `publish` does this
    /// itself; the vm test drives it directly.
    Sign {
        payload: PathBuf,
        #[arg(long)]
        out: PathBuf,
    },
    /// The release the boxes see.
    Show {
        #[arg(long, default_value = DEFAULT_URL)]
        url: String,
    },
    /// What every box says it runs, from each box's own metrics over the
    /// tailnet, next to what the current release says it should.
    Status {
        #[arg(long, default_value = DEFAULT_URL)]
        url: String,
    },
}

pub fn run(cmd: ReleaseCmd, repo: &str, keys: &auth::Store) -> Result<()> {
    match cmd {
        ReleaseCmd::Init => {
            if keys.get(ACCOUNT)?.is_some() {
                bail!("there is a release key here already");
            }
            let k = release::generate();
            keys.set(ACCOUNT, &release::encode_secret(&k))?;
            let public = release::encode_public(&k.verifying_key());
            let root = repo_root(repo)?;
            let path = root.join("fleet/release.pub");
            std::fs::write(&path, format!("{public}\n"))?;
            println!("release key made; public half in {}", path.display());
            println!("{public}");
        }
        ReleaseCmd::Sign { payload, out } => {
            let key = load(keys)?;
            let payload: release::Payload =
                serde_json::from_slice(&std::fs::read(&payload).context("reading payload")?)?;
            let signed = release::sign(payload, &key)?;
            std::fs::write(&out, serde_json::to_vec_pretty(&signed)?)?;
            println!(
                "signed release {} into {}",
                signed.payload.counter,
                out.display()
            );
        }
        ReleaseCmd::Show { url } => {
            let signed = fetch(&url)?.context("no release published yet")?;
            print(&signed);
        }
        ReleaseCmd::Status { url } => status(repo, &url)?,
        ReleaseCmd::Publish {
            r#ref,
            cache_host,
            url,
            dry_run,
        } => publish(repo, &r#ref, &cache_host, &url, dry_run, keys)?,
    }
    Ok(())
}

fn publish(
    repo: &str,
    r#ref: &str,
    cache_host: &str,
    url: &str,
    dry_run: bool,
    keys: &auth::Store,
) -> Result<()> {
    let key = load(keys)?;
    let root = repo_root(repo)?;
    let rev = git(&root, &["rev-parse", &format!("refs/heads/{ref}")])
        .with_context(|| format!("no local branch {ref}"))?;
    let forge_rev =
        git(&root, &["rev-parse", &format!("refs/remotes/forge/{ref}")]).unwrap_or_default();
    ensure!(
        rev == forge_rev,
        "local {ref} ({}) is not what the forge has - push it first",
        &rev[..12]
    );

    let boxes: serde_json::Value =
        serde_json::from_slice(&std::fs::read(root.join("fleet/boxes.json"))?)?;
    let names: Vec<String> = boxes
        .as_object()
        .context("boxes.json is not an object")?
        .keys()
        .cloned()
        .collect();

    // every box, from git, never from the working tree
    let mut built = BTreeMap::new();
    for name in &names {
        eprintln!("== build {name} from {ref} ({})", &rev[..12]);
        let flake = format!(
            "git+file://{}?ref={ref}#nixosConfigurations.{name}.config.system.build.toplevel",
            root.display()
        );
        let path = sh("nix", &["build", "--no-link", "--print-out-paths", &flake])
            .with_context(|| format!("building {name}"))?
            .trim()
            .to_owned();
        let info = sh("nix", &["path-info", "--json", &path])?;
        let nar_hash = release::nar_hash_from_path_info(&info, &path)?;
        eprintln!("   {path}");
        built.insert(name.clone(), release::BoxRelease { path, nar_hash });
    }

    let counter = match fetch(url)? {
        Some(current) => current.payload.counter + 1,
        None => 1,
    };
    let signed = release::sign(
        release::Payload {
            counter,
            rev: rev.clone(),
            issued: release::now(),
            boxes: built,
        },
        &key,
    )?;
    let body = serde_json::to_string_pretty(&signed)?;
    if dry_run {
        println!("{body}");
        return Ok(());
    }

    eprintln!("== copy the closures to {cache_host}");
    copy_closures(
        cache_host,
        &signed
            .payload
            .boxes
            .values()
            .map(|b| b.path.as_str())
            .collect::<Vec<_>>(),
    )?;

    eprintln!("== publish release {counter}");
    let wt = std::env::temp_dir().join(format!("dd-release-{}", std::process::id()));
    // the releases branch: history of every release, never merged anywhere
    let have_branch = git(
        &root,
        &["ls-remote", "--exit-code", "--heads", "forge", "releases"],
    )
    .is_ok();
    if have_branch {
        git(&root, &["fetch", "-q", "forge", "releases"])?;
        git(
            &root,
            &[
                "worktree",
                "add",
                "-q",
                "--detach",
                wt.to_str().unwrap(),
                "forge/releases",
            ],
        )?;
    } else {
        git(
            &root,
            &["worktree", "add", "-q", "--orphan", wt.to_str().unwrap()],
        )?;
    }
    let result = (|| -> Result<()> {
        std::fs::create_dir_all(wt.join("history"))?;
        std::fs::write(wt.join("current.json"), &body)?;
        std::fs::write(wt.join(format!("history/{counter}.json")), &body)?;
        git(&wt, &["add", "current.json", "history"])?;
        git(
            &wt,
            &[
                "commit",
                "-q",
                "-m",
                &format!("release {counter}: {}", &rev[..12]),
            ],
        )?;
        git(&wt, &["push", "-q", "forge", "HEAD:refs/heads/releases"])?;
        Ok(())
    })();
    let _ = git(
        &root,
        &["worktree", "remove", "--force", wt.to_str().unwrap()],
    );
    result?;
    print(&signed);
    Ok(())
}

/// Each box answers for itself: its prometheus holds what its agent last
/// wrote. Nothing aggregates; a box that is off says nothing.
fn status(repo: &str, url: &str) -> Result<()> {
    let root = repo_root(repo)?;
    let boxes: serde_json::Value =
        serde_json::from_slice(&std::fs::read(root.join("fleet/boxes.json"))?)?;
    let current = fetch(url)?;
    match &current {
        Some(s) => println!(
            "release {}  commit {}",
            s.payload.counter,
            &s.payload.rev[..12.min(s.payload.rev.len())]
        ),
        None => println!("no release published yet"),
    }
    for (name, b) in boxes.as_object().context("boxes.json is not an object")? {
        let tailnet = b["tailnet"].as_str().unwrap_or("-");
        let q = |expr: &str| -> Option<serde_json::Value> {
            let out = Command::new("curl")
                .args(["-sf", "-m", "5", "--get", "--data-urlencode"])
                .arg(format!("query={expr}"))
                .arg(format!("http://{tailnet}:9090/api/v1/query"))
                .output()
                .ok()?;
            let v: serde_json::Value = serde_json::from_slice(&out.stdout).ok()?;
            v["data"]["result"].as_array()?.first().cloned()
        };
        let counter = q("dd_agent_counter")
            .and_then(|r| r["value"][1].as_str().map(str::to_owned))
            .unwrap_or_else(|| "-".into());
        let info = q("dd_agent_info");
        let running = info
            .as_ref()
            .and_then(|r| r["metric"]["path"].as_str().map(str::to_owned))
            .unwrap_or_else(|| "?".into());
        let result = info
            .as_ref()
            .and_then(|r| r["metric"]["result"].as_str().map(str::to_owned))
            .unwrap_or_else(|| "unreachable".into());
        let want = current
            .as_ref()
            .and_then(|s| s.payload.boxes.get(name))
            .map(|b| b.path.as_str());
        let verdict = match want {
            Some(w) if w == running => "current",
            Some(_) => "behind",
            None => "-",
        };
        println!("{name}  counter {counter}  last run {result}  {verdict}");
        println!("  runs {running}");
    }
    Ok(())
}

/// What the box lacks, exported here and imported there as root. `nix copy`
/// over ssh would need the paths signed by a key the box's nix trusts;
/// the release file carries the nar hashes, which is what the agents
/// check, so this is bin/deploy's way.
fn copy_closures(host: &str, paths: &[&str]) -> Result<()> {
    use std::io::Write as _;
    use std::process::Stdio;
    let mut args = vec!["-qR"];
    args.extend(paths);
    let all = sh("nix-store", &args)?;
    let mut probe = Command::new("ssh")
        .arg(host)
        .arg("xargs -n1 sh -c 'test -e \"$0\" || echo \"$0\"'")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .context("ssh to the cache box")?;
    probe.stdin.take().unwrap().write_all(all.as_bytes())?;
    let out = probe.wait_with_output()?;
    ensure!(out.status.success(), "asking {host} what it lacks");
    let missing: Vec<&str> = std::str::from_utf8(&out.stdout)?
        .lines()
        .filter(|l| !l.is_empty())
        .collect();
    if missing.is_empty() {
        eprintln!("   nothing to copy");
        return Ok(());
    }
    eprintln!("   {} paths", missing.len());
    let mut export = Command::new("nix-store")
        .arg("--export")
        .args(&missing)
        .stdout(Stdio::piped())
        .spawn()?;
    let mut zstd = Command::new("zstd")
        .args(["-T0", "-q"])
        .stdin(export.stdout.take().unwrap())
        .stdout(Stdio::piped())
        .spawn()
        .context("zstd")?;
    let status = Command::new("ssh")
        .arg(host)
        .arg("zstd -d | sudo nix-store --import >/dev/null")
        .stdin(zstd.stdout.take().unwrap())
        .status()?;
    ensure!(export.wait()?.success(), "nix-store --export");
    ensure!(zstd.wait()?.success(), "zstd");
    ensure!(status.success(), "importing on {host}");
    Ok(())
}

fn print(signed: &release::Signed) {
    let p = &signed.payload;
    println!(
        "release {}  commit {}  signed by {}",
        p.counter,
        &p.rev[..12.min(p.rev.len())],
        identity::fingerprint(&signed.signer)
    );
    for (name, b) in &p.boxes {
        println!("  {name}  {}", b.path);
    }
}

fn fetch(url: &str) -> Result<Option<release::Signed>> {
    match ureq_get(url) {
        Ok(body) => Ok(Some(
            serde_json::from_str(&body).context("release file is not json")?,
        )),
        Err(NotFound) => Ok(None),
    }
}

struct NotFound;

fn ureq_get(url: &str) -> std::result::Result<String, NotFound> {
    // curl is on every machine this runs on; no need for another http stack
    let out = Command::new("curl")
        .args(["-sf", "-L", url])
        .output()
        .map_err(|_| NotFound)?;
    if !out.status.success() {
        return Err(NotFound);
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

fn load(keys: &auth::Store) -> Result<ed25519_dalek::SigningKey> {
    let s = keys
        .get(ACCOUNT)?
        .context("no release key here - `dd release init`")?;
    release::decode_secret(&s).map_err(|e| anyhow::anyhow!("release key: {e}"))
}

fn repo_root(repo: &str) -> Result<PathBuf> {
    Ok(PathBuf::from(
        git(Path::new(repo), &["rev-parse", "--show-toplevel"]).context("not in the repository")?,
    ))
}

fn git(dir: &Path, args: &[&str]) -> Result<String> {
    let out = Command::new("git").arg("-C").arg(dir).args(args).output()?;
    if !out.status.success() {
        bail!(
            "git {}: {}",
            args.join(" "),
            String::from_utf8_lossy(&out.stderr).trim()
        );
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_owned())
}

fn sh(bin: &str, args: &[&str]) -> Result<String> {
    let out = Command::new(bin)
        .args(args)
        .output()
        .with_context(|| format!("running {bin}"))?;
    if !out.status.success() {
        bail!(
            "{bin} {}: {}",
            args.join(" "),
            String::from_utf8_lossy(&out.stderr).trim()
        );
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}
