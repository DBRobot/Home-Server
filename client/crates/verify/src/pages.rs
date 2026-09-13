//! The two pages a browser sees. Plain html and the WebAuthn calls; nothing
//! is loaded from anywhere.

pub const LOGIN: &str = r#"<!doctype html><html lang="en"><head><meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1"><title>Sign in</title>
<style>:root{color-scheme:light dark}body{font:16px/1.5 system-ui,sans-serif;margin:0;padding:2rem 1rem;display:flex;justify-content:center}
main{width:100%;max-width:24rem}h1{font-size:1.4rem;margin:0 0 .5rem}p{opacity:.75}button{width:100%;padding:.7rem;font:inherit;font-weight:600;border:0;border-radius:6px;cursor:pointer;background:currentColor;margin-top:1rem}
button span{color:Canvas}#msg{margin-top:1rem;min-height:1.5em}a{color:inherit}</style></head><body><main>
<h1>Sign in</h1><p>With the passkey you set up for this network. Nothing leaves your device but a signature.</p>
<label for="u" style="display:block;font-weight:600;margin-top:1rem">Username</label>
<input id="u" autocomplete="username webauthn" autocapitalize="none" spellcheck="false" style="width:100%;box-sizing:border-box;padding:.6rem .7rem;font:inherit;border:1px solid currentColor;border-radius:6px;background:transparent;color:inherit">
<button id="go"><span>Use passkey</span></button><div id="msg"></div>
<p style="font-size:.9rem">First time in a browser? <a id="enrol" href="/_dd/enrol">Set up a passkey</a>.</p>
<script>
const rd=new URLSearchParams(location.search).get('rd')||'/';
document.getElementById('enrol').href='/_dd/enrol?rd='+encodeURIComponent(rd);
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
<h1>Set up a passkey</h1><p>For signing in to this network from a browser. It is checked here, on this box, against a key that only your device holds.</p>
<button id="go"><span>Create passkey</span></button><div id="msg"></div>
<script>
const rd=new URLSearchParams(location.search).get('rd')||'/';
const b64u=s=>Uint8Array.from(atob(s.replace(/-/g,'+').replace(/_/g,'/')),c=>c.charCodeAt(0));
const u8b64=a=>btoa(String.fromCharCode(...new Uint8Array(a))).replace(/\+/g,'-').replace(/\//g,'_').replace(/=+$/,'');
async function go(){const m=document.getElementById('msg');m.textContent='…';try{
const r=await fetch('/_dd/enrol/start',{method:'POST'});
if(r.status===401){location.href='/oauth2/start?rd='+encodeURIComponent(location.pathname+location.search);return}
if(!r.ok)throw new Error(await r.text());
const {publicKey,ceremony}=await r.json();
publicKey.challenge=b64u(publicKey.challenge);publicKey.user.id=b64u(publicKey.user.id);
if(publicKey.excludeCredentials)publicKey.excludeCredentials=publicKey.excludeCredentials.map(c=>({...c,id:b64u(c.id)}));
const cred=await navigator.credentials.create({publicKey});
const body={id:cred.id,rawId:u8b64(cred.rawId),type:cred.type,extensions:cred.getClientExtensionResults(),response:{
attestationObject:u8b64(cred.response.attestationObject),clientDataJSON:u8b64(cred.response.clientDataJSON)}};
const f=await fetch('/_dd/enrol/finish',{method:'POST',headers:{'content-type':'application/json','x-dd-ceremony':ceremony},body:JSON.stringify(body)});
if(!f.ok)throw new Error(await f.text());m.textContent='Done. Signing you in…';location.href=rd;}catch(e){m.textContent='Failed: '+e.message}}
document.getElementById('go').onclick=go;
</script></main></body></html>"#;
