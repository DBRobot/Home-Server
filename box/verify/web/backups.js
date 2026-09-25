// Backups: what each box has taken and how far back it goes. Nothing
// here reads a backup: the password is on the box that made it.

import { ready, failed, section, ago, when } from './panel.js';

async function start() {
  const boxes = await (await fetch('/_dd/fleet.json')).json();
  const body = ready();
  for (const b of boxes) {
    const k = b.backup;
    if (!k) {
      section(body, b.name, [], b.up ? 'Nothing on this box is backed up.' : 'No answer from this box.');
      continue;
    }
    const rows = [
      ['Last good run', ago(k.last_success), when(k.last_success)],
      ['Snapshots kept', k.snapshots != null ? String(k.snapshots) : '—',
        k.oldest ? `oldest ${when(k.oldest)}` : ''],
      ...k.paths.map((p) => ['Covers', p]),
    ];
    section(body, b.name, rows.map((r) => r.filter(Boolean)), '');
  }
  if (!boxes.length) section(body, 'The fleet', [], 'No boxes are declared here.');
}

start().catch(failed);
