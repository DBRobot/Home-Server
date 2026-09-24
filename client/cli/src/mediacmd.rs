//! `dd media`: every openable library as folders on this machine, through
//! rclone, and a player on request (client/media does the work).

use std::path::PathBuf;

use anyhow::{Context, Result};

pub async fn run(
    keys: &auth::Store,
    dirs: &[String],
    at: Option<PathBuf>,
    jellyfin: Option<PathBuf>,
) -> Result<()> {
    let home = std::env::var("HOME").context("HOME")?;
    let at = at.unwrap_or_else(|| PathBuf::from(&home).join("Commonty"));
    let (opener, user, _) = media::gate::Opener::load(keys)?;
    // a token that outlives a film: the mount holds it for the day
    let kp = auth::device::load(keys)?.context("no device key here")?;
    let token = auth::device::mint(&kp, &user, std::time::Duration::from_secs(24 * 3600))?;
    let base = media::gate::files_base(dirs)?;
    let mut mounts = Vec::new();
    for (owner, lib, key) in media::gate::openable(dirs, &user, &opener).await? {
        let gate = media::gate::Gate::new(&base, &lib.id, &token, &key);
        let (dav, _) = gate.dav();
        let dir = at.join(&lib.id[..12]);
        let m = media::mount::mount(&lib.id, &owner, dav, &token, &key, &dir)?;
        eprintln!(
            "dd media: {} ({}) at {}",
            &lib.id[..12],
            if owner == user {
                "yours".to_string()
            } else {
                format!("{owner}'s")
            },
            dir.display()
        );
        mounts.push(m);
    }
    if mounts.is_empty() {
        anyhow::bail!("no library to mount - `dd library new` makes one");
    }
    println!("mounted under {}", at.display());
    let player = match jellyfin {
        Some(bin) => {
            let data = PathBuf::from(&home).join(".local/share/dd/jellyfin");
            let password = media::jellyfin::password(keys)?;
            let p = media::jellyfin::start(&bin, &data, &at, &user, &password).await?;
            println!(
                "jellyfin at {} with its own data in {}; sign in as {user} with the password in the keyring ({})",
                p.url,
                data.display(),
                media::jellyfin::PASSWORD
            );
            Some(p)
        }
        None => {
            println!(
                "point your player at it; `--jellyfin <path>` starts jellyfin on this machine"
            );
            None
        }
    };
    println!("press ctrl-c to unmount");
    tokio::signal::ctrl_c().await?;
    drop(player);
    drop(mounts);
    Ok(())
}
