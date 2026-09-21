//! The Games page. Server-rendered, forms and links, no script: the same
//! look as the home page it is reached from.

use crate::{Instance, Manager, State};

fn esc(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

const CSS: &str = r##":root{color-scheme:light dark;--brand:#124e63;--brand-ink:#fff;--bg:#eef3f5;--bg-2:#e2eaee;--card:#fff;--ink:#142129;--ink-2:#5b6b74;--line:#d3dde2;--ok:#2f9e6f;--warn:#c4562d;
--shadow:0 1px 2px rgba(18,78,99,.06),0 10px 30px -14px rgba(18,78,99,.35);
--font:-apple-system,BlinkMacSystemFont,"Segoe UI","Noto Sans",Helvetica,Arial,sans-serif;--mono:ui-monospace,SFMono-Regular,Menlo,Consolas,monospace}
@media (prefers-color-scheme:dark){:root:not([data-theme="light"]){--brand:#0d3a4a;--bg:#0b151b;--bg-2:#0f1d25;--card:#142430;--ink:#e7eef2;--ink-2:#93a5b0;--line:#223442;--shadow:0 1px 2px rgba(0,0,0,.3),0 10px 30px -14px rgba(0,0,0,.7)}}
:root[data-theme="dark"]{--brand:#0d3a4a;--bg:#0b151b;--bg-2:#0f1d25;--card:#142430;--ink:#e7eef2;--ink-2:#93a5b0;--line:#223442;--shadow:0 1px 2px rgba(0,0,0,.3),0 10px 30px -14px rgba(0,0,0,.7)}
*{box-sizing:border-box}body{margin:0;min-height:100vh;background:linear-gradient(180deg,var(--bg-2) 0,var(--bg) 320px);color:var(--ink);font-family:var(--font);font-size:15px;line-height:1.5;-webkit-font-smoothing:antialiased}
a{color:inherit}header{background:var(--brand);color:var(--brand-ink)}.bar{max-width:980px;margin:0 auto;padding:16px 24px;display:flex;align-items:center;justify-content:space-between;gap:16px}
.brand{font-weight:600;font-size:17px;letter-spacing:-.01em;text-decoration:none}.me{font-size:14px}
main{max-width:980px;margin:0 auto;padding-inline:24px;padding-block:40px 72px}h1{margin:0 0 20px;font-size:26px;font-weight:700;letter-spacing:-.02em}h2{margin:36px 0 14px;font-size:18px}
.note{margin:0 0 22px;padding:12px 16px;border-radius:12px;background:var(--card);box-shadow:var(--shadow);border-left:4px solid var(--warn);font-size:14px}
.grid{display:grid;grid-template-columns:repeat(auto-fill,minmax(270px,1fr));gap:18px;margin:0;padding:0;list-style:none}
.card{display:flex;flex-direction:column;gap:10px;padding:20px;background:var(--card);border-radius:16px;box-shadow:var(--shadow)}
.card h3{margin:0;font-size:17px}.card p{margin:0;color:var(--ink-2);font-size:14px}
.addr{font-family:var(--mono);font-size:13.5px;background:var(--bg-2);padding:6px 10px;border-radius:8px;overflow-wrap:anywhere}
.state{font-size:13px;font-weight:600}.state.running{color:var(--ok)}.state.failed{color:var(--warn)}
.row{display:flex;gap:8px;flex-wrap:wrap;margin-top:4px}form{margin:0}
button{padding:8px 14px;font:inherit;font-weight:600;font-size:14px;border:0;border-radius:9px;cursor:pointer;color:var(--brand-ink);background:var(--brand)}
button.quiet{background:var(--bg-2);color:var(--ink)}button:hover{filter:brightness(1.08)}button:focus-visible{outline:3px solid #7fb3c4;outline-offset:2px}
.empty{color:var(--ink-2)}"##;

fn server(m: &Manager, i: &Instance, mine: bool) -> String {
    let st = m.state(i);
    let game = m
        .cfg
        .catalogue
        .get(&i.game)
        .map(|g| g.name.as_str())
        .unwrap_or(&i.game);
    let class = match st {
        State::Running => " running",
        State::Failed => " failed",
        _ => "",
    };
    let mut out = format!(
        r#"<li class="card"><h3>{}</h3><p class="state{class}">{}</p>"#,
        esc(game),
        st.label()
    );
    if mine {
        for p in &i.ports {
            out.push_str(&format!(
                r#"<div class="addr">{}:{} <span style="opacity:.6">{}</span></div>"#,
                esc(&m.cfg.address),
                p.host,
                esc(&p.proto)
            ));
        }
        out.push_str(r#"<div class="row">"#);
        let id = esc(&i.id);
        if st == State::Stopped || st == State::Failed {
            out.push_str(&format!(
                r#"<form method="post" action="/start/{id}"><button>Start</button></form><form method="post" action="/delete/{id}"><button class="quiet">Delete, with its world</button></form>"#
            ));
        } else {
            out.push_str(&format!(
                r#"<form method="post" action="/stop/{id}"><button class="quiet">Stop</button></form>"#
            ));
        }
        out.push_str("</div>");
    } else {
        out.push_str(&format!("<p>{}'s</p>", esc(&i.owner)));
    }
    out.push_str("</li>");
    out
}

/// Everything on one page: what went wrong last (if anything), your
/// servers, the games you can start, and who else has one up.
pub fn index(m: &Manager, user: &str, notice: Option<&str>) -> String {
    let all = m.instances();
    let mut out = format!(
        r#"<!doctype html><html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width, initial-scale=1"><title>Games</title><style>{CSS}</style></head><body>
<header><div class="bar"><a class="brand" href="{}">Distributed Datacenter</a><span class="me">{}</span></div></header><main><h1>Games</h1>"#,
        esc(&m.cfg.home),
        esc(user)
    );
    if let Some(n) = notice {
        out.push_str(&format!(r#"<p class="note">{}</p>"#, esc(n)));
    }

    out.push_str("<h2>Your servers</h2>");
    let mine: Vec<&Instance> = all.iter().filter(|i| i.owner == user).collect();
    if mine.is_empty() {
        out.push_str(r#"<p class="empty">None yet. Pick a game below; it keeps running until you stop it, through updates and restarts of the machine it is on.</p>"#);
    } else {
        out.push_str(r#"<ul class="grid">"#);
        for i in mine {
            out.push_str(&server(m, i, true));
        }
        out.push_str("</ul>");
    }

    out.push_str(r#"<h2>Start one</h2><ul class="grid">"#);
    for (id, g) in &m.cfg.catalogue {
        out.push_str(&format!(
            r#"<li class="card"><h3>{}</h3><p>{}</p><p>{} GB of memory, {} cores</p><div class="row"><form method="post" action="/create/{}"><button>Start a server</button></form></div></li>"#,
            esc(&g.name),
            esc(&g.description),
            g.memory.div_ceil(1024),
            g.cores,
            esc(id)
        ));
    }
    out.push_str("</ul>");

    let others: Vec<&Instance> = all.iter().filter(|i| i.owner != user).collect();
    if !others.is_empty() {
        out.push_str(r#"<h2>Also up here</h2><ul class="grid">"#);
        for i in others {
            out.push_str(&server(m, i, false));
        }
        out.push_str("</ul>");
    }
    out.push_str("</main></body></html>");
    out
}
