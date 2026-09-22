// Add a passkey to an entry the terminal made: the link carries a token;
// the terminal signs the new passkey into the entry once it appears.

import { creationOptions, attestation, post, say } from './webauthn.js';

const q = new URLSearchParams(location.search);
const rd = q.get('rd') || '/';
const token = q.get('t');
const auth = token ? { authorization: 'Bearer ' + token } : {};

async function go() {
  say('…');
  try {
    const r = await fetch('/_dd/enrol/start', { method: 'POST', headers: auth });
    if (r.status === 401) {
      say('This link is not valid, or has expired.');
      document.getElementById('how').hidden = false;
      return;
    }
    if (!r.ok) throw new Error(await r.text());
    const { publicKey, ceremony } = await r.json();
    const cred = await navigator.credentials.create({ publicKey: creationOptions(publicKey) });
    const finish = await post('/_dd/enrol/finish', attestation(cred), { 'x-dd-ceremony': ceremony, ...auth });
    const { id, user } = await finish.json();

    say('Passkey made. Waiting for the terminal to sign it into your entry…');
    for (let i = 0; i < 90; i++) {
      await new Promise(res => setTimeout(res, 2000));
      try {
        const e = await fetch('/_dd/directory/' + encodeURIComponent(user));
        if (!e.ok) continue;
        const entry = (await e.json()).entry;
        if ((entry.passkeys || []).some(p => p.id === id)) {
          say('Signed in to your entry. Taking you to sign in…');
          location.href = '/_dd/login?rd=' + encodeURIComponent(rd);
          return;
        }
      } catch (e) {}
    }
    say('Passkey made, but it has not appeared in your entry yet. Once the terminal reports it published, sign in.');
  } catch (e) {
    say('Failed: ' + e.message);
  }
}

document.getElementById('go').onclick = go;
