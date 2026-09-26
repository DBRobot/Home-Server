// A member's library in a browser tab: the passkey makes the key, the
// gate hands over ciphertext on WebDAV, and every name and every byte is
// turned over here. Files and Movies & TV are two corners of the same
// library, so the opening, listing and carrying live here once.

import init, { library_device_key, library_open, library_key_for_box, path_encrypt, path_decrypt, file_open, file_seal, plain_size } from '/_dd/web/dd_web.js';
import { b64u, u8b64 } from './webauthn.js';

// the passkey's own secret, under a label of this page's own
async function passkeySecret(cfg, passkeys) {
  const allow = passkeys.map((p) => ({ type: 'public-key', id: b64u(p.id) }));
  if (!allow.length) throw new Error('this account has no passkey in a browser yet: `dd enrol` adds one');
  const salt = new Uint8Array(await crypto.subtle.digest('SHA-256', new TextEncoder().encode('dd-library')));
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
  if (!secret) throw new Error('this passkey cannot make a key on this browser; try another');
  const raw = new Uint8Array(a.rawId);
  const id = btoa(String.fromCharCode(...raw)).replace(/\+/g, '-').replace(/\//g, '_').replace(/=+$/, '');
  return { secret: u8b64(secret), id };
}

/// The first library this browser's passkey opens. Returns
/// `{ ok: lib }`, `{ none: true }` when the account has no library, or
/// `{ link: 'dd passkey link …' }` when nothing here is sealed to it.
/// A library comes with the role this viewer has in it: a reader looks
/// and does not touch.
export async function unlock(user) {
  await init();
  const cfg = await (await fetch('/_dd/config')).json();
  // the demo holds no key of its own; the box hands it one (verify.nix)
  if (cfg.demoLibrary) {
    return { ok: { id: cfg.demoLibrary.id, key: cfg.demoLibrary.key, reader: true } };
  }
  const text = await (await fetch('/_dd/directory/' + encodeURIComponent(user))).text();
  const entry = JSON.parse(text);
  const { secret, id } = await passkeySecret(cfg, entry.entry.passkeys || []);
  const libraries = entry.entry.libraries || [];
  if (!libraries.length) return { none: true };
  for (const l of libraries) {
    try {
      return { ok: { id: l.id, key: library_open(text, l.id, secret) } };
    } catch { /* the next one, or none */ }
  }
  return { link: `dd passkey link ${id} ${library_device_key(secret)}` };
}

/// the gate's url for a path in the library, under its encrypted name
export function dav(lib, path) {
  const enc = path ? path_encrypt(lib.key, lib.id, path).split('/').map(encodeURIComponent).join('/') : '';
  return `/_dd/dav/${lib.id}/${enc}`;
}

/// what a folder holds, names and sizes as they really are
export async function list(lib, dir) {
  const r = await fetch(dav(lib, dir) + '/', { method: 'PROPFIND', headers: { depth: '1' } });
  if (r.status === 404) return [];
  if (r.status !== 207) throw new Error(`the gate said ${r.status}`);
  const doc = new DOMParser().parseFromString(await r.text(), 'application/xml');
  const prefix = `/_dd/dav/${lib.id}/`;
  const out = [];
  for (const el of doc.getElementsByTagNameNS('DAV:', 'response')) {
    const href = el.getElementsByTagNameNS('DAV:', 'href')[0]?.textContent || '';
    const rel = decodeURI(href).slice(prefix.length).replace(/\/$/, '');
    if (!rel) continue;
    let path;
    try { path = path_decrypt(lib.key, lib.id, rel.split('/').map(decodeURIComponent).join('/')); }
    catch { continue; }                       // not ours to read: the trash, a stray
    if (path === dir) continue;               // the folder lists itself first
    const isDir = !!el.getElementsByTagNameNS('DAV:', 'collection').length;
    const sealed = Number(el.getElementsByTagNameNS('DAV:', 'getcontentlength')[0]?.textContent || 0);
    out.push({
      path,
      name: path.split('/').pop(),
      dir: isDir,
      size: isDir ? 0 : plain_size(sealed),
      sealed,
      modified: el.getElementsByTagNameNS('DAV:', 'getlastmodified')[0]?.textContent || '',
    });
  }
  out.sort((a, b) => (a.dir === b.dir ? a.name.localeCompare(b.name) : a.dir ? -1 : 1));
  return out;
}

/// a file out of the library and into the machine's downloads
export async function fetchPlain(lib, path) {
  const r = await fetch(dav(lib, path));
  if (!r.ok) throw new Error(`the gate said ${r.status}`);
  return file_open(lib.key, lib.id, new Uint8Array(await r.arrayBuffer()));
}

export function save(bytes, name) {
  const url = URL.createObjectURL(new Blob([bytes]));
  const a = document.createElement('a');
  a.href = url;
  a.download = name;
  a.click();
  URL.revokeObjectURL(url);
}

/// a file into the library, sealed here
export async function put(lib, path, file) {
  const sealed = file_seal(lib.key, lib.id, new Uint8Array(await file.arrayBuffer()));
  const r = await fetch(dav(lib, path), { method: 'PUT', body: sealed });
  if (!r.ok) throw new Error(`the gate said ${r.status}`);
}

export async function trash(lib, path) {
  await fetch(dav(lib, path), { method: 'DELETE' });
}

/// A film this tab cannot open: the box does the work instead. It is
/// given a url it can fetch ranges from while the film plays and the
/// library's data key sealed to a key it made when it started, so the
/// plaintext exists in its memory for this one file and nowhere else.
/// What comes back is a playlist on the same host as this page.
export async function transcode(lib, path, sealedSize) {
  const pre = await fetch(dav(lib, path), { headers: { 'x-dd-presign': '1' } });
  if (!pre.ok) throw new Error(`the gate said ${pre.status}`);
  const { url } = await pre.json();
  const box = await fetch('/_dd/transcode/key');
  if (!box.ok) throw new Error('this box does not transcode');
  const { key } = await box.json();
  const r = await fetch('/_dd/transcode/start', {
    method: 'POST',
    headers: { 'content-type': 'application/json' },
    body: JSON.stringify({ url, key: library_key_for_box(lib.key, lib.id, key), size: sealedSize }),
  });
  if (!r.ok) throw new Error(`the box said ${r.status}: ${await r.text()}`);
  const { playlist } = await r.json();
  return `/_dd/transcode${playlist}`;
}

/// Tell the box it can stop. Without this the ffmpeg behind a film the
/// viewer closed after a minute keeps going until the session times out
/// ten minutes later, which on a box with one job at a time is the
/// difference between the next film starting now and starting then.
export async function stopTranscode(url, leaving) {
  const at = url.replace(/\/index\.m3u8$/, '');
  try {
    // keepalive so it still goes when the tab is on its way out; a beacon
    // would not, since that is a POST and this route takes DELETE
    await fetch(at, { method: 'DELETE', keepalive: !!leaving });
  } catch { /* it times out by itself; this is only sooner */ }
}

/// whether this browser plays a playlist by itself (Safari, every iPhone)
export function playsPlaylists() {
  const v = document.createElement('video');
  return !!(v.canPlayType('application/vnd.apple.mpegurl') || v.canPlayType('application/x-mpegURL'));
}

/// Where it does not, a player from the box (flake.nix pins it beside our
/// own wasm). Loaded the first time a film is opened and not before: it is
/// half a megabyte and most visits never play anything.
let player = null;
export async function playlistPlayer() {
  if (!player) {
    // a classic script, not an import: the file is a UMD bundle and its
    // wrapper wants `this` to be the window, which a module denies it
    await new Promise((ok, no) => {
      const s = document.createElement('script');
      s.src = '/_dd/web/hls.js';
      s.onload = ok;
      s.onerror = () => no(new Error('the player did not load'));
      document.head.append(s);
    });
    player = window.Hls;
  }
  if (!player || !player.isSupported()) throw new Error('this browser cannot play a film');
  return player;
}

export function human(n) {
  const u = ['B', 'KB', 'MB', 'GB', 'TB'];
  let i = 0;
  while (n >= 1024 && i < u.length - 1) { n /= 1024; i++; }
  return `${n < 10 && i ? n.toFixed(1) : Math.round(n)} ${u[i]}`;
}

export async function mkdir(lib, path) {
  const r = await fetch(dav(lib, path), { method: 'MKCOL' });
  if (!r.ok && r.status !== 405) throw new Error(`the gate said ${r.status}`);
}
