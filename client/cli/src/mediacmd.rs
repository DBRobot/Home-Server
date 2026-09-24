//! `dd media`: every openable library as folders on this machine, and a
//! player on request (client/media does the mounting).

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
    let mount = media::fs::mount(keys, dirs, at).await?;
    let (_, user, _) = media::gate::Opener::load(keys)?;
    for l in &mount.libraries {
        eprintln!(
            "dd media: {} ({}): {} file(s)",
            &l.id[..12],
            if l.owner == user {
                "yours".to_string()
            } else {
                format!("{}'s", l.owner)
            },
            l.files
        );
    }
    println!("mounted at {}", mount.at.display());
    let player = match jellyfin {
        Some(bin) => {
            let data = PathBuf::from(&home).join(".local/share/dd/jellyfin");
            let password = media::jellyfin::password(keys)?;
            let p = media::jellyfin::start(&bin, &data, &mount.at, &user, &password).await?;
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
    drop(mount);
    drop(player);
    Ok(())
}
