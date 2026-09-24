// Files: a member's library in the browser. The passkey makes the key the
// library was sealed to, the page asks the gate for ciphertext over
// WebDAV, and every name and every byte is turned over here. The box is
// storage; it never sees a name or a file.

import init, { library_device_key, library_open, path_encrypt, path_decrypt, file_open, file_seal, plain_size } from '/_dd/web/dd_web.js';
import { b64u, u8b64, say } from './webauthn.js';

const $ = (id) => document.getElementById(id);
const user = document.querySelector('[data-user]').dataset.user;
const ROOT = 'Files';            // this page shows one corner of the library
let lib = null;                  // { id, key }
let here = '';                   // the folder under ROOT, plain

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
  return { secret: u8b64(secret), id: b64u_of(a.rawId) };
}

function b64u_of(buf) {
  return btoa(String.fromCharCode(...new Uint8Array(buf))).replace(/\+/g, '-').replace(/\//g, '_').replace(/=+$/, '');
}

// the gate, over WebDAV, under this library's prefix
function dav(path) {
  const enc = path ? path_encrypt(lib.key, lib.id, path).split('/').map(encodeURIComponent).join('/') : '';
  return `/_dd/dav/${lib.id}/${enc}`;
}

async function list(dir) {
  const r = await fetch(dav(dir) + '/', { method: 'PROPFIND', headers: { depth: '1' } });
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
      modified: el.getElementsByTagNameNS('DAV:', 'getlastmodified')[0]?.textContent || '',
    });
  }
  out.sort((a, b) => (a.dir === b.dir ? a.name.localeCompare(b.name) : a.dir ? -1 : 1));
  return out;
}

function human(n) {
  const u = ['B', 'KB', 'MB', 'GB', 'TB'];
  let i = 0;
  while (n >= 1024 && i < u.length - 1) { n /= 1024; i++; }
  return `${n < 10 && i ? n.toFixed(1) : Math.round(n)} ${u[i]}`;
}

function crumbs() {
  const el = $('crumbs');
  const parts = here ? here.split('/') : [];
  el.replaceChildren();
  const add = (label, to) => {
    const a = document.createElement('button');
    a.className = 'crumb';
    a.textContent = label;
    a.onclick = () => show(to);
    el.append(a);
  };
  add('Files', '');
  parts.forEach((p, i) => {
    el.append(document.createTextNode('/'));
    add(p, parts.slice(0, i + 1).join('/'));
  });
  el.hidden = false;
}

async function show(dir) {
  here = dir;
  crumbs();
  const items = await list(`${ROOT}${dir ? '/' + dir : ''}`);
  const ul = $('list');
  ul.replaceChildren(...items.map((it) => {
    const li = document.createElement('li');
    const name = document.createElement('button');
    name.className = 'rowname';
    name.textContent = (it.dir ? '📁 ' : '') + it.name;
    name.onclick = () => (it.dir ? show(here ? `${here}/${it.name}` : it.name) : download(it));
    const meta = document.createElement('span');
    meta.className = 'tag';
    meta.textContent = it.dir ? '' : human(it.size);
    li.append(name, meta);
    if (!it.dir) {
      const rm = document.createElement('button');
      rm.className = 'quiet inline';
      rm.textContent = 'Trash';
      rm.onclick = async () => {
        rm.disabled = true;
        await fetch(dav(it.path), { method: 'DELETE' });
        show(here);
      };
      li.append(rm);
    }
    return li;
  }));
  ul.hidden = false;
  $('empty').hidden = items.length > 0;
  $('up').hidden = false;
}

async function download(it) {
  const job = addJob(`${it.name} — fetching`);
  try {
    const r = await fetch(dav(it.path));
    if (!r.ok) throw new Error(`the gate said ${r.status}`);
    const plain = file_open(lib.key, lib.id, new Uint8Array(await r.arrayBuffer()));
    const url = URL.createObjectURL(new Blob([plain]));
    const a = document.createElement('a');
    a.href = url;
    a.download = it.name;
    a.click();
    URL.revokeObjectURL(url);
    job.textContent = `${it.name} — saved`;
  } catch (e) {
    job.textContent = `${it.name} — ${e.message}`;
  }
}

function addJob(text) {
  const li = document.createElement('li');
  li.textContent = text;
  $('jobs').append(li);
  $('jobs').hidden = false;
  return li;
}

async function upload(files) {
  for (const f of files) {
    const job = addJob(`${f.name} — encrypting`);
    try {
      const sealed = file_seal(lib.key, lib.id, new Uint8Array(await f.arrayBuffer()));
      job.textContent = `${f.name} — uploading`;
      const path = `${ROOT}${here ? '/' + here : ''}/${f.name}`;
      const r = await fetch(dav(path), { method: 'PUT', body: sealed });
      if (!r.ok) throw new Error(`the gate said ${r.status}`);
      job.textContent = `${f.name} — ${human(f.size)}`;
    } catch (e) {
      job.textContent = `${f.name} — ${e.message}`;
    }
  }
  show(here);
}

async function start() {
  await init();
  const cfg = await (await fetch('/_dd/config')).json();
  const entryText = await (await fetch('/_dd/directory/' + encodeURIComponent(user))).text();
  const entry = JSON.parse(entryText);
  const { secret, id: passkeyId } = await passkeySecret(cfg, entry.entry.passkeys || []);
  const libraries = entry.entry.libraries || [];
  if (!libraries.length) {
    $('msg').textContent = 'No library yet. `dd library new` makes one on the machine that holds your key.';
    return;
  }
  for (const l of libraries) {
    try {
      lib = { id: l.id, key: library_open(entryText, l.id, secret) };
      break;
    } catch (e) { /* the next one, or none */ }
  }
  if (!lib) {
    $('msg').textContent = 'This browser is not linked to your files yet.';
    $('linkcmd').textContent = `dd passkey link ${passkeyId} ${library_device_key(secret)}`;
    $('link').hidden = false;
    return;
  }
  $('msg').hidden = true;
  $('up').onclick = () => $('picker').click();
  $('picker').onchange = () => upload($('picker').files);
  await show('');
}

start().catch((e) => { $('msg').textContent = String(e.message || e); });
