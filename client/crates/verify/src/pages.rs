//! The pages a browser sees. Plain html; the login and enrolment pages carry
//! the WebAuthn calls, the home page carries no script at all. Nothing is
//! loaded from anywhere.

pub const LOGIN: &str = r#"<!doctype html><html lang="en"><head><meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1"><title>Sign in</title>
<style>:root{color-scheme:light dark}body{font:16px/1.5 system-ui,sans-serif;margin:0;padding:2rem 1rem;display:flex;justify-content:center}
main{width:100%;max-width:24rem}h1{font-size:1.4rem;margin:0 0 .5rem}p{opacity:.75}button{width:100%;padding:.7rem;font:inherit;font-weight:600;border:0;border-radius:6px;cursor:pointer;background:currentColor;margin-top:1rem}
button span{color:Canvas}#msg{margin-top:1rem;min-height:1.5em}a{color:inherit}</style></head><body><main>
<h1>Sign in</h1><p>With the passkey you set up for this network. Nothing leaves your device but a signature.</p>
<label for="u" style="display:block;font-weight:600;margin-top:1rem">Username</label>
<input id="u" autocomplete="username webauthn" autocapitalize="none" spellcheck="false" style="width:100%;box-sizing:border-box;padding:.6rem .7rem;font:inherit;border:1px solid currentColor;border-radius:6px;background:transparent;color:inherit">
<button id="go"><span>Use passkey</span></button><div id="msg"></div>
<p style="font-size:.9rem">First time in a browser? On a device that holds your key, run <code>dd enrol</code> and open the link it prints.</p>
<script>
const rd=new URLSearchParams(location.search).get('rd')||'/';
const b64u=s=>Uint8Array.from(atob(s.replace(/-/g,'+').replace(/_/g,'/')),c=>c.charCodeAt(0));
const u8b64=a=>btoa(String.fromCharCode(...new Uint8Array(a))).replace(/\+/g,'-').replace(/\//g,'_').replace(/=+$/,'');
async function go(){const m=document.getElementById('msg');m.textContent='…';try{
const u=document.getElementById('u').value.trim();try{localStorage.setItem('dd_user',u)}catch(e){}
const r=await fetch('/_dd/login/start',{method:'POST',headers:{'content-type':'application/json'},body:JSON.stringify({username:u})});if(!r.ok)throw new Error(await r.text());
const {publicKey,ceremony}=await r.json();
publicKey.challenge=b64u(publicKey.challenge);
if(publicKey.allowCredentials)publicKey.allowCredentials=publicKey.allowCredentials.map(c=>({...c,id:b64u(c.id)}));
const cred=await navigator.credentials.get({publicKey});
const body={id:cred.id,rawId:u8b64(cred.rawId),type:cred.type,extensions:cred.getClientExtensionResults(),response:{
authenticatorData:u8b64(cred.response.authenticatorData),clientDataJSON:u8b64(cred.response.clientDataJSON),
signature:u8b64(cred.response.signature),userHandle:cred.response.userHandle?u8b64(cred.response.userHandle):null}};
const f=await fetch('/_dd/login/finish',{method:'POST',headers:{'content-type':'application/json','x-dd-ceremony':ceremony},body:JSON.stringify(body)});
if(!f.ok)throw new Error(await f.text());location.href=rd;}catch(e){m.textContent='Sign-in failed: '+e.message}}
document.getElementById('go').onclick=go;
try{document.getElementById('u').value=localStorage.getItem('dd_user')||''}catch(e){}
</script></main></body></html>"#;

pub const ENROL: &str = r#"<!doctype html><html lang="en"><head><meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1"><title>Set up a passkey</title>
<style>:root{color-scheme:light dark}body{font:16px/1.5 system-ui,sans-serif;margin:0;padding:2rem 1rem;display:flex;justify-content:center}
main{width:100%;max-width:24rem}h1{font-size:1.4rem;margin:0 0 .5rem}p{opacity:.75}button{width:100%;padding:.7rem;font:inherit;font-weight:600;border:0;border-radius:6px;cursor:pointer;background:currentColor;margin-top:1rem}
button span{color:Canvas}#msg{margin-top:1rem;min-height:1.5em}</style></head><body><main>
<h1>Set up a passkey</h1><p>For signing in to this network from a browser. You got here from a link a device of yours signed. The passkey goes into your own signed entry, so every box can check it and none can add one.</p>
<button id="go"><span>Create passkey</span></button><div id="msg"></div>
<p id="how" style="font-size:.9rem" hidden>No signed link? On a device that holds your key, run <code>dd enrol</code> and open the address it prints.</p>
<script>
const q=new URLSearchParams(location.search);const rd=q.get('rd')||'/';const t=q.get('t');
const hdr=t?{'authorization':'Bearer '+t}:{};
const b64u=s=>Uint8Array.from(atob(s.replace(/-/g,'+').replace(/_/g,'/')),c=>c.charCodeAt(0));
const u8b64=a=>btoa(String.fromCharCode(...new Uint8Array(a))).replace(/\+/g,'-').replace(/\//g,'_').replace(/=+$/,'');
async function go(){const m=document.getElementById('msg');m.textContent='…';try{
const r=await fetch('/_dd/enrol/start',{method:'POST',headers:hdr});
if(r.status===401){m.textContent='This link is not valid, or has expired.';document.getElementById('how').hidden=false;return}
if(!r.ok)throw new Error(await r.text());
const {publicKey,ceremony}=await r.json();
publicKey.challenge=b64u(publicKey.challenge);publicKey.user.id=b64u(publicKey.user.id);
if(publicKey.excludeCredentials)publicKey.excludeCredentials=publicKey.excludeCredentials.map(c=>({...c,id:b64u(c.id)}));
const cred=await navigator.credentials.create({publicKey});
const body={id:cred.id,rawId:u8b64(cred.rawId),type:cred.type,extensions:cred.getClientExtensionResults(),response:{
attestationObject:u8b64(cred.response.attestationObject),clientDataJSON:u8b64(cred.response.clientDataJSON)}};
const f=await fetch('/_dd/enrol/finish',{method:'POST',headers:{'content-type':'application/json','x-dd-ceremony':ceremony,...hdr},body:JSON.stringify(body)});
if(!f.ok)throw new Error(await f.text());const {id,user}=await f.json();
m.textContent='Passkey made. Waiting for the terminal to sign it into your entry…';
for(let i=0;i<90;i++){await new Promise(r=>setTimeout(r,2000));try{const e=await fetch('/_dd/directory/'+encodeURIComponent(user));if(e.ok){const j=await e.json();if((j.entry.passkeys||[]).some(p=>p.id===id)){m.textContent='Signed in to your entry. Taking you to sign in…';location.href='/_dd/login?rd='+encodeURIComponent(rd);return}}}catch(e){}}
m.textContent='Passkey made, but it has not appeared in your entry yet. Once the terminal reports it published, sign in.';}catch(e){m.textContent='Failed: '+e.message}}
document.getElementById('go').onclick=go;
</script></main></body></html>"#;

/// One tile on the home page. The roles a box runs declare these.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
pub struct Service {
    pub name: String,
    pub url: String,
    pub description: String,
    /// photos, videos, files, chat, code, metrics, or anything else for a plain mark
    pub icon: String,
    /// a css colour for the tile's icon
    pub color: String,
}

fn esc(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

fn icon(key: &str) -> &'static str {
    match key {
        "photos" => {
            r#"<rect x="2.5" y="4" width="15" height="12" rx="2"/><circle cx="7.5" cy="8.5" r="1.5"/><path d="M4 15l4.5-4.5 3 3 2-2L17 15"/>"#
        }
        "videos" => {
            r#"<rect x="2.5" y="4" width="15" height="12" rx="2"/><path d="M8.5 7.5v5l4-2.5z" fill="currentColor" stroke="none"/>"#
        }
        "files" => {
            r#"<path d="M3 6.5a1.5 1.5 0 011.5-1.5h3.2l1.6 2h6.2A1.5 1.5 0 0117 8.5v6a1.5 1.5 0 01-1.5 1.5h-11A1.5 1.5 0 013 14.5z"/>"#
        }
        "chat" => {
            r#"<path d="M3.5 5.5A1.5 1.5 0 015 4h10a1.5 1.5 0 011.5 1.5v7A1.5 1.5 0 0115 14H9l-3.5 3v-3H5a1.5 1.5 0 01-1.5-1.5z"/><path d="M7 8h6M7 10.5h4"/>"#
        }
        "code" => {
            r#"<circle cx="6" cy="5" r="2"/><circle cx="6" cy="15" r="2"/><circle cx="14" cy="8" r="2"/><path d="M6 7v6M14 10c0 3-8 2-8 5"/>"#
        }
        "metrics" => r#"<path d="M3 15.5h14"/><path d="M4.5 12l3.5-4 3 2.5 4.5-6"/>"#,
        _ => r#"<rect x="3" y="3" width="14" height="14" rx="3"/>"#,
    }
}

const HOME_HEAD: &str = r##"<!doctype html><html lang="en"><head><meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1"><title>Distributed Datacenter</title>
<style>
:root{color-scheme:light dark;--brand:#124e63;--brand-ink:#fff;--bg:#eef3f5;--bg-2:#e2eaee;--card:#fff;--ink:#142129;--ink-2:#5b6b74;
--shadow:0 1px 2px rgba(18,78,99,.06),0 10px 30px -14px rgba(18,78,99,.35);--shadow-hover:0 2px 4px rgba(18,78,99,.08),0 18px 40px -16px rgba(18,78,99,.45);
--font:-apple-system,BlinkMacSystemFont,"Segoe UI","Noto Sans",Helvetica,Arial,sans-serif}
@media (prefers-color-scheme:dark){:root:not([data-theme="light"]){--brand:#0d3a4a;--bg:#0b151b;--bg-2:#0f1d25;--card:#142430;--ink:#e7eef2;--ink-2:#93a5b0;
--shadow:0 1px 2px rgba(0,0,0,.3),0 10px 30px -14px rgba(0,0,0,.7);--shadow-hover:0 2px 4px rgba(0,0,0,.35),0 18px 40px -16px rgba(0,0,0,.8)}}
:root[data-theme="dark"]{--brand:#0d3a4a;--bg:#0b151b;--bg-2:#0f1d25;--card:#142430;--ink:#e7eef2;--ink-2:#93a5b0;
--shadow:0 1px 2px rgba(0,0,0,.3),0 10px 30px -14px rgba(0,0,0,.7);--shadow-hover:0 2px 4px rgba(0,0,0,.35),0 18px 40px -16px rgba(0,0,0,.8)}
*{box-sizing:border-box}body{margin:0;min-height:100vh;background:linear-gradient(180deg,var(--bg-2) 0,var(--bg) 320px);color:var(--ink);font-family:var(--font);font-size:15px;line-height:1.5;-webkit-font-smoothing:antialiased}
a{color:inherit;text-decoration:none}a:focus-visible{outline:3px solid #7fb3c4;outline-offset:3px;border-radius:16px}
header{background:var(--brand);color:var(--brand-ink)}.bar{max-width:980px;margin:0 auto;padding:16px 24px;display:flex;align-items:center;justify-content:space-between;gap:16px}
.brand{display:flex;align-items:center;gap:12px;font-weight:600;font-size:17px;letter-spacing:-.01em}.brand svg{width:26px;height:26px}
.me{display:flex;align-items:center;gap:12px;font-size:14px}.me .avatar{width:32px;height:32px;border-radius:50%;display:grid;place-items:center;background:rgba(255,255,255,.18);font-weight:600;font-size:14px;text-transform:uppercase}
.me a.out{padding:7px 12px;border-radius:8px;background:rgba(255,255,255,.12);font-weight:500}.me a.out:hover{background:rgba(255,255,255,.22)}
main{max-width:980px;margin:0 auto;padding-inline:24px;padding-block:48px 72px}h1{margin:0 0 28px;font-size:28px;font-weight:700;letter-spacing:-.02em}
.grid{display:grid;grid-template-columns:repeat(3,minmax(0,1fr));gap:20px;margin:0;padding:0;list-style:none}
@media (max-width:800px){.grid{grid-template-columns:repeat(2,minmax(0,1fr))}}@media (max-width:480px){.grid{grid-template-columns:1fr}}
.tile{display:flex;flex-direction:column;gap:18px;padding:24px;background:var(--card);border-radius:16px;box-shadow:var(--shadow);transition:transform .15s ease,box-shadow .15s ease}
.tile:hover{transform:translateY(-2px);box-shadow:var(--shadow-hover)}
.icon{width:56px;height:56px;border-radius:14px;display:grid;place-items:center;color:#fff;box-shadow:inset 0 -2px 0 rgba(0,0,0,.12),inset 0 1px 0 rgba(255,255,255,.18)}.icon svg{width:28px;height:28px}
.tile h2{margin:0 0 4px;font-size:18px;font-weight:650;letter-spacing:-.01em}.tile p{margin:0;color:var(--ink-2);font-size:14px;line-height:1.45}
.empty{color:var(--ink-2)}@media (prefers-reduced-motion:reduce){.tile{transition:none}.tile:hover{transform:none}}
</style></head><body>
<header><div class="bar"><a class="brand" href="/" aria-label="Distributed Datacenter"><svg viewBox="0 0 26 26" fill="none" aria-hidden="true"><rect x="3" y="4" width="20" height="7" rx="2.5" fill="rgba(255,255,255,.22)" stroke="currentColor" stroke-width="1.8"/><rect x="3" y="15" width="20" height="7" rx="2.5" fill="rgba(255,255,255,.22)" stroke="currentColor" stroke-width="1.8"/><circle cx="7.5" cy="7.5" r="1.3" fill="currentColor"/><circle cx="7.5" cy="18.5" r="1.3" fill="currentColor"/></svg>Distributed Datacenter</a>"##;

/// The signed-in home page: the services this box offers, as tiles. Server
/// rendered, so the browser runs nothing.
pub fn home(user: &str, services: &[Service]) -> String {
    let mut out = String::with_capacity(8192);
    out.push_str(HOME_HEAD);
    let initial = user
        .chars()
        .next()
        .map(|c| esc(&c.to_string()))
        .unwrap_or_default();
    out.push_str(&format!(
        r#"<div class="me"><span class="avatar" aria-hidden="true">{initial}</span><span>{}</span><a class="out" href="/_dd/logout">Sign out</a></div></div></header>
<main><h1>Your services</h1>"#,
        esc(user)
    ));
    if services.is_empty() {
        out.push_str(r#"<p class="empty">Nothing runs here yet.</p>"#);
    } else {
        out.push_str(r#"<ul class="grid">"#);
        for s in services {
            out.push_str(&format!(
                r#"<li><a class="tile" href="{}"><span class="icon" style="background:{}" aria-hidden="true"><svg viewBox="0 0 20 20" fill="none" stroke="currentColor" stroke-width="1.6">{}</svg></span><span><h2>{}</h2><p>{}</p></span></a></li>"#,
                esc(&s.url),
                esc(&s.color),
                icon(&s.icon),
                esc(&s.name),
                esc(&s.description)
            ));
        }
        out.push_str("</ul>");
    }
    out.push_str("</main></body></html>");
    out
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
        }
    }

    #[test]
    fn lists_every_service_with_its_link() {
        let html = home("david", &[svc("Photos", "photos"), svc("Code", "code")]);
        assert!(html.contains("<h2>Photos</h2>"));
        assert!(html.contains(r#"href="https://code.example/""#));
        assert!(html.contains(">david<"));
        assert!(!html.contains("<script"));
    }

    #[test]
    fn escapes_what_it_prints() {
        let mut s = svc("Photos", "photos");
        s.description = "a <b>bold</b> & \"quoted\" claim".into();
        let html = home("<script>x</script>", &[s]);
        assert!(!html.contains("<script>x"));
        assert!(html.contains("&lt;b&gt;bold&lt;/b&gt; &amp; &quot;quoted&quot;"));
    }

    #[test]
    fn a_box_with_nothing_says_so() {
        assert!(home("david", &[]).contains("Nothing runs here yet"));
    }
}
