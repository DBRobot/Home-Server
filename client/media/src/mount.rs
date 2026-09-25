//! The libraries as folders on a desktop: rclone does it. Each library is
//! an rclone crypt remote over the gate's WebDAV, with the device token as
//! bearer and the library key as password; `rclone mount` puts it under
//! a folder of its own. Nothing of ours is in the path of a byte.

use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};

use anyhow::{Context, Result, bail};

pub struct Mounted {
    pub id: String,
    pub owner: String,
    pub at: PathBuf,
    child: Child,
}

impl Drop for Mounted {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = Command::new("fusermount3")
            .arg("-uz")
            .arg(&self.at)
            .stderr(Stdio::null())
            .status();
    }
}

/// the rclone binary: named by the packaging, or on PATH
pub fn rclone() -> PathBuf {
    std::env::var("DD_RCLONE")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("rclone"))
}

/// what rclone stores in place of a password: not a secret, a shape
pub fn obscure(rclone: &Path, plain: &str) -> Result<String> {
    let o = Command::new(rclone)
        .args(["obscure", plain])
        .output()
        .with_context(|| format!("running {}", rclone.display()))?;
    if !o.status.success() {
        bail!("rclone obscure: {}", String::from_utf8_lossy(&o.stderr));
    }
    Ok(String::from_utf8(o.stdout)?.trim().to_string())
}

/// mount one library at `at`, as rclone crypt over the gate's WebDAV
pub fn mount(
    id: &str,
    owner: &str,
    dav_url: &str,
    token: &str,
    key: &library::Key,
    at: &Path,
) -> Result<Mounted> {
    let rc = rclone();
    // a mount left by a process that died is a folder nothing can open
    if std::fs::read_dir(at).is_err() {
        let _ = Command::new("fusermount3")
            .arg("-uz")
            .arg(at)
            .stderr(Stdio::null())
            .status();
    }
    std::fs::create_dir_all(at)?;
    let password = obscure(&rc, &library::crypt::password_of(key))?;
    let salt = obscure(&rc, id)?;
    let child = Command::new(&rc)
        .args([
            "mount",
            "lib:",
            at.to_str().context("mount path")?,
            "--vfs-cache-mode",
            "writes",
            "--dir-cache-time",
            "10s",
            "--poll-interval",
            "0",
            "--quiet",
        ])
        // the two remotes, in the environment: nothing written to disk
        .env("RCLONE_CONFIG_DAV_TYPE", "webdav")
        .env("RCLONE_CONFIG_DAV_URL", dav_url)
        .env("RCLONE_CONFIG_DAV_VENDOR", "other")
        .env("RCLONE_CONFIG_DAV_BEARER_TOKEN", token)
        .env("RCLONE_CONFIG_LIB_TYPE", "crypt")
        .env("RCLONE_CONFIG_LIB_REMOTE", "dav:")
        .env("RCLONE_CONFIG_LIB_PASSWORD", password)
        .env("RCLONE_CONFIG_LIB_PASSWORD2", salt)
        .env("RCLONE_CONFIG", "/dev/null")
        .stdin(Stdio::null())
        .spawn()
        .with_context(|| format!("starting {}", rc.display()))?;
    Ok(Mounted {
        id: id.to_string(),
        owner: owner.to_string(),
        at: at.to_path_buf(),
        child,
    })
}
