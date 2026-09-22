//! The Games pages: what each template gets. The HTML is in templates/,
//! the look in web/games.css, the little script in web/games.js.

use askama::Template;

use crate::{Game, Instance, Manager, Setting, State};

pub fn state_name(st: State) -> &'static str {
    match st {
        State::Stopped => "stopped",
        State::Starting => "starting",
        State::Updating => "updating",
        State::Running => "running",
        State::Failed => "failed",
    }
}

/// a server as the pages show it
pub struct ServerView<'a> {
    pub id: &'a str,
    pub name: &'a str,
    pub game: Option<&'a Game>,
    pub state: &'static str,
    pub label: &'static str,
    pub who: String,
    pub address: Option<String>,
}

impl<'a> ServerView<'a> {
    fn new(m: &'a Manager, i: &'a Instance, user: &str) -> Self {
        let st = m.state(i);
        let game = m.cfg.catalogue.get(&i.game);
        let port = i
            .ports
            .iter()
            .find(|p| p.var == "SERVER_PORT")
            .or(i.ports.first());
        Self {
            id: &i.id,
            name: game.map(|g| g.name.as_str()).unwrap_or(&i.game),
            game,
            state: state_name(st),
            label: st.label(),
            who: if i.owner == user {
                "yours".into()
            } else {
                format!("{}'s", i.owner)
            },
            address: port.map(|p| format!("{}:{}", m.cfg.address, p.port)),
        }
    }
}

/// a game opened over the library
pub struct OpenView<'a> {
    pub game: &'a Game,
    pub back: String,
    pub ours: Vec<&'a Setting>,
    pub more: Vec<&'a Setting>,
    pub memory_gb: u64,
    pub cores: u32,
}

#[derive(Template)]
#[template(path = "library.html")]
pub struct Library<'a> {
    pub user: &'a str,
    /// the demo may look, not start
    pub demo: bool,
    pub home: &'a str,
    pub q: &'a str,
    pub count: usize,
    pub notice: Option<&'a str>,
    pub mine: Vec<ServerView<'a>>,
    pub others: Vec<ServerView<'a>>,
    pub games: Vec<&'a Game>,
    pub open: Option<OpenView<'a>>,
}

/// The library, with one game open over it when `open` says so.
pub fn library(
    m: &Manager,
    user: &str,
    q: &str,
    open: Option<&str>,
    notice: Option<&str>,
) -> String {
    let all = m.instances();
    let ql = q.trim().to_lowercase();
    let (mine, others): (Vec<&Instance>, Vec<&Instance>) =
        all.iter().partition(|i| i.owner == user);
    let open = open.and_then(|id| m.cfg.catalogue.get(id)).map(|g| {
        let (ours, more) = g.visible_settings().partition(|s| s.ours.is_some());
        OpenView {
            game: g,
            back: if q.is_empty() {
                "/".to_string()
            } else {
                format!("/?q={q}")
            },
            ours,
            more,
            memory_gb: g.memory() / 1024,
            cores: g.cores(),
        }
    });
    Library {
        user,
        demo: user == "demo",
        home: &m.cfg.home,
        q,
        count: m.cfg.catalogue.len(),
        notice,
        mine: mine.iter().map(|i| ServerView::new(m, i, user)).collect(),
        others: others.iter().map(|i| ServerView::new(m, i, user)).collect(),
        games: m
            .cfg
            .catalogue
            .values()
            .filter(|g| ql.is_empty() || g.name.to_lowercase().contains(&ql))
            .collect(),
        open,
    }
    .render()
    .unwrap_or_default()
}

pub struct PortView {
    pub port: u16,
    pub label: String,
}

#[derive(Template)]
#[template(path = "server.html")]
pub struct Server<'a> {
    pub user: &'a str,
    pub demo: bool,
    /// the settings a person may change, with this server's values
    pub fields: Vec<Setting>,
    pub home: &'a str,
    pub s: ServerView<'a>,
    pub address: &'a str,
    /// the port a player types: SERVER_PORT, else the first
    pub main_port: Option<PortView>,
    pub other_ports: Vec<PortView>,
    pub settings: String,
    pub mine: bool,
    pub idle: bool,
    pub busy: bool,
    pub log: String,
}

/// One server: what it is, where it is, what it says, and its controls.
pub fn server(m: &Manager, user: &str, i: &Instance) -> String {
    let st = m.state(i);
    let g = m.cfg.catalogue.get(&i.game);
    let settings = g
        .map(|g| {
            g.visible_settings()
                .filter(|s| s.ours.is_some())
                .filter_map(|s| i.env.get(&s.var).map(|v| format!("{}: {v}", s.label)))
                .collect::<Vec<_>>()
                .join(" · ")
        })
        .unwrap_or_default();
    let fields = g
        .map(|g| {
            g.visible_settings()
                .map(|s| {
                    let mut f = s.clone();
                    if let Some(v) = i.env.get(&s.var) {
                        f.default = v.clone();
                    }
                    f
                })
                .collect()
        })
        .unwrap_or_default();
    Server {
        user,
        demo: user == "demo",
        fields,
        home: &m.cfg.home,
        s: ServerView::new(m, i, user),
        address: &m.cfg.address,
        main_port: i
            .ports
            .iter()
            .find(|p| p.var == "SERVER_PORT")
            .or(i.ports.first())
            .map(|p| PortView {
                port: p.port,
                label: String::new(),
            }),
        other_ports: {
            let main = i
                .ports
                .iter()
                .find(|p| p.var == "SERVER_PORT")
                .or(i.ports.first())
                .map(|p| p.var.clone());
            i.ports
                .iter()
                .filter(|p| Some(&p.var) != main.as_ref())
                .map(|p| PortView {
                    port: p.port,
                    label: p.var.replace("_PORT", "").replace('_', " ").to_lowercase(),
                })
                .collect()
        },
        settings,
        mine: i.owner == user,
        idle: matches!(st, State::Stopped | State::Failed),
        busy: matches!(st, State::Starting | State::Updating),
        log: m.log_tail(&i.id, 80),
    }
    .render()
    .unwrap_or_default()
}
