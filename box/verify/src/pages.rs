//! The verifier's pages: what each template gets. The HTML is in
//! templates/, the look in web/home.css, the scripts in web/*.js, served
//! from /_dd/static/.

use askama::Template;

/// One tile on the home page. The roles a box runs declare these.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
pub struct Service {
    pub name: String,
    pub url: String,
    /// photos, videos, files, chat, code, metrics, games, or anything else for a plain mark
    pub icon: String,
    /// a css colour for the tile's icon
    pub color: String,
    /// What the demo account may do here, as the gate enforces it: `full`
    /// (the service's own permissions are the limit), `read` (no writing
    /// method), `rate:N` (reads free, N other requests an hour). None:
    /// nothing, and the tile is greyed on its home page.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub demo: Option<String>,
    /// Where the demo goes instead. A member's Movies & TV is their own
    /// library; the demo has none, and gets the box's own films.
    #[serde(rename = "demoUrl", default, skip_serializing_if = "Option::is_none")]
    pub demo_url: Option<String>,
}

/// The account that needs no invite and no key: a look at what a member
/// sees, with nothing of their own and nothing kept.
pub const DEMO_USER: &str = "demo";

/// the stylesheet and the scripts, by the name under /_dd/static/
pub fn static_file(name: &str) -> Option<(&'static str, &'static str)> {
    let js = "text/javascript; charset=utf-8";
    Some(match name {
        "home.css" => (include_str!("../web/home.css"), "text/css; charset=utf-8"),
        "webauthn.js" => (include_str!("../web/webauthn.js"), js),
        "invite.js" => (include_str!("../web/invite.js"), js),
        "login.js" => (include_str!("../web/login.js"), js),
        "join.js" => (include_str!("../web/join.js"), js),
        "enrol.js" => (include_str!("../web/enrol.js"), js),
        "redeem.js" => (include_str!("../web/redeem.js"), js),
        "photos.js" => (include_str!("../web/photos.js"), js),
        "files.js" => (include_str!("../web/files.js"), js),
        "media.js" => (include_str!("../web/media.js"), js),
        "library.js" => (include_str!("../web/library.js"), js),
        "boxes.js" => (include_str!("../web/boxes.js"), js),
        "backups.js" => (include_str!("../web/backups.js"), js),
        "devices.js" => (include_str!("../web/devices.js"), js),
        "network.js" => (include_str!("../web/network.js"), js),
        "panel.js" => (include_str!("../web/panel.js"), js),
        _ => return None,
    })
}

/// the mark on a tile: an svg fragment from web/icons/, by the role's name
fn icon(key: &str) -> &'static str {
    match key {
        "photos" => include_str!("../web/icons/photos.svg"),
        "videos" => include_str!("../web/icons/videos.svg"),
        "files" => include_str!("../web/icons/files.svg"),
        "chat" => include_str!("../web/icons/chat.svg"),
        "code" => include_str!("../web/icons/code.svg"),
        "metrics" => include_str!("../web/icons/metrics.svg"),
        "games" => include_str!("../web/icons/games.svg"),
        _ => include_str!("../web/icons/plain.svg"),
    }
}

fn initial(user: &str) -> String {
    user.chars()
        .next()
        .map(|c| c.to_string())
        .unwrap_or_default()
}

/// One line in the bar's menu.
pub struct MenuItem {
    pub label: &'static str,
    pub url: String,
}

/// Everything in the bar that is not a service: the member's own pages,
/// the fleet's, and the way out. Groups are drawn with a rule between.
pub struct Menu {
    pub groups: Vec<Vec<MenuItem>>,
}

impl Menu {
    /// What this person may actually open. The demo has no library, no
    /// devices and no backups, so it is offered none of them.
    fn of(user: &str, services: &[Service]) -> Menu {
        let item = |label, url: &str| MenuItem {
            label,
            url: url.to_string(),
        };
        let metrics = services
            .iter()
            .find(|s| s.icon == "metrics")
            .map(|s| item("Metrics", &s.url));
        let mut groups = Vec::new();
        if user != DEMO_USER {
            groups.push(vec![
                item("Files", "/_dd/files"),
                item("Movies & TV", "/_dd/media"),
            ]);
            let mut fleet = vec![
                item("Backups", "/_dd/backups"),
                item("Devices", "/_dd/devices"),
                item("Network", "/_dd/network"),
                item("Boxes", "/_dd/boxes"),
            ];
            fleet.extend(metrics);
            groups.push(fleet);
        } else if let Some(m) = metrics {
            groups.push(vec![m]);
        }
        groups.push(vec![item("Sign out", "/_dd/logout")]);
        Menu { groups }
    }
}

#[derive(Template)]
#[template(path = "login.html")]
struct Login;

#[derive(Template)]
#[template(path = "enrol.html")]
struct Enrol;

#[derive(Template)]
#[template(path = "join.html")]
struct Join;

#[derive(Template)]
#[template(path = "waiting.html")]
struct Waiting<'a> {
    user: &'a str,
    initial: String,
    menu: Menu,
}

#[derive(Template)]
#[template(path = "files.html")]
struct Files<'a> {
    user: &'a str,
    initial: String,
    menu: Menu,
}

#[derive(Template)]
#[template(path = "media.html")]
struct Media<'a> {
    user: &'a str,
    initial: String,
    menu: Menu,
}

#[derive(Template)]
#[template(path = "photos.html")]
struct Photos<'a> {
    user: &'a str,
    initial: String,
    menu: Menu,
}

#[derive(Template)]
#[template(path = "panel.html")]
struct Panel<'a> {
    user: &'a str,
    initial: String,
    menu: Menu,
    title: &'static str,
    waiting: &'static str,
    note: &'static str,
    script: &'static str,
}

fn panel(
    user: &str,
    services: &[Service],
    title: &'static str,
    waiting: &'static str,
    note: &'static str,
    script: &'static str,
) -> String {
    render(Panel {
        user,
        initial: initial(user),
        menu: Menu::of(user, services),
        title,
        waiting,
        note,
        script,
    })
}

/// What every box is running, and whether it answered at all.
pub fn boxes(user: &str, services: &[Service]) -> String {
    panel(
        user,
        services,
        "Boxes",
        "Asking every box…",
        "Each box answers for itself, over the fleet's own network. A box that says nothing is off or unreachable, not gone.",
        "boxes.js",
    )
}

/// What has been backed up and how far back it goes. Reading one back is
/// not something a browser does: it is `restic restore` on the box, with
/// the password only that box holds.
pub fn backups(user: &str, services: &[Service]) -> String {
    panel(
        user,
        services,
        "Backups",
        "Asking every box…",
        "A box backs itself up, encrypted with a password only it holds, into the cluster. This page says what happened; restoring is done on the box.",
        "backups.js",
    )
}

/// The keys that are you: the devices in your entry, and the passkeys.
pub fn devices(user: &str, services: &[Service]) -> String {
    panel(
        user,
        services,
        "Devices",
        "Reading your entry…",
        "Your entry names these, and only a device holding your root key can add or remove one (`dd device`).",
        "devices.js",
    )
}

/// The machines on the fleet's own network, as the control server has them.
pub fn network_page(user: &str, services: &[Service]) -> String {
    panel(
        user,
        services,
        "Network",
        "Asking the control server…",
        "The fleet's own network, so your devices reach the boxes wherever they are. `dd net join` puts a machine on it.",
        "network.js",
    )
}

struct Tile<'a> {
    name: &'a str,
    url: &'a str,
    icon: &'static str,
    color: &'a str,
    shut: bool,
}

#[derive(Template)]
#[template(path = "home.html")]
struct Home<'a> {
    user: &'a str,
    initial: String,
    demo: bool,
    tiles: Vec<Tile<'a>>,
    menu: Menu,
}

fn render<T: Template>(t: T) -> String {
    t.render().unwrap_or_default()
}

/// Sign in: username, then the passkey.
pub fn login() -> String {
    render(Login)
}

/// Set up a passkey, from a link a device signed.
pub fn enrol() -> String {
    render(Enrol)
}

/// An account, from nothing, in the browser: a name and a passkey.
pub fn join() -> String {
    render(Join)
}

/// Signed in but not on the member list: the account exists, nothing is
/// open to it yet. A code from the owner opens it here.
pub fn waiting(user: &str, services: &[Service]) -> String {
    render(Waiting {
        user,
        initial: initial(user),
        menu: Menu::of(user, services),
    })
}

/// Files: a member's library, opened by their passkey.
pub fn files(user: &str, services: &[Service]) -> String {
    render(Files {
        user,
        initial: initial(user),
        menu: Menu::of(user, services),
    })
}

/// Movies & TV: the same library, the corners a player cares about.
pub fn media(user: &str, services: &[Service]) -> String {
    render(Media {
        user,
        initial: initial(user),
        menu: Menu::of(user, services),
    })
}

/// Photos: opened by the passkey, or by the demo's password.
pub fn photos(user: &str, services: &[Service]) -> String {
    render(Photos {
        user,
        initial: initial(user),
        menu: Menu::of(user, services),
    })
}

/// The signed-in home page: the services this box offers, as tiles.
pub fn home(user: &str, services: &[Service]) -> String {
    let demo = user == DEMO_USER;
    render(Home {
        user,
        initial: initial(user),
        demo,
        tiles: services
            .iter()
            .map(|s| Tile {
                name: &s.name,
                url: match (demo, &s.demo_url) {
                    (true, Some(u)) => u,
                    _ => &s.url,
                },
                icon: icon(&s.icon),
                color: &s.color,
                // a door this account has no key to: shown, shut, and why
                shut: demo && s.demo.is_none(),
            })
            .collect(),
        menu: Menu::of(user, services),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn svc(name: &str, icon: &str) -> Service {
        Service {
            name: name.into(),
            url: format!("https://{}.example/", name.to_lowercase()),
            icon: icon.into(),
            color: "#123456".into(),
            demo: None,
            demo_url: None,
        }
    }

    #[test]
    fn auth_pages_are_one_shell_one_script() {
        for page in [login(), enrol(), join()] {
            assert!(page.starts_with("<!doctype html>"));
            assert!(page.trim_end().ends_with("</html>"));
            assert_eq!(page.matches("<script").count(), 1);
            assert_eq!(page.matches("</header>").count(), 1);
            assert!(page.contains("/_dd/static/home.css"));
        }
        assert!(login().contains("dd enrol"));
        assert!(enrol().contains("dd enrol"));
        assert!(login().contains("/_dd/static/login.js"));
        assert!(login().contains("href=\"/_dd/join\""));
        assert!(enrol().contains("/_dd/static/enrol.js"));
        assert!(join().contains("/_dd/static/join.js"));
        for f in ["login.js", "join.js", "enrol.js", "redeem.js", "photos.js"] {
            assert!(static_file(f).is_some(), "{f}");
        }
        assert!(
            static_file("login.js")
                .unwrap()
                .0
                .contains("/_dd/login/start")
        );
        assert!(static_file("join.js").unwrap().0.contains("/_dd/join/sign"));
        assert!(
            static_file("enrol.js")
                .unwrap()
                .0
                .contains("/_dd/enrol/start")
        );
        assert!(
            static_file("redeem.js")
                .unwrap()
                .0
                .contains("/_dd/redeem/start")
        );
        assert!(static_file("nope").is_none());
    }

    #[test]
    fn the_menu_offers_a_member_their_own_pages_and_the_demo_none_of_them() {
        let svcs = [svc("Metrics", "metrics"), svc("Chat", "chat")];
        let html = home("tom", &svcs);
        for page in [
            "/_dd/files",
            "/_dd/media",
            "/_dd/backups",
            "/_dd/devices",
            "/_dd/network",
            "/_dd/boxes",
        ] {
            assert!(html.contains(page), "member's menu is missing {page}");
        }
        assert!(html.contains("https://metrics.example/"));
        assert!(html.contains("/_dd/logout"));
        let html = home(DEMO_USER, &svcs);
        for page in [
            "/_dd/files",
            "/_dd/media",
            "/_dd/backups",
            "/_dd/devices",
            "/_dd/network",
            "/_dd/boxes",
        ] {
            assert!(!html.contains(page), "the demo was offered {page}");
        }
        assert!(html.contains("https://metrics.example/") && html.contains("/_dd/logout"));
        // no metrics on this box: no line for it, and nothing else moves
        let html = home("tom", &[svc("Chat", "chat")]);
        assert!(!html.contains("Metrics") && html.contains("/_dd/boxes"));
    }

    #[test]
    fn the_library_pages_carry_the_member_and_their_script() {
        let html = files("tom", &[]);
        assert!(html.contains("data-user=\"tom\""));
        assert!(html.contains("/_dd/static/files.js"));
        let html = media("tom", &[svc("Metrics", "metrics")]);
        assert!(html.contains("data-user=\"tom\""));
        assert!(html.contains("/_dd/static/media.js"));
        assert!(html.contains("Movies") && html.contains("Shows"));
        // both pages open the library through the one module
        for f in ["files.js", "media.js"] {
            assert!(static_file(f).unwrap().0.contains("from './library.js'"));
        }
        assert!(static_file("library.js").unwrap().0.contains("/_dd/dav/"));
    }

    #[test]
    fn waiting_page_names_the_person_and_offers_nothing() {
        let html = waiting("tom", &[]);
        assert!(html.contains("<span>tom</span>"));
        assert!(html.contains("data-user=\"tom\""));
        assert!(html.contains("/_dd/logout"));
        assert!(!html.contains("class=\"tile\""));
        assert!(html.contains("/_dd/static/redeem.js"));
    }

    #[test]
    fn the_demo_sees_every_tile_and_the_shut_ones_greyed() {
        let mut files = svc("Files", "files");
        files.url = "https://files.x/".into();
        files.demo = Some("read".into());
        let mut chat = svc("Chat", "chat");
        chat.url = "https://llm.x/".into();
        let html = home(DEMO_USER, &[files.clone(), chat.clone()]);
        assert!(html.contains("This is a demo"));
        assert!(html.contains("href=\"https://files.x/\""));
        assert!(html.contains("Chat") && html.contains("Not in the demo."));
        assert!(!html.contains("href=\"https://llm.x/\""));
        let html = home("tom", &[files, chat]);
        assert!(!html.contains("This is a demo") && !html.contains("Not in the demo."));
        assert!(html.contains("href=\"https://llm.x/\""));
    }

    #[test]
    fn a_tile_can_send_the_demo_somewhere_else() {
        let mut tv = svc("Movies & TV", "videos");
        tv.url = "https://files.x/_dd/media".into();
        tv.demo_url = Some("https://jellyfin.x/sso".into());
        tv.demo = Some("full".into());
        let html = home("tom", &[tv.clone()]);
        assert!(html.contains("href=\"https://files.x/_dd/media\""));
        assert!(!html.contains("jellyfin"));
        let html = home(DEMO_USER, &[tv]);
        assert!(html.contains("href=\"https://jellyfin.x/sso\""));
        assert!(!html.contains("/_dd/media"));
    }

    #[test]
    fn a_tile_per_service_with_its_mark() {
        let html = home("tom", &[svc("Photos", "photos"), svc("Odd", "odd")]);
        assert_eq!(html.matches("class=\"tile\"").count(), 2);
        assert!(html.contains("<h2>Photos</h2>"));
        assert!(html.contains("href=\"https://photos.example/\""));
        assert!(html.contains("cx=\"7.5\""), "the photos mark");
        assert!(html.contains("rx=\"3\""), "the plain mark");
        assert!(html.contains("<span>tom</span>"));
        assert!(html.contains("/_dd/logout"));
        let html = home("tom", &[]);
        assert!(html.contains("Nothing runs here yet"));
    }
}
