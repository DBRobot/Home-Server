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

/// One game the catalogue knows: a pelican egg, resolved (games/resolve.py).
/// The guest does what the egg's daemon would: install with steamcmd, fill
/// the startup line from the settings, rewrite the files it names, watch
/// for the ready line.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Game {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub description: String,
    /// the dedicated server's steam app
    pub app: u64,
    /// the game's own steam app: what the cover is of
    #[serde(default)]
    pub game: Option<u64>,
    #[serde(default)]
    pub cover: bool,
    #[serde(default)]
    pub wine: bool,
    pub startup: String,
    #[serde(default)]
    pub ready: Option<String>,
    #[serde(default = "ctrl_c")]
    pub stop: String,
    #[serde(default)]
    pub files: serde_json::Value,
    pub install: String,
    #[serde(default)]
    pub settings: Vec<Setting>,
    /// variables that are ports: each gets one from the pool
    #[serde(default)]
    pub ports: Vec<String>,
    #[serde(default)]
    pub port_defaults: BTreeMap<String, String>,
}
fn ctrl_c() -> String {
    "^C".into()
}

impl Game {
    /// MiB: nothing in an egg says; wine servers and the big builders want more
    pub fn memory(&self) -> u64 {
        if self.wine { 8192 } else { 6144 }
    }
    pub fn cores(&self) -> u32 {
        4
    }
    /// what a person fills in: our few by our names, then the egg's own
    pub fn visible_settings(&self) -> impl Iterator<Item = &Setting> {
        self.settings.iter().filter(|s| s.editable)
    }
}

/// One thing a person may set, by the egg's variable and our label for it
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Setting {
    pub var: String,
    pub label: String,
    #[serde(default)]
    pub default: String,
    #[serde(default = "text")]
    pub kind: String,
    #[serde(default)]
    pub help: String,
    #[serde(default)]
    pub editable: bool,
    /// our short name for a common one: name, password, players, world
    #[serde(default)]
    pub ours: Option<String>,
    #[serde(default)]
    pub choices: Option<Vec<String>>,
}
fn text() -> String {
    "text".into()
}

#[derive(Debug, Clone, Deserialize)]
pub struct Catalogue {
    pub games: Vec<Game>,
}

/// a port from the pool, forwarded both ways: game ports are udp and tcp
/// alike more often than not, and the guest listens on the same number
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Port {
    /// the egg's variable it fills (SERVER_PORT, QUERY_PORT, ...)
    pub var: String,
    pub port: u16,
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
    pub ports: Vec<Port>,
    /// the egg's variables as this server has them: settings and ports
    pub env: BTreeMap<String, String>,
    /// the recipe, so the guest needs nothing but this file
    pub recipe: Recipe,
    /// "running" or "stopped": what a reboot restores
    pub desired: String,
    pub created: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Recipe {
    pub app: u64,
    pub wine: bool,
    pub startup: String,
    pub ready: Option<String>,
    pub stop: String,
    pub files: serde_json::Value,
    pub install: String,
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
    /// the box's cpus: a guest gets no more
    pub cores: u32,
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

    /// the game's own output, as the guest appends it beside the record
    pub fn log_tail(&self, id: &str, lines: usize) -> String {
        let Ok(text) = std::fs::read_to_string(self.instance_dir(id).join("game.log")) else {
            return String::new();
        };
        let all: Vec<&str> = text.lines().collect();
        let from = all.len().saturating_sub(lines);
        all[from..].join("\n")
    }

    pub fn state(&self, i: &Instance) -> State {
        let (active, failed) = self.units.state(&i.id);
        if failed {
            return State::Failed;
        }
        if !active {
            // meant to be up and not yet: between states (a restart, the
            // moment after a start), not stopped
            return if i.desired == "running" {
                State::Starting
            } else {
                State::Stopped
            };
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
    pub fn create(
        &self,
        owner: &str,
        game: &str,
        settings: &BTreeMap<String, String>,
    ) -> Result<Instance> {
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
        self.room_for(&all, spec.memory().min(self.cfg.memory_budget))?;

        let used: HashSet<u16> = all
            .iter()
            .flat_map(|i| i.ports.iter().map(|p| p.port))
            .collect();
        let mut free = (self.cfg.port_base..self.cfg.port_base.saturating_add(self.cfg.port_count))
            .filter(|p| !used.contains(p));
        let mut ports = Vec::new();
        for var in &spec.ports {
            ports.push(Port {
                var: var.clone(),
                port: free.next().context("this box has no game ports left")?,
            });
        }

        // the egg's variables: its defaults, then what the person set where
        // they may, then the ports
        let mut env: BTreeMap<String, String> = spec
            .settings
            .iter()
            .map(|s| (s.var.clone(), s.default.clone()))
            .collect();
        for s in spec.visible_settings() {
            if let Some(v) = settings.get(&s.var) {
                let v = v.trim();
                if !v.is_empty() {
                    if let Some(c) = &s.choices
                        && !c.iter().any(|x| x == v)
                    {
                        bail!("{}: not one of the choices", s.label);
                    }
                    if s.kind == "number" && v.parse::<f64>().is_err() {
                        bail!("{}: not a number", s.label);
                    }
                    env.insert(s.var.clone(), v.to_string());
                }
            }
        }
        for p in &ports {
            env.insert(p.var.clone(), p.port.to_string());
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

        // no more than the box has: the budget bounds memory, the cpu count
        // bounds cores (qemu warns past it, kvm crawls)
        let memory = spec.memory().min(self.cfg.memory_budget);
        let cores = spec.cores().min(self.cfg.cores.max(1));
        let i = Instance {
            id,
            game: game.to_string(),
            owner: owner.to_string(),
            memory,
            cores,
            ports,
            env,
            recipe: Recipe {
                app: spec.app,
                wine: spec.wine,
                startup: spec.startup.clone(),
                ready: spec.ready.clone(),
                stop: spec.stop.clone(),
                files: spec.files.clone(),
                install: spec.install.clone(),
            },
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

    /// The settings a person may set, changed; they take effect when the
    /// server next starts (the guest fills its files at start). A running
    /// server is restarted for them, world kept.
    pub fn configure(
        &self,
        owner: &str,
        id: &str,
        settings: &BTreeMap<String, String>,
    ) -> Result<()> {
        let mut i = self.owned(owner, id)?;
        let spec = self
            .cfg
            .catalogue
            .get(&i.game)
            .with_context(|| format!("no such game: {}", i.game))?;
        for s in spec.visible_settings() {
            if let Some(v) = settings.get(&s.var) {
                let v = v.trim();
                if v.is_empty() {
                    continue;
                }
                if let Some(c) = &s.choices
                    && !c.iter().any(|x| x == v)
                {
                    bail!("{}: not one of the choices", s.label);
                }
                if s.kind == "number" && v.parse::<f64>().is_err() {
                    bail!("{}: not a number", s.label);
                }
                i.env.insert(s.var.clone(), v.to_string());
            }
        }
        self.save(&i)?;
        if i.desired == "running" {
            self.units.stop(&i.id)?;
            self.units.start(&i.id)?;
        }
        Ok(())
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
                id: "valheim".into(),
                name: "Valheim".into(),
                description: String::new(),
                app: 896660,
                game: Some(892970),
                cover: true,
                wine: false,
                startup: "./valheim_server.x86_64 -name \"{{SERVER_NAME}}\" -port {{SERVER_PORT}} -password \"{{PASSWORD}}\"".into(),
                ready: Some("Game server connected".into()),
                stop: "^C".into(),
                files: serde_json::json!({}),
                install: "steamcmd".into(),
                settings: vec![
                    Setting {
                        var: "SERVER_NAME".into(),
                        label: "Server name".into(),
                        default: "My Server".into(),
                        kind: "text".into(),
                        help: String::new(),
                        editable: true,
                        ours: Some("name".into()),
                        choices: None,
                    },
                    Setting {
                        var: "PASSWORD".into(),
                        label: "Password".into(),
                        default: "secret".into(),
                        kind: "text".into(),
                        help: String::new(),
                        editable: true,
                        ours: Some("password".into()),
                        choices: None,
                    },
                    Setting {
                        var: "SRCDS_APPID".into(),
                        label: "App".into(),
                        default: "896660".into(),
                        kind: "text".into(),
                        help: String::new(),
                        editable: false,
                        ours: None,
                        choices: None,
                    },
                ],
                ports: vec!["SERVER_PORT".into(), "QUERY_PORT".into()],
                port_defaults: BTreeMap::new(),
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
                cores: 8,
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
        let mut set = BTreeMap::new();
        set.insert("SERVER_NAME".to_string(), "Tom's".to_string());
        set.insert("SRCDS_APPID".to_string(), "1".to_string());
        let i = m.create("tom", "valheim", &set).unwrap();
        assert_eq!(i.id, "valheim1");
        assert_eq!(i.ports[0].port, 27000);
        assert_eq!(i.ports[1].port, 27001);
        assert_eq!(i.recipe.app, 896660);
        assert_eq!(i.env["SERVER_NAME"], "Tom's");
        assert_eq!(i.env["PASSWORD"], "secret", "the egg's default");
        assert_eq!(i.env["SRCDS_APPID"], "896660", "not a person's to set");
        assert_eq!(i.env["SERVER_PORT"], i.ports[0].port.to_string());
        assert!(fake.up.lock().unwrap().contains("valheim1"));
        // the record is what the runner and the guest read
        assert_eq!(m.instance("valheim1").unwrap(), i);

        // one at a time, and not someone else's
        assert!(m.create("tom", "valheim", &BTreeMap::new()).is_err());
        assert!(m.stop("eve", "valheim1").is_err());
        assert!(m.delete("tom", "valheim1").is_err(), "running: stop first");

        m.stop("tom", "valheim1").unwrap();
        assert_eq!(m.instance("valheim1").unwrap().desired, "stopped");
        assert_eq!(m.state(&m.instance("valheim1").unwrap()), State::Stopped);
        m.delete("tom", "valheim1").unwrap();
        assert!(m.instance("valheim1").is_none());
        assert!(m.create("tom", "nope", &BTreeMap::new()).is_err());
    }

    #[test]
    fn a_reboot_brings_back_what_should_be_up_and_only_that() {
        let (m, fake) = manager(2, 16384);
        let a = m.create("tom", "valheim", &BTreeMap::new()).unwrap();
        let b = m.create("tom", "valheim", &BTreeMap::new()).unwrap();
        m.stop("tom", &b.id).unwrap();
        // the box goes down: nothing is up, the records remain
        fake.up.lock().unwrap().clear();
        assert_eq!(m.restore(), 1);
        assert!(fake.up.lock().unwrap().contains(&a.id));
        assert!(!fake.up.lock().unwrap().contains(&b.id));
        assert_eq!(m.restore(), 0, "already up");
    }

    #[test]
    fn settings_change_and_a_running_server_restarts_for_them() {
        let (m, fake) = manager(1, 16384);
        let i = m.create("tom", "valheim", &BTreeMap::new()).unwrap();
        let mut set = BTreeMap::new();
        set.insert("SERVER_NAME".to_string(), "Renamed".to_string());
        set.insert("SRCDS_APPID".to_string(), "1".to_string());
        m.configure("tom", &i.id, &set).unwrap();
        let i = m.instance(&i.id).unwrap();
        assert_eq!(i.env["SERVER_NAME"], "Renamed");
        assert_eq!(i.env["SRCDS_APPID"], "896660", "not a person's to set");
        assert!(fake.up.lock().unwrap().contains(&i.id), "back up after");
        assert!(m.configure("ann", &i.id, &set).is_err(), "not hers");
    }

    #[test]
    fn the_box_has_so_much_memory_and_so_many_ports() {
        let (m, _) = manager(5, 12288);
        m.create("a", "valheim", &BTreeMap::new()).unwrap();
        m.create("b", "valheim", &BTreeMap::new()).unwrap();
        let e = m
            .create("c", "valheim", &BTreeMap::new())
            .unwrap_err()
            .to_string();
        assert!(e.contains("full"), "{e}");
        let (m, _) = manager(5, 1 << 20);
        m.create("a", "valheim", &BTreeMap::new()).unwrap();
        m.create("b", "valheim", &BTreeMap::new()).unwrap();
        let e = m
            .create("c", "valheim", &BTreeMap::new())
            .unwrap_err()
            .to_string();
        assert!(e.contains("ports"), "{e}");
    }
}
