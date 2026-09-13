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
