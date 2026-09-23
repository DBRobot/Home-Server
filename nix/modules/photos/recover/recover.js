// The photos password is the passkey's PRF output for a fixed salt
// (box/verify/web/photos.js: passkeySecret). The same call, on the old
// origin, with whichever passkey the browser still holds for it.
const u8b64 = a =>
  btoa(String.fromCharCode(...new Uint8Array(a)))
    .replace(/\+/g, '-').replace(/\//g, '_').replace(/=+$/, '');

const say = t => { document.getElementById('say').textContent = t; };

document.getElementById('go').onclick = async () => {
  try {
    say('Waiting for the passkey…');
    const salt = new Uint8Array(await crypto.subtle.digest('SHA-256', new TextEncoder().encode('dd-photos')));
    const a = await navigator.credentials.get({
      publicKey: {
        challenge: crypto.getRandomValues(new Uint8Array(32)),
        rpId: location.hostname.split('.').slice(1).join('.'),
        userVerification: 'preferred',
        extensions: { prf: { eval: { first: salt } } },
      },
    });
    const prf = a.getClientExtensionResults().prf;
    const secret = prf && prf.results && prf.results.first;
    if (!secret) throw new Error('this passkey gave no key on this browser; try the one that opened Photos before');
    const out = document.getElementById('out');
    out.textContent = u8b64(secret);
    out.hidden = false;
    say('This is the old password. Copy it, then close this page.');
  } catch (e) {
    say('Could not: ' + (e.message || e));
  }
};
