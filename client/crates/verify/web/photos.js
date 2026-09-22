// Photos: a person's ente account, opened by their passkey. The page asks
// the passkey for its PRF secret; our Rust in the browser (dd_web) makes
// or opens the ente account with it and hands ente's app a signed-in
// session. The master key never leaves this tab. The demo has no passkey:
// its password comes with the config, for that session only.

import init, { ente_login, ente_create, ente_adopt } from '/_dd/web/dd_web.js';
import { b64u, u8b64, say } from './webauthn.js';

async function passkeySecret(user, cfg) {
  const e = await fetch('/_dd/directory/' + encodeURIComponent(user));
  if (!e.ok) throw new Error('no entry');
  const allow = ((await e.json()).entry.passkeys || []).map(p => ({ type: 'public-key', id: b64u(p.id) }));
  if (!allow.length) throw new Error('this account has no passkey in a browser yet: dd enrol adds one');

  const salt = new Uint8Array(await crypto.subtle.digest('SHA-256', new TextEncoder().encode('dd-photos')));
  const a = await navigator.credentials.get({
    publicKey: {
      challenge: crypto.getRandomValues(new Uint8Array(32)),
      rpId: cfg.rpId,
      allowCredentials: allow,
      userVerification: 'preferred',
      extensions: { prf: { eval: { first: salt } } },
    },
  });
  const prf = a.getClientExtensionResults().prf;
  const secret = prf && prf.results && prf.results.first;
  if (!secret) throw new Error('this passkey cannot make the photos key on this browser; try your phone');
  return u8b64(secret);
}

// no account under this passkey yet: link one made before, or start fresh
function askToLink(cfg, password) {
  return new Promise((resolve, reject) => {
    const panel = document.getElementById('link');
    panel.hidden = false;
    say('');
    document.getElementById('adopt').onclick = async () => {
      try {
        say('Linking…');
        panel.hidden = true;
        const email = document.getElementById('le').value.trim();
        const old = document.getElementById('lp').value;
        resolve(JSON.parse(await ente_adopt(cfg.api, email, old, cfg.email, password, cfg.code)));
      } catch (e) { reject(e); }
    };
    document.getElementById('fresh').onclick = async () => {
      try {
        say('Making your photo account…');
        panel.hidden = true;
        resolve(JSON.parse(await ente_create(cfg.api, cfg.email, password, cfg.code)));
      } catch (e) { reject(e); }
    };
  });
}

// what ente's web app reads to be signed in, in the shapes it keeps
async function seed(s) {
  localStorage.setItem('user', JSON.stringify({ id: s.userId, email: s.email, token: s.token }));
  localStorage.setItem('keyAttributes', JSON.stringify(s.keyAttributes));
  sessionStorage.setItem('encryptionKey', JSON.stringify(s.sessionKey));
  await new Promise((resolve, reject) => {
    const r = indexedDB.open('kv', 1);
    r.onupgradeneeded = () => { r.result.createObjectStore('kv'); };
    r.onerror = () => reject(r.error);
    r.onsuccess = () => {
      const tx = r.result.transaction('kv', 'readwrite');
      tx.objectStore('kv').put(s.token, 'token');
      tx.oncomplete = () => { r.result.close(); resolve(); };
      tx.onerror = () => reject(tx.error);
    };
  });
  location.replace('/');
}

async function go() {
  try {
    const user = document.querySelector('.card').dataset.user;
    const c = await fetch('/_dd/photos/config', { method: 'POST' });
    if (!c.ok) throw new Error('photos is not on this box');
    const cfg = await c.json();

    const password = cfg.password || await passkeySecret(user, cfg);
    await init();
    say('Opening your photos…');

    let session;
    try {
      session = JSON.parse(await ente_login(cfg.api, cfg.email, password));
    } catch (err) {
      if (!/404|not found|user not/i.test(String(err))) throw err;
      session = await askToLink(cfg, password);
    }
    await seed(session);
  } catch (err) {
    say('Could not open Photos: ' + ((err && err.message) || err));
  }
}

go();
