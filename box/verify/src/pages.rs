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
    /// In the bar's menu and not on the home page. Looking at the fleet
    /// is not a service the way photos and films are, and a tile for it
    /// sits oddly beside them.
    #[serde(
        rename = "menuOnly",
        default,
        skip_serializing_if = "std::ops::Not::not"
    )]
    pub menu_only: bool,
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
        // The tile's url, where a tile says. A library page belongs on the
        // gate's own host: that is the only one serving /_dd/transcode, so
        // a relative link followed from another host plays nothing.
        let demo = user == DEMO_USER;
        let mut groups = Vec::new();
        if !demo {
            // Files and Movies & TV are tiles on the home page; repeating
            // them here would be the same door twice
            let mut fleet = vec![
                item("Backups", "/_dd/backups"),
                item("Devices", "/_dd/devices"),
                item("Network", "/_dd/network"),
                item("Boxes", "/_dd/boxes"),
            ];
            fleet.extend(metrics);
            groups.push(fleet);
        } else if let Some(m) = metrics {
            // the fleet's pages mean nothing to an account with no
            // devices, no backups and no boxes of its own
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

/// one platform's card on the downloads page
struct Platform {
    name: &'static str,
    icon: &'static str,
    /// what to call the file, and where it is; empty means not yet
    files: Vec<(&'static str, String)>,
    /// shown under the name when there is nothing to download
    soon: &'static str,
}

struct Group {
    title: &'static str,
    platforms: Vec<Platform>,
}

#[derive(Template)]
#[template(path = "download.html")]
struct Download<'a> {
    domain: &'a str,
    repo: &'a str,
    groups: Vec<Group>,
}

/// the logo on a platform's card, from web/icons/os/
fn os_icon(key: &str) -> &'static str {
    match key {
        "linux" => include_str!("../web/icons/os/linux.svg"),
        "android" => include_str!("../web/icons/os/android.svg"),
        "apple" => include_str!("../web/icons/os/apple.svg"),
        _ => include_str!("../web/icons/os/windows.svg"),
    }
}

/// Where a stranger gets the app. Public: someone invited has nothing to
/// sign in with until they have it. The artifacts are built after a tag
/// and published as a release on the mirror, so the links point there by
/// that tag, not at a file this box holds. A platform with no files is
/// shown anyway, so the page says what is coming rather than hiding it.
pub fn download(domain: &str, repo: &str, version: &str) -> String {
    let at = |name: String| format!("{repo}/releases/download/{version}/{name}");
    let v = version.trim_start_matches('v');
    let groups = vec![
        Group {
            title: "Desktop",
            platforms: vec![
                Platform {
                    name: "Linux",
                    icon: os_icon("linux"),
                    files: vec![
                        (".deb", at(format!("commonty_{v}_amd64.deb"))),
                        ("AppImage", at(format!("commonty_{v}_amd64.AppImage"))),
                    ],
                    soon: "",
                },
                Platform {
                    name: "Windows",
                    icon: os_icon("windows"),
                    files: vec![],
                    soon: "Not built yet",
                },
                Platform {
                    name: "macOS",
                    icon: os_icon("apple"),
                    files: vec![],
                    soon: "Not built yet",
                },
            ],
        },
        Group {
            title: "Mobile",
            platforms: vec![
                Platform {
                    name: "Android",
                    icon: os_icon("android"),
                    files: vec![("APK", at("commonty.apk".into()))],
                    soon: "",
                },
                Platform {
                    name: "iOS",
                    icon: os_icon("apple"),
                    files: vec![],
                    soon: "Not built yet",
                },
            ],
        },
    ];
    render(Download {
        domain,
        repo,
        groups,
    })
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
        "Each box answers for itself, over the fleet's own network, and says when it last backed itself up. A box that says nothing is off or unreachable, not gone.",
        "boxes.js",
    )
}

/// The disks people have archived here: `dd image` writes an old
/// computer into their own folder, in restic's format, with a password
/// that never leaves the machine that made it. The box's own backups are
/// fleet health and live on the Boxes page.
pub fn backups(user: &str, services: &[Service]) -> String {
    panel(
        user,
        services,
        "Backups",
        "Looking for your archives…",
        "Disks you have put here with `dd image`. The box stores them and cannot read them: the password never left the machine that made the archive, so listing what is inside is `dd image list` there.",
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
            .filter(|s| !s.menu_only)
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
            menu_only: false,
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
    fn the_downloads_page_asks_for_nothing_and_points_at_the_release() {
        let html = download("commonty.org", "https://github.com/x/y", "v0.2.0");
        assert!(html.contains("https://github.com/x/y/releases/download/v0.2.0/commonty.apk"));
        assert!(html.contains("commonty_0.2.0_amd64.deb"));
        assert!(html.contains("commonty_0.2.0_amd64.AppImage"));
        // desktop and mobile, each with what is there and what is not
        assert!(html.contains("Desktop") && html.contains("Mobile"));
        for os in ["Linux", "macOS", "Windows", "Android", "iOS"] {
            assert!(html.contains(os), "no card for {os}");
        }
        // a platform with nothing to download says so and offers no link
        assert_eq!(html.matches("class=\"os off\"").count(), 3);
        // two formats collapse into one control, one format is a button
        // the button takes the first format; the arrow offers every one
        assert_eq!(html.matches("class=\"split\"").count(), 1);
        assert!(
            html.contains("Download .deb"),
            "the default is not on the button"
        );
        assert!(html.contains(">.deb<") && html.contains(">AppImage<"));
        // and a platform with one format is a plain button, no arrow
        assert_eq!(html.matches("class=\"get\"").count(), 2);
        // a stranger is who this is for: no name, no avatar, no menu
        assert!(!html.contains("class=\"me\"") && !html.contains("/_dd/logout"));
        assert!(html.contains("https://home.commonty.org/"));
    }

    #[test]
    fn the_menu_offers_a_member_their_own_pages_and_the_demo_none_of_them() {
        let svcs = [svc("Metrics", "metrics"), svc("Chat", "chat")];
        let html = home("tom", &svcs);
        for page in ["/_dd/backups", "/_dd/devices", "/_dd/network", "/_dd/boxes"] {
            assert!(html.contains(page), "member's menu is missing {page}");
        }
        // and these are tiles, so the menu must not repeat them
        for page in ["/_dd/files", "/_dd/media"] {
            assert!(!html.contains(page), "the menu repeats the {page} tile");
        }
        assert!(html.contains("https://metrics.example/"));
        assert!(html.contains("/_dd/logout"));
        // the demo opens the library the box keeps for it, and nothing
        // that belongs to an account with devices and boxes of its own
        let html = home(DEMO_USER, &svcs);
        for page in ["/_dd/backups", "/_dd/devices", "/_dd/network", "/_dd/boxes"] {
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
        // a film goes through the box, with the key sealed to it
        let lib = static_file("library.js").unwrap().0;
        assert!(lib.contains("/_dd/transcode/start") && lib.contains("library_key_for_box"));
        // the player comes from this box, never from someone else's
        assert!(lib.contains("'/_dd/web/hls.js'"));
        assert!(!lib.contains("http://") && !lib.contains("https://"));
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
        assert!(!html.contains("files.x/_dd/media"));
    }

    #[test]
    fn the_name_is_what_opens_the_menu() {
        let html = home("tom", &[svc("Chat", "chat")]);
        // the name and the avatar are inside the control, not beside it
        let s = html.split("<summary>").nth(1).unwrap_or_default();
        let s = s.split("</summary>").next().unwrap_or_default();
        assert!(
            s.contains("<span>tom</span>"),
            "the name is not the control: {s}"
        );
        assert!(s.contains("class=\"avatar\""));
        // and nothing else in the bar competes with it
        assert!(!html.contains("aria-label=\"Menu\""));
    }

    #[test]
    fn a_menu_only_service_is_in_the_menu_and_not_a_tile() {
        let mut m = svc("Metrics", "metrics");
        m.menu_only = true;
        let html = home("tom", &[svc("Photos", "photos"), m.clone()]);
        assert_eq!(
            html.matches("class=\"tile\"").count(),
            1,
            "metrics got a tile"
        );
        assert!(
            html.contains("https://metrics.example/"),
            "metrics left the menu too"
        );
        // the name it is given comes from the service, so the json the
        // module emits and the field here have to agree
        let json = serde_json::to_string(&m).unwrap();
        assert!(json.contains("menuOnly"), "{json}");
        let back: Service = serde_json::from_str(&json).unwrap();
        assert!(back.menu_only);
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
