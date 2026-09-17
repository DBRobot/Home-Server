//! The member list: who may use the services. `fleet/members.json` in the
//! repo, a sorted list of member ids (a hash of each person's root), so it
//! carries no names. It reaches the boxes in the signed release; a box
//! checks it and cannot add to it.

use anyhow::{Context, Result, bail};
use std::path::{Path, PathBuf};

fn path(repo: &str) -> PathBuf {
    Path::new(repo).join("fleet/members.json")
}

pub fn read(repo: &str) -> Result<Vec<String>> {
    let p = path(repo);
    if !p.exists() {
        return Ok(vec![]);
    }
    serde_json::from_slice(&std::fs::read(&p).with_context(|| format!("reading {}", p.display()))?)
        .with_context(|| format!("parsing {}", p.display()))
}

fn write(repo: &str, mut ids: Vec<String>) -> Result<()> {
    ids.sort();
    ids.dedup();
    let p = path(repo);
    std::fs::write(&p, format!("{}\n", serde_json::to_string_pretty(&ids)?))
        .with_context(|| format!("writing {}", p.display()))
}

fn is_id(s: &str) -> bool {
    s.len() == 64 && s.chars().all(|c| c.is_ascii_hexdigit())
}

/// A member id, or a name looked up in the directories.
pub async fn resolve(who: &str, dirs: &[String]) -> Result<String> {
    if is_id(who) {
        return Ok(who.to_string());
    }
    let found = crate::who::fetch(dirs, who).await;
    match crate::who::newest(&found) {
        Some(e) => Ok(identity::member_id(&e.entry.root)),
        None => bail!("no entry for {who} in any directory"),
    }
}

pub async fn add(repo: &str, who: &str, dirs: &[String]) -> Result<()> {
    let id = resolve(who, dirs).await?;
    let mut ids = read(repo)?;
    if ids.contains(&id) {
        println!("already a member: {id}");
        return Ok(());
    }
    ids.push(id.clone());
    write(repo, ids)?;
    println!("added {id}\ncommit fleet/members.json and release; every box then lets them in");
    Ok(())
}

pub async fn remove(repo: &str, who: &str, dirs: &[String]) -> Result<()> {
    let id = resolve(who, dirs).await?;
    let mut ids = read(repo)?;
    let before = ids.len();
    ids.retain(|x| *x != id);
    if ids.len() == before {
        bail!("not a member: {id}");
    }
    write(repo, ids)?;
    println!("removed {id}\ncommit fleet/members.json and release; every box then shuts them out");
    Ok(())
}

/// Every id, with the name behind it where a directory lists an entry with
/// that root. Names are looked up, never stored.
pub async fn list(repo: &str, dirs: &[String]) -> Result<()> {
    let ids = read(repo)?;
    let mut names = std::collections::HashMap::new();
    if let Some(d) = dirs.first()
        && let Ok(h) = crate::who::http()
        && let Ok(r) = h.get(d.trim_end_matches('/')).send().await
        && let Ok(listed) = r.json::<Vec<serde_json::Value>>().await
    {
        for l in listed {
            if let Some(n) = l["name"].as_str()
                && let Some(e) = crate::who::newest(&crate::who::fetch(&dirs[..1], n).await)
            {
                names.insert(identity::member_id(&e.entry.root), n.to_string());
            }
        }
    }
    for id in ids {
        println!(
            "{id}  {}",
            names.get(&id).map(String::as_str).unwrap_or("-")
        );
    }
    Ok(())
}
