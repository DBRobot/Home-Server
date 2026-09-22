//! The verifier's pages: what each template gets. The HTML is in
//! templates/, the look in web/home.css, the scripts in web/*.js, served
//! from /_dd/static/.

use askama::Template;

/// One tile on the home page. The roles a box runs declare these.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
pub struct Service {
    pub name: String,
    pub url: String,
    pub description: String,
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
}

#[derive(Template)]
#[template(path = "photos.html")]
struct Photos<'a> {
    user: &'a str,
    initial: String,
}

struct Tile<'a> {
    name: &'a str,
    url: &'a str,
    description: &'a str,
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
pub fn waiting(user: &str) -> String {
    render(Waiting {
        user,
        initial: initial(user),
    })
}

/// Photos: opened by the passkey, or by the demo's password.
pub fn photos(user: &str) -> String {
    render(Photos {
        user,
        initial: initial(user),
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
                url: &s.url,
                description: &s.description,
                icon: icon(&s.icon),
                color: &s.color,
                // a door this account has no key to: shown, shut, and why
                shut: demo && s.demo.is_none(),
            })
            .collect(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn svc(name: &str, icon: &str) -> Service {
        Service {
            name: name.into(),
            url: format!("https://{}.example/", name.to_lowercase()),
            description: "what it is".into(),
            icon: icon.into(),
            color: "#123456".into(),
            demo: None,
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
    fn waiting_page_names_the_person_and_offers_nothing() {
        let html = waiting("tom");
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
