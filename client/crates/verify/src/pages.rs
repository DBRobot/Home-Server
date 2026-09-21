//! The pages a browser sees. Plain html from one shell (tokens, header,
//! ground) so they cannot drift apart; the login and enrolment pages carry
//! the WebAuthn calls, the home page carries no script at all. Nothing is
//! loaded from anywhere.

const SHELL_CSS: &str = r##":root{color-scheme:light dark;--brand:#124e63;--brand-ink:#fff;--bg:#eef3f5;--bg-2:#e2eaee;--card:#fff;--ink:#142129;--ink-2:#5b6b74;--line:#d3dde2;
--shadow:0 1px 2px rgba(18,78,99,.06),0 10px 30px -14px rgba(18,78,99,.35);--shadow-hover:0 2px 4px rgba(18,78,99,.08),0 18px 40px -16px rgba(18,78,99,.45);
--font:-apple-system,BlinkMacSystemFont,"Segoe UI","Noto Sans",Helvetica,Arial,sans-serif}
@media (prefers-color-scheme:dark){:root:not([data-theme="light"]){--brand:#0d3a4a;--bg:#0b151b;--bg-2:#0f1d25;--card:#142430;--ink:#e7eef2;--ink-2:#93a5b0;--line:#223442;
--shadow:0 1px 2px rgba(0,0,0,.3),0 10px 30px -14px rgba(0,0,0,.7);--shadow-hover:0 2px 4px rgba(0,0,0,.35),0 18px 40px -16px rgba(0,0,0,.8)}}
:root[data-theme="dark"]{--brand:#0d3a4a;--bg:#0b151b;--bg-2:#0f1d25;--card:#142430;--ink:#e7eef2;--ink-2:#93a5b0;--line:#223442;
--shadow:0 1px 2px rgba(0,0,0,.3),0 10px 30px -14px rgba(0,0,0,.7);--shadow-hover:0 2px 4px rgba(0,0,0,.35),0 18px 40px -16px rgba(0,0,0,.8)}
*{box-sizing:border-box}body{margin:0;min-height:100vh;background:linear-gradient(180deg,var(--bg-2) 0,var(--bg) 320px);color:var(--ink);font-family:var(--font);font-size:15px;line-height:1.5;-webkit-font-smoothing:antialiased}
a{color:inherit;text-decoration:none}a:focus-visible{outline:3px solid #7fb3c4;outline-offset:3px;border-radius:16px}
header{background:var(--brand);color:var(--brand-ink)}.bar{max-width:980px;margin:0 auto;padding:16px 24px;display:flex;align-items:center;justify-content:space-between;gap:16px}
.brand{display:flex;align-items:center;gap:12px;font-weight:600;font-size:17px;letter-spacing:-.01em}.brand svg{width:26px;height:26px}
.me{display:flex;align-items:center;gap:12px;font-size:14px}.me .avatar{width:32px;height:32px;border-radius:50%;display:grid;place-items:center;background:rgba(255,255,255,.18);font-weight:600;font-size:14px;text-transform:uppercase}
.me a.out{padding:7px 12px;border-radius:8px;background:rgba(255,255,255,.12);font-weight:500}.me a.out:hover{background:rgba(255,255,255,.22)}
"##;

const HOME_CSS: &str = r##"main{max-width:980px;margin:0 auto;padding-inline:24px;padding-block:48px 72px}h1{margin:0 0 28px;font-size:28px;font-weight:700;letter-spacing:-.02em}
.grid{display:grid;grid-template-columns:repeat(3,minmax(0,1fr));gap:20px;margin:0;padding:0;list-style:none}
@media (max-width:800px){.grid{grid-template-columns:repeat(2,minmax(0,1fr))}}@media (max-width:480px){.grid{grid-template-columns:1fr}}
.tile{display:flex;flex-direction:column;gap:18px;padding:24px;background:var(--card);border-radius:16px;box-shadow:var(--shadow);transition:transform .15s ease,box-shadow .15s ease}
.tile:hover{transform:translateY(-2px);box-shadow:var(--shadow-hover)}
.icon{width:56px;height:56px;border-radius:14px;display:grid;place-items:center;color:#fff;box-shadow:inset 0 -2px 0 rgba(0,0,0,.12),inset 0 1px 0 rgba(255,255,255,.18)}.icon svg{width:28px;height:28px}
.tile h2{margin:0 0 4px;font-size:18px;font-weight:650;letter-spacing:-.01em}.tile p{margin:0;color:var(--ink-2);font-size:14px;line-height:1.45}
.empty{color:var(--ink-2)}.banner{margin:0 0 24px;padding:14px 18px;border-radius:12px;background:var(--card);box-shadow:var(--shadow);color:var(--ink-2);font-size:14px;border-left:4px solid var(--brand)}@media (prefers-reduced-motion:reduce){.tile{transition:none}.tile:hover{transform:none}}
"##;

const AUTH_CSS: &str = r##"main{max-width:420px;margin:0 auto;padding-inline:24px;padding-block:56px 72px}
.card{background:var(--card);border-radius:16px;box-shadow:var(--shadow);padding:32px 28px}
h1{margin:0 0 6px;font-size:24px;font-weight:700;letter-spacing:-.02em}.lead{margin:0 0 22px;color:var(--ink-2);font-size:14px}
label{display:block;font-size:13px;font-weight:600;margin:0 0 6px}
input{width:100%;padding:11px 12px;font:inherit;color:var(--ink);background:transparent;border:1px solid var(--line);border-radius:10px}
input:focus{outline:none;border-color:var(--brand);box-shadow:0 0 0 3px rgba(18,78,99,.22)}
button{display:block;width:100%;margin-top:18px;padding:12px;font:inherit;font-weight:600;font-size:15px;color:var(--brand-ink);background:var(--brand);border:0;border-radius:10px;cursor:pointer}
button:hover{filter:brightness(1.08)}button:focus-visible{outline:3px solid #7fb3c4;outline-offset:3px}
#msg{min-height:1.5em;margin-top:14px;font-size:14px;color:var(--ink-2)}
.note{margin:22px 0 0;padding-top:18px;border-top:1px solid var(--line);color:var(--ink-2);font-size:13px}.note a{text-decoration:underline}
code{font-family:ui-monospace,SFMono-Regular,Menlo,Consolas,monospace;font-size:12.5px;background:var(--bg-2);padding:2px 6px;border-radius:5px}
"##;

const HEADER: &str = r##"<header><div class="bar"><a class="brand" href="/" aria-label="Distributed Datacenter"><svg viewBox="0 0 26 26" fill="none" aria-hidden="true"><rect x="3" y="4" width="20" height="7" rx="2.5" fill="rgba(255,255,255,.22)" stroke="currentColor" stroke-width="1.8"/><rect x="3" y="15" width="20" height="7" rx="2.5" fill="rgba(255,255,255,.22)" stroke="currentColor" stroke-width="1.8"/><circle cx="7.5" cy="7.5" r="1.3" fill="currentColor"/><circle cx="7.5" cy="18.5" r="1.3" fill="currentColor"/></svg>Distributed Datacenter</a>"##;

/// Everything before the page's own content: head with the shell and the
/// page's stylesheet, the brand bar with `bar` (the right-hand side of the
/// bar, or nothing) inside it.
fn open(title: &str, css: &str, bar: &str) -> String {
    format!(
        concat!(
            "<!doctype html><html lang=\"en\"><head><meta charset=\"utf-8\">\n",
            "<meta name=\"viewport\" content=\"width=device-width, initial-scale=1\"><title>{}</title>\n",
            "<style>{}{}</style></head><body>\n{}{}</div></header>\n"
        ),
        title, SHELL_CSS, css, HEADER, bar
    )
}

/// Sign in: username, then the passkey.
pub fn login() -> String {
    let mut out = open("Sign in", AUTH_CSS, "");
    out.push_str(r#"<main><div class="card">
<h1>Sign in</h1><p class="lead">With the passkey you set up for this network. Nothing leaves your device but a signature.</p>
<label for="u">Username</label>
<input id="u" autocomplete="username webauthn" autocapitalize="none" spellcheck="false">
<button id="go">Use passkey</button><div id="msg"></div>
<p class="note">New here? <a href="/_dd/join">Create an account</a>, or <a href="/_dd/demo">look around first</a>.<br>Have a device with <code>dd</code> on it? Run <code>dd enrol</code> there and open the link it prints.</p>
"#);
    out.push_str(LOGIN_JS);
    out.push_str("</div></main></body></html>");
    out
}

/// Set up a passkey, from a link a device signed.
pub fn enrol() -> String {
    let mut out = open("Set up a passkey", AUTH_CSS, "");
    out.push_str(r#"<main><div class="card">
<h1>Set up a passkey</h1><p class="lead">For signing in from a browser. You got here from a link a device of yours signed; the passkey goes into your own entry, so every box can check it and none can add one.</p>
<button id="go" style="margin-top:4px">Create passkey</button><div id="msg"></div>
<p id="how" class="note" hidden>No signed link? On a device that holds your key, run <code>dd enrol</code> and open the address it prints.</p>
"#);
    out.push_str(ENROL_JS);
    out.push_str("</div></main></body></html>");
    out
}

/// An account, from nothing, in the browser: a name and a passkey.
pub fn join() -> String {
    let mut out = open("Create your account", AUTH_CSS, "");
    out.push_str(r#"<main><div class="card">
<h1>Create your account</h1><p class="lead">Pick a name and make a passkey. The passkey is the account: it stays on your device and follows you to your other devices the way passkeys do. No password, and nothing here can sign as you.</p>
<label for="u">Name</label>
<input id="u" autocomplete="username" autocapitalize="none" spellcheck="false" placeholder="lowercase letters, digits, - _ .">
<label for="c" style="margin-top:14px">Invite code <span style="font-weight:400;color:var(--ink-2)">(if someone gave you one)</span></label>
<input id="c" autocomplete="off" autocapitalize="none" spellcheck="false" placeholder="xxxxx-xxxxx">
<button id="go">Create passkey</button><div id="msg"></div>
<p class="note">Already have one? <a href="/_dd/login">Sign in</a>. Just looking? <a href="/_dd/demo">View the demo</a>.</p>
"#);
    out.push_str(INVITE_JS);
    out.push_str(JOIN_JS);
    out.push_str("</div></main></body></html>");
    out
}

const JOIN_JS: &str = r#"<script>
async function go(){const m=document.getElementById('msg');m.textContent='…';try{
const u=document.getElementById('u').value.trim().toLowerCase();
const code=document.getElementById('c').value.trim();
if(code)await checkInvite(code);
const r=await fetch('/_dd/join/start',{method:'POST',headers:{'content-type':'application/json'},body:JSON.stringify({username:u})});if(!r.ok)throw new Error(await r.text());
const {publicKey,ceremony}=await r.json();
publicKey.challenge=b64u(publicKey.challenge);publicKey.user.id=b64u(publicKey.user.id);
if(publicKey.excludeCredentials)publicKey.excludeCredentials=publicKey.excludeCredentials.map(c=>({...c,id:b64u(c.id)}));
publicKey.extensions={...(publicKey.extensions||{}),prf:{}};
const cred=await navigator.credentials.create({publicKey});
const body={id:cred.id,rawId:u8b64(cred.rawId),type:cred.type,extensions:cred.getClientExtensionResults(),response:{
attestationObject:u8b64(cred.response.attestationObject),clientDataJSON:u8b64(cred.response.clientDataJSON)}};
const hdr={'content-type':'application/json','x-dd-ceremony':ceremony};
if(code)hdr['x-dd-grant']=btoa(JSON.stringify(await claim(code,'webauthn:'+u8b64(cred.rawId))));
const f=await fetch('/_dd/join/finish',{method:'POST',headers:hdr,body:JSON.stringify(body)});
if(!f.ok)throw new Error(await f.text());const s=await f.json();
m.textContent='Once more, to sign your entry with it…';
const pk=s.publicKey;pk.challenge=b64u(pk.challenge);pk.allowCredentials=pk.allowCredentials.map(c=>({...c,id:b64u(c.id)}));
const a=await navigator.credentials.get({publicKey:pk});
const body2={id:a.id,rawId:u8b64(a.rawId),type:a.type,extensions:a.getClientExtensionResults(),response:{
authenticatorData:u8b64(a.response.authenticatorData),clientDataJSON:u8b64(a.response.clientDataJSON),
signature:u8b64(a.response.signature),userHandle:a.response.userHandle?u8b64(a.response.userHandle):null}};
const g=await fetch('/_dd/join/sign',{method:'POST',headers:{'content-type':'application/json','x-dd-ceremony':s.ceremony},body:JSON.stringify(body2)});
if(!g.ok)throw new Error(await g.text());try{localStorage.setItem('dd_user',u)}catch(e){}location.href='/_dd/home';}catch(e){m.textContent='Could not create the account: '+e.message}}
document.getElementById('go').onclick=go;
</script>"#;

/// The code as the browser holds it: a seed for an ed25519 key. Only the
/// public key and a signature ever leave.
const INVITE_JS: &str = r#"<script>
const enc=new TextEncoder();
const b64u=s=>Uint8Array.from(atob(s.replace(/-/g,'+').replace(/_/g,'/')),c=>c.charCodeAt(0));
const u8b64=a=>btoa(String.fromCharCode(...new Uint8Array(a))).replace(/\+/g,'-').replace(/\//g,'_').replace(/=+$/,'');
async function codeKey(code){const n=code.replace(/[^A-Za-z0-9]/g,'').toLowerCase();
const seed=new Uint8Array(await crypto.subtle.digest('SHA-256',enc.encode('dd-invite:'+n)));
const pkcs8=new Uint8Array([0x30,0x2e,0x02,0x01,0x00,0x30,0x05,0x06,0x03,0x2b,0x65,0x70,0x04,0x22,0x04,0x20,...seed]);
const k=await crypto.subtle.importKey('pkcs8',pkcs8,{name:'Ed25519'},true,['sign']);
const jwk=await crypto.subtle.exportKey('jwk',k);
return {k,pub:btoa(String.fromCharCode(...b64u(jwk.x)))};}
async function checkInvite(code){const {pub}=await codeKey(code);const r=await fetch('/_dd/invite/'+u8b64(enc.encode(pub)));
if(!r.ok)throw new Error('That code is not valid, or it has expired.');}
async function claim(code,root){const {k,pub}=await codeKey(code);const redeemed=Math.floor(Date.now()/1000);
const sig=new Uint8Array(await crypto.subtle.sign('Ed25519',k,enc.encode(JSON.stringify({invite:pub,root,redeemed}))));
return {invite_public_key:pub,redeemed,proof:btoa(String.fromCharCode(...sig))};}
</script>"#;

const LOGIN_JS: &str = r#"<script>
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
</script>"#;

const ENROL_JS: &str = r#"<script>
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
publicKey.extensions={...(publicKey.extensions||{}),prf:{}};
const cred=await navigator.credentials.create({publicKey});
const body={id:cred.id,rawId:u8b64(cred.rawId),type:cred.type,extensions:cred.getClientExtensionResults(),response:{
attestationObject:u8b64(cred.response.attestationObject),clientDataJSON:u8b64(cred.response.clientDataJSON)}};
const f=await fetch('/_dd/enrol/finish',{method:'POST',headers:{'content-type':'application/json','x-dd-ceremony':ceremony,...hdr},body:JSON.stringify(body)});
if(!f.ok)throw new Error(await f.text());const {id,user}=await f.json();
m.textContent='Passkey made. Waiting for the terminal to sign it into your entry…';
for(let i=0;i<90;i++){await new Promise(r=>setTimeout(r,2000));try{const e=await fetch('/_dd/directory/'+encodeURIComponent(user));if(e.ok){const j=await e.json();if((j.entry.passkeys||[]).some(p=>p.id===id)){m.textContent='Signed in to your entry. Taking you to sign in…';location.href='/_dd/login?rd='+encodeURIComponent(rd);return}}}catch(e){}}
m.textContent='Passkey made, but it has not appeared in your entry yet. Once the terminal reports it published, sign in.';}catch(e){m.textContent='Failed: '+e.message}}
document.getElementById('go').onclick=go;
</script>"#;

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
    /// where the demo visitor goes for this tile; None: the tile is not in
    /// the demo
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub demo: Option<String>,
}

/// The account that needs no invite and no key: a look at what a member
/// sees, with nothing of their own and nothing kept.
pub const DEMO_USER: &str = "demo";

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
        "games" => {
            r#"<rect x="2.5" y="6.5" width="15" height="8.5" rx="4"/><path d="M6.5 9v3.5M4.75 10.75h3.5"/><circle cx="12.5" cy="10" r=".9" fill="currentColor" stroke="none"/><circle cx="14.6" cy="12" r=".9" fill="currentColor" stroke="none"/>"#
        }
        _ => r#"<rect x="3" y="3" width="14" height="14" rx="3"/>"#,
    }
}

fn me_bar(user: &str) -> String {
    let initial = user
        .chars()
        .next()
        .map(|c| esc(&c.to_string()))
        .unwrap_or_default();
    format!(
        r#"<div class="me"><span class="avatar" aria-hidden="true">{initial}</span><span>{}</span><a class="out" href="/_dd/logout">Sign out</a></div>"#,
        esc(user)
    )
}

/// Signed in but not on the member list: the account exists, nothing is
/// open to it yet. A code from the owner opens it here.
pub fn waiting(user: &str) -> String {
    let mut out = open("Distributed Datacenter", AUTH_CSS, &me_bar(user));
    out.push_str(&format!(
        r#"<main><div class="card" data-user="{}">
<h1>Your account is made</h1><p class="lead">Nothing here is open to you yet. Whoever runs this network adds people to it; once they have added you, this page fills in with what you can use.</p>
<label for="c">Have a code from them?</label>
<input id="c" autocomplete="off" autocapitalize="none" spellcheck="false" placeholder="xxxxx-xxxxx">
<button id="go">Use code</button><div id="msg"></div>
</div></main>"#,
        esc(user)
    ));
    out.push_str(INVITE_JS);
    out.push_str(REDEEM_JS);
    out.push_str("</body></html>");
    out
}

const REDEEM_JS: &str = r#"<script>
async function go(){const m=document.getElementById('msg');m.textContent='…';try{
const code=document.getElementById('c').value.trim();if(!code)throw new Error('type the code');
await checkInvite(code);
const user=document.querySelector('.card').dataset.user;
const e=await fetch('/_dd/directory/'+encodeURIComponent(user));if(!e.ok)throw new Error('no entry');
const root=(await e.json()).entry.root;
const r=await fetch('/_dd/redeem/start',{method:'POST',headers:{'x-dd-grant':btoa(JSON.stringify(await claim(code,root)))}});
if(!r.ok)throw new Error(await r.text());const s=await r.json();
const pk=s.publicKey;pk.challenge=b64u(pk.challenge);pk.allowCredentials=pk.allowCredentials.map(c=>({...c,id:b64u(c.id)}));
m.textContent='Confirm with your passkey…';
const a=await navigator.credentials.get({publicKey:pk});
const body={id:a.id,rawId:u8b64(a.rawId),type:a.type,extensions:a.getClientExtensionResults(),response:{
authenticatorData:u8b64(a.response.authenticatorData),clientDataJSON:u8b64(a.response.clientDataJSON),
signature:u8b64(a.response.signature),userHandle:a.response.userHandle?u8b64(a.response.userHandle):null}};
const g=await fetch('/_dd/redeem/sign',{method:'POST',headers:{'content-type':'application/json','x-dd-ceremony':s.ceremony},body:JSON.stringify(body)});
if(!g.ok)throw new Error(await g.text());location.href='/_dd/home';}catch(e){m.textContent='That did not work: '+e.message}}
document.getElementById('go').onclick=go;
</script>"#;

/// Photos: a person's ente account, opened by their passkey. Nothing to
/// type; the page asks the passkey for its PRF secret, and our Rust in the
/// browser makes or opens the account with it and hands ente's app a
/// signed-in session. The master key never leaves this tab.
pub fn photos(user: &str) -> String {
    let mut out = open("Photos", AUTH_CSS, &me_bar(user));
    out.push_str(&format!(
        r#"<main><div class="card" data-user="{}">
<h1>Photos</h1><p class="lead">Unlocking with your passkey.</p>
<div id="msg">…</div>
<div id="link" hidden>
<p class="lead" style="margin-top:18px">No photo account for this passkey yet. Made one here before, with an email and a password? Sign in with it once and it becomes this passkey's. Otherwise start fresh.</p>
<label for="le">Email</label><input id="le" autocomplete="email" autocapitalize="none" spellcheck="false">
<label for="lp" style="margin-top:14px">Password</label><input id="lp" type="password" autocomplete="current-password">
<button id="adopt">Link that account</button>
<button id="fresh" style="margin-top:10px;background:var(--bg-2);color:var(--ink)">Start fresh</button>
</div>
<p class="note">Your photos are encrypted with a key only your passkey can make. No box holds it; nobody here can look.</p>
</div></main>"#,
        esc(user)
    ));
    out.push_str(PHOTOS_JS);
    out.push_str("</body></html>");
    out
}

const PHOTOS_JS: &str = r#"<script type="module">
import init, { ente_login, ente_create, ente_adopt } from '/_dd/web/dd_web.js';
const m=document.getElementById('msg');
const b64u=s=>Uint8Array.from(atob(s.replace(/-/g,'+').replace(/_/g,'/')),c=>c.charCodeAt(0));
const u8b64=a=>btoa(String.fromCharCode(...new Uint8Array(a))).replace(/\+/g,'-').replace(/\//g,'_').replace(/=+$/,'');
async function go(){try{
const user=document.querySelector('.card').dataset.user;
const c=await fetch('/_dd/photos/config',{method:'POST'});if(!c.ok)throw new Error('photos is not on this box');const cfg=await c.json();
const e=await fetch('/_dd/directory/'+encodeURIComponent(user));if(!e.ok)throw new Error('no entry');
const allow=((await e.json()).entry.passkeys||[]).map(p=>({type:'public-key',id:b64u(p.id)}));
if(!allow.length)throw new Error('this account has no passkey in a browser yet: dd enrol adds one');
const salt=new Uint8Array(await crypto.subtle.digest('SHA-256',new TextEncoder().encode('dd-photos')));
const a=await navigator.credentials.get({publicKey:{challenge:crypto.getRandomValues(new Uint8Array(32)),rpId:cfg.rpId,allowCredentials:allow,userVerification:'preferred',extensions:{prf:{eval:{first:salt}}}}});
const prf=a.getClientExtensionResults().prf;const secret=prf&&prf.results&&prf.results.first;
if(!secret)throw new Error('this passkey cannot make the photos key on this browser; try your phone');
const password=u8b64(secret);
await init();
m.textContent='Opening your photos…';
let s;
try{s=JSON.parse(await ente_login(cfg.api,cfg.email,password));}
catch(err){if(!/404|not found|user not/i.test(String(err)))throw err;
// no account under this passkey yet: link one made before, or start fresh
s=await new Promise((res,rej)=>{const l=document.getElementById('link');l.hidden=false;m.textContent='';
document.getElementById('adopt').onclick=async()=>{try{m.textContent='Linking…';l.hidden=true;
res(JSON.parse(await ente_adopt(cfg.api,document.getElementById('le').value.trim(),document.getElementById('lp').value,cfg.email,password,cfg.code)));}catch(e){rej(e)}};
document.getElementById('fresh').onclick=async()=>{try{m.textContent='Making your photo account…';l.hidden=true;
res(JSON.parse(await ente_create(cfg.api,cfg.email,password,cfg.code)));}catch(e){rej(e)}};});}
await seed(s);
}catch(err){m.textContent='Could not open Photos: '+(err&&err.message||err);}}
async function seed(s){
localStorage.setItem('user',JSON.stringify({id:s.userId,email:s.email,token:s.token}));
localStorage.setItem('keyAttributes',JSON.stringify(s.keyAttributes));
sessionStorage.setItem('encryptionKey',JSON.stringify(s.sessionKey));
await new Promise((res,rej)=>{const r=indexedDB.open('kv',1);r.onupgradeneeded=()=>{r.result.createObjectStore('kv')};r.onerror=()=>rej(r.error);
r.onsuccess=()=>{const tx=r.result.transaction('kv','readwrite');tx.objectStore('kv').put(s.token,'token');tx.oncomplete=()=>{r.result.close();res()};tx.onerror=()=>rej(tx.error)}});
location.replace('/');}
go();
</script>"#;

/// The signed-in home page: the services this box offers, as tiles. Server
/// rendered, so the browser runs nothing.
pub fn home(user: &str, services: &[Service]) -> String {
    let demo = user == DEMO_USER;
    let mut out = open("Distributed Datacenter", HOME_CSS, &me_bar(user));
    if demo {
        out.push_str(r#"<main><div class="banner"><strong>This is a demo.</strong> You are seeing what a member sees, without the keys: nothing here is yours, nothing you do is kept, and some doors stay shut. To join, someone who is in gives you a code.</div><h1>The services</h1>"#);
    } else {
        out.push_str("<main><h1>Your services</h1>");
    }
    let shown: Vec<&Service> = services
        .iter()
        .filter(|s| !demo || s.demo.is_some())
        .collect();
    if shown.is_empty() {
        out.push_str(r#"<p class="empty">Nothing runs here yet.</p>"#);
    } else {
        out.push_str(r#"<ul class="grid">"#);
        for s in shown {
            let url = if demo {
                s.demo.as_deref().unwrap_or(&s.url)
            } else {
                &s.url
            };
            out.push_str(&format!(
                r#"<li><a class="tile" href="{}"><span class="icon" style="background:{}" aria-hidden="true"><svg viewBox="0 0 20 20" fill="none" stroke="currentColor" stroke-width="1.6">{}</svg></span><span><h2>{}</h2><p>{}</p></span></a></li>"#,
                esc(url),
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
            demo: None,
        }
    }

    #[test]
    fn auth_pages_are_one_shell_one_script() {
        for page in [login(), enrol(), join()] {
            assert!(page.starts_with("<!doctype html>"));
            assert!(page.ends_with("</html>"));
            assert_eq!(
                page.matches("<script>").count(),
                if page.contains("/_dd/join/sign") {
                    2
                } else {
                    1
                }
            );
            assert_eq!(page.matches("</header>").count(), 1);
            assert!(page.contains("--brand:#124e63"));
        }
        assert!(login().contains("dd enrol"));
        assert!(enrol().contains("dd enrol"));
        assert!(login().contains("/_dd/login/start"));
        assert!(login().contains("href=\"/_dd/join\""));
        assert!(enrol().contains("/_dd/enrol/start"));
        assert!(join().contains("/_dd/join/sign"));
    }

    #[test]
    fn waiting_page_names_the_person_and_offers_nothing() {
        let html = waiting("tom");
        assert!(html.contains("<span>tom</span>"));
        assert!(html.contains("data-user=\"tom\""));
        assert!(html.contains("/_dd/logout"));
        assert!(!html.contains("class=\"tile\""));
        assert!(html.contains("/_dd/redeem/start"));
    }

    #[test]
    fn the_demo_sees_only_demo_tiles_and_a_banner() {
        let a = Service {
            name: "Files".into(),
            url: "https://files.x/".into(),
            description: "d".into(),
            icon: "files".into(),
            color: "#000".into(),
            demo: Some("https://files.x/demo/".into()),
        };
        let b = Service {
            name: "Chat".into(),
            url: "https://llm.x/".into(),
            description: "d".into(),
            icon: "chat".into(),
            color: "#000".into(),
            demo: None,
        };
        let html = home(DEMO_USER, &[a.clone(), b.clone()]);
        assert!(html.contains("This is a demo"));
        assert!(html.contains("https://files.x/demo/"));
        assert!(!html.contains("Chat"));
        let html = home("tom", &[a, b]);
        assert!(!html.contains("This is a demo"));
        assert!(html.contains("https://files.x/\"") && html.contains("Chat"));
    }

    #[test]
    fn photos_page_asks_the_passkey_and_runs_our_wasm() {
        let html = photos("tom");
        assert!(html.contains("data-user=\"tom\""));
        assert!(html.contains("/_dd/web/dd_web.js"));
        assert!(html.contains("prf"));
        assert!(html.contains("/_dd/photos/config"));
    }

    #[test]
    fn dump_pages_for_a_look() {
        if let Ok(dir) = std::env::var("DD_DUMP_PAGES") {
            std::fs::write(format!("{dir}/login.html"), login()).unwrap();
            std::fs::write(format!("{dir}/enrol.html"), enrol()).unwrap();
            std::fs::write(format!("{dir}/join.html"), join()).unwrap();
            std::fs::write(format!("{dir}/waiting.html"), waiting("tom")).unwrap();
            std::fs::write(format!("{dir}/photos.html"), photos("tom")).unwrap();
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
