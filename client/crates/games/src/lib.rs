//! Game servers a member starts for themselves.
//!
//! The release carries a catalogue of games and one guest machine
//! (modules/games.nix). That a particular server exists is not in any
//! release: it is a record in this box's state, made here at a member's
//! request, and the unit that runs it is an instance of a template
//! (`dd-game@<id>`). This service makes and removes those records, starts
//! and stops those units, and on its own start brings back every server
//! whose record says it should be up - so a reboot does not end a world, and
//! a deploy, which never touches the template's instances, does not either.
//!
//! Who is asking comes from the gate: nginx sets `x-dd-user` from the
//! verifier's answer. What the demo may do here (look) is the gate's rule,
//! not this service's.

pub mod pages;

use std::collections::{BTreeMap, HashSet};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize)]
pub struct Game {
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(rename = "appId")]
    pub app_id: Option<u64>,
    pub exec: String,
    #[serde(default)]
    pub args: Vec<String>,
    pub ports: Vec<GamePort>,
    #[serde(default)]
    pub saves: Vec<String>,
    /// MiB
    pub memory: u64,
    #[serde(default = "two")]
    pub cores: u32,
}
fn two() -> u32 {
    2
}

#[derive(Debug, Clone, Deserialize)]
pub struct GamePort {
    pub proto: String,
    pub guest: u16,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Port {
    pub proto: String,
    pub host: u16,
    pub guest: u16,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Steam {
    #[serde(rename = "appId")]
    pub app_id: u64,
}

/// One server: what the guest needs to be it (the runner and the guest read
/// this same file), whose it is, and whether it is meant to be up.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Instance {
    pub id: String,
    pub game: String,
    pub owner: String,
    pub memory: u64,
    pub cores: u32,
    pub exec: String,
    pub args: Vec<String>,
    pub ports: Vec<Port>,
    pub saves: Vec<String>,
    pub steam: Option<Steam>,
    /// "running" or "stopped": what a reboot restores
    pub desired: String,
    pub created: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    Stopped,
    Starting,
    Updating,
    Running,
    Failed,
}

impl State {
    pub fn label(self) -> &'static str {
        match self {
            State::Stopped => "stopped",
            State::Starting => "starting",
            State::Updating => "downloading the game",
            State::Running => "running",
            State::Failed => "failed",
        }
    }
}

/// systemd, as far as this service needs it; the tests have their own.
pub trait Units: Send + Sync {
    fn start(&self, id: &str) -> Result<()>;
    fn stop(&self, id: &str) -> Result<()>;
    /// (active, failed)
    fn state(&self, id: &str) -> (bool, bool);
}

pub struct Systemd;

impl Systemd {
    fn ctl(args: &[&str]) -> Result<std::process::Output> {
        std::process::Command::new("systemctl")
            .args(args)
            .output()
            .context("systemctl")
    }
}

impl Units for Systemd {
    fn start(&self, id: &str) -> Result<()> {
        // --no-block: the guest takes a while; the page shows its progress
        let o = Self::ctl(&["start", "--no-block", &format!("dd-game@{id}.service")])?;
        anyhow::ensure!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
        Ok(())
    }
    fn stop(&self, id: &str) -> Result<()> {
        let o = Self::ctl(&["stop", "--no-block", &format!("dd-game@{id}.service")])?;
        anyhow::ensure!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
        Ok(())
    }
    fn state(&self, id: &str) -> (bool, bool) {
        let unit = format!("dd-game@{id}.service");
        let is = |what: &str| {
            Self::ctl(&[what, "--quiet", &unit])
                .map(|o| o.status.success())
                .unwrap_or(false)
        };
        (is("is-active"), is("is-failed"))
    }
}

pub struct Config {
    pub dir: PathBuf,
    pub catalogue: BTreeMap<String, Game>,
    /// first host port handed out; each instance port gets the next free one
    pub port_base: u16,
    pub port_count: u16,
    /// servers one member may have up at once
    pub per_member: usize,
    /// MiB every running guest together may have
    pub memory_budget: u64,
    /// the address players type, shown with the port
    pub address: String,
    pub home: String,
}

pub struct Manager {
    pub cfg: Config,
    units: Box<dyn Units>,
    // creation allocates ports and an id: one at a time
    lock: Mutex<()>,
}

pub fn valid_id(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 24
        && s.chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
}

fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

impl Manager {
    pub fn new(cfg: Config, units: Box<dyn Units>) -> Result<Arc<Self>> {
        std::fs::create_dir_all(cfg.dir.join("instances"))?;
        Ok(Arc::new(Self {
            cfg,
            units,
            lock: Mutex::new(()),
        }))
    }

    fn instance_dir(&self, id: &str) -> PathBuf {
        self.cfg.dir.join("instances").join(id)
    }

    pub fn instances(&self) -> Vec<Instance> {
        let mut out = Vec::new();
        let Ok(rd) = std::fs::read_dir(self.cfg.dir.join("instances")) else {
            return out;
        };
        for e in rd.flatten() {
            let p = e.path().join("instance.json");
            if let Ok(b) = std::fs::read(&p)
                && let Ok(i) = serde_json::from_slice::<Instance>(&b)
            {
                out.push(i);
            }
        }
        out.sort_by_key(|i| i.created);
        out
    }

    pub fn instance(&self, id: &str) -> Option<Instance> {
        if !valid_id(id) {
            return None;
        }
        serde_json::from_slice(&std::fs::read(self.instance_dir(id).join("instance.json")).ok()?)
            .ok()
    }

    fn save(&self, i: &Instance) -> Result<()> {
        let d = self.instance_dir(&i.id);
        std::fs::create_dir_all(&d)?;
        let tmp = d.join(".instance.json.tmp");
        std::fs::write(&tmp, serde_json::to_vec_pretty(i)?)?;
        std::fs::rename(tmp, d.join("instance.json"))?;
        Ok(())
    }

    pub fn state(&self, i: &Instance) -> State {
        let (active, failed) = self.units.state(&i.id);
        if failed {
            return State::Failed;
        }
        if !active {
            return State::Stopped;
        }
        match std::fs::read_to_string(self.instance_dir(&i.id).join("status"))
            .unwrap_or_default()
            .trim()
        {
            "running" => State::Running,
            "updating" => State::Updating,
            _ => State::Starting,
        }
    }

    fn up(&self, i: &Instance) -> bool {
        self.units.state(&i.id).0
    }

    /// A new server of `game` for `owner`, started. Refused past the
    /// member's share or the box's memory.
    pub fn create(&self, owner: &str, game: &str) -> Result<Instance> {
        let _g = self.lock.lock().unwrap();
        let spec = self
            .cfg
            .catalogue
            .get(game)
            .with_context(|| format!("no such game: {game}"))?;
        let all = self.instances();
        let mine_up = all
            .iter()
            .filter(|i| i.owner == owner && i.desired == "running")
            .count();
        if mine_up >= self.cfg.per_member {
            bail!(
                "you have {mine_up} server{} up already; stop one first",
                if mine_up == 1 { "" } else { "s" }
            );
        }
        self.room_for(&all, spec.memory)?;

        let used: HashSet<u16> = all
            .iter()
            .flat_map(|i| i.ports.iter().map(|p| p.host))
            .collect();
        let mut free = (self.cfg.port_base..self.cfg.port_base.saturating_add(self.cfg.port_count))
            .filter(|p| !used.contains(p));
        let mut ports = Vec::new();
        for p in &spec.ports {
            ports.push(Port {
                proto: p.proto.clone(),
                host: free.next().context("this box has no game ports left")?,
                guest: p.guest,
            });
        }

        let taken: HashSet<String> = all.iter().map(|i| i.id.clone()).collect();
        let id = (1..)
            .map(|n| format!("{game}{n}"))
            .map(|s| {
                s.chars()
                    .filter(|c| c.is_ascii_alphanumeric())
                    .collect::<String>()
                    .to_lowercase()
            })
            .find(|s| !taken.contains(s) && valid_id(s))
            .context("no id")?;

        let i = Instance {
            id,
            game: game.to_string(),
            owner: owner.to_string(),
            memory: spec.memory,
            cores: spec.cores,
            exec: spec.exec.clone(),
            args: spec.args.clone(),
            ports,
            saves: spec.saves.clone(),
            steam: spec.app_id.map(|app_id| Steam { app_id }),
            desired: "running".into(),
            created: now(),
        };
        self.save(&i)?;
        self.units.start(&i.id)?;
        Ok(i)
    }

    fn room_for(&self, all: &[Instance], more: u64) -> Result<()> {
        let up: u64 = all
            .iter()
            .filter(|i| i.desired == "running")
            .map(|i| i.memory)
            .sum();
        if up + more > self.cfg.memory_budget {
            bail!("this box is full of games right now; try again when one stops");
        }
        Ok(())
    }

    fn owned(&self, owner: &str, id: &str) -> Result<Instance> {
        let i = self.instance(id).context("no such server")?;
        anyhow::ensure!(i.owner == owner, "that server is not yours");
        Ok(i)
    }

    pub fn start(&self, owner: &str, id: &str) -> Result<()> {
        let _g = self.lock.lock().unwrap();
        let mut i = self.owned(owner, id)?;
        if i.desired != "running" {
            let all = self.instances();
            let mine_up = all
                .iter()
                .filter(|x| x.owner == owner && x.desired == "running")
                .count();
            anyhow::ensure!(
                mine_up < self.cfg.per_member,
                "you have a server up already; stop it first"
            );
            self.room_for(&all, i.memory)?;
        }
        i.desired = "running".into();
        self.save(&i)?;
        self.units.start(&i.id)
    }

    pub fn stop(&self, owner: &str, id: &str) -> Result<()> {
        let mut i = self.owned(owner, id)?;
        i.desired = "stopped".into();
        self.save(&i)?;
        self.units.stop(&i.id)
    }

    /// The server and its world, gone. Stopped first.
    pub fn delete(&self, owner: &str, id: &str) -> Result<()> {
        let i = self.owned(owner, id)?;
        anyhow::ensure!(
            !self.up(&i),
            "stop it first; deleting removes the world with it"
        );
        std::fs::remove_dir_all(self.instance_dir(&i.id))?;
        Ok(())
    }

    /// After a reboot, or this service's own restart: every server whose
    /// record says it should be up, is.
    pub fn restore(&self) -> usize {
        let mut n = 0;
        for i in self.instances() {
            if i.desired == "running" && !self.up(&i) {
                match self.units.start(&i.id) {
                    Ok(()) => n += 1,
                    Err(e) => eprintln!("games: could not bring {} back: {e:#}", i.id),
                }
            }
        }
        n
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[derive(Default)]
    struct Fake {
        up: Mutex<HashSet<String>>,
    }
    impl Units for Arc<Fake> {
        fn start(&self, id: &str) -> Result<()> {
            self.up.lock().unwrap().insert(id.into());
            Ok(())
        }
        fn stop(&self, id: &str) -> Result<()> {
            self.up.lock().unwrap().remove(id);
            Ok(())
        }
        fn state(&self, id: &str) -> (bool, bool) {
            (self.up.lock().unwrap().contains(id), false)
        }
    }

    static N: AtomicUsize = AtomicUsize::new(0);
    fn manager(per_member: usize, budget: u64) -> (Arc<Manager>, Arc<Fake>) {
        let dir = std::env::temp_dir().join(format!(
            "dd-games-{}-{}",
            std::process::id(),
            N.fetch_add(1, Ordering::Relaxed)
        ));
        let mut catalogue = BTreeMap::new();
        catalogue.insert(
            "valheim".to_string(),
            Game {
                name: "Valheim".into(),
                description: String::new(),
                app_id: Some(896660),
                exec: "valheim_server.x86_64".into(),
                args: vec!["-port".into(), "2456".into()],
                ports: vec![
                    GamePort {
                        proto: "udp".into(),
                        guest: 2456,
                    },
                    GamePort {
                        proto: "udp".into(),
                        guest: 2457,
                    },
                ],
                saves: vec![".config/unity3d/IronGate/Valheim".into()],
                memory: 4096,
                cores: 2,
            },
        );
        let fake = Arc::new(Fake::default());
        let m = Manager::new(
            Config {
                dir,
                catalogue,
                port_base: 27000,
                port_count: 4,
                per_member,
                memory_budget: budget,
                address: "box.example".into(),
                home: "https://home.example/".into(),
            },
            Box::new(fake.clone()),
        )
        .unwrap();
        (m, fake)
    }

    #[test]
    fn a_member_starts_stops_and_deletes_their_own_server() {
        let (m, fake) = manager(1, 16384);
        let i = m.create("tom", "valheim").unwrap();
        assert_eq!(i.id, "valheim1");
        assert_eq!(i.ports[0].host, 27000);
        assert_eq!(i.ports[1].host, 27001);
        assert_eq!(i.steam, Some(Steam { app_id: 896660 }));
        assert!(fake.up.lock().unwrap().contains("valheim1"));
        // the record is what the runner and the guest read
        assert_eq!(m.instance("valheim1").unwrap(), i);

        // one at a time, and not someone else's
        assert!(m.create("tom", "valheim").is_err());
        assert!(m.stop("eve", "valheim1").is_err());
        assert!(m.delete("tom", "valheim1").is_err(), "running: stop first");

        m.stop("tom", "valheim1").unwrap();
        assert_eq!(m.instance("valheim1").unwrap().desired, "stopped");
        assert_eq!(m.state(&m.instance("valheim1").unwrap()), State::Stopped);
        m.delete("tom", "valheim1").unwrap();
        assert!(m.instance("valheim1").is_none());
        assert!(m.create("tom", "nope").is_err());
    }

    #[test]
    fn a_reboot_brings_back_what_should_be_up_and_only_that() {
        let (m, fake) = manager(2, 16384);
        let a = m.create("tom", "valheim").unwrap();
        let b = m.create("tom", "valheim").unwrap();
        m.stop("tom", &b.id).unwrap();
        // the box goes down: nothing is up, the records remain
        fake.up.lock().unwrap().clear();
        assert_eq!(m.restore(), 1);
        assert!(fake.up.lock().unwrap().contains(&a.id));
        assert!(!fake.up.lock().unwrap().contains(&b.id));
        assert_eq!(m.restore(), 0, "already up");
    }

    #[test]
    fn the_box_has_so_much_memory_and_so_many_ports() {
        let (m, _) = manager(5, 8192);
        m.create("a", "valheim").unwrap();
        m.create("b", "valheim").unwrap();
        let e = m.create("c", "valheim").unwrap_err().to_string();
        assert!(e.contains("full"), "{e}");
        let (m, _) = manager(5, 1 << 20);
        m.create("a", "valheim").unwrap();
        m.create("b", "valheim").unwrap();
        let e = m.create("c", "valheim").unwrap_err().to_string();
        assert!(e.contains("ports"), "{e}");
    }
}
