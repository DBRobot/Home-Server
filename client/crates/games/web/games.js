// The Games pages work without this; it makes them feel instant.
(() => {
  // the library filters as you type; the form still submits without it
  const q = document.getElementById('q');
  if (q) {
    const tiles = [...document.querySelectorAll('.grid .tile')];
    const empty = document.getElementById('none');
    q.addEventListener('input', () => {
      const s = q.value.trim().toLowerCase();
      let n = 0;
      for (const t of tiles) {
        const hit = !s || t.dataset.n.includes(s);
        t.parentElement.hidden = !hit;
        if (hit) n++;
      }
      if (empty) empty.hidden = n > 0;
    });
  }

  // a game's card over the library: the veil, the ×, and Escape all go back
  const d = document.querySelector('dialog');
  if (d) {
    document.addEventListener('keydown', e => {
      if (e.key === 'Escape') location.href = d.dataset.back;
    });
    const first = d.querySelector('input:not([type=hidden]),select');
    if (first) first.focus();
  }

  // a server's page: while it installs or starts, ask for its state and
  // swap in what changed instead of reloading the whole page
  const live = document.querySelector('[data-live]');
  if (live) {
    const tick = async () => {
      try {
        const r = await fetch(location.pathname + '/state', { cache: 'no-store' });
        if (!r.ok) return;
        const s = await r.json();
        const st = document.getElementById('st');
        if (st && st.textContent !== s.label) {
          st.textContent = s.label;
          st.className = 'state ' + s.state;
        }
        const log = document.getElementById('log');
        if (log && log.textContent !== s.log) {
          const atEnd = log.scrollTop + log.clientHeight >= log.scrollHeight - 8;
          log.textContent = s.log || 'nothing yet';
          if (atEnd) log.scrollTop = log.scrollHeight;
        }
        if (s.state === 'running' || s.state === 'stopped' || s.state === 'failed') {
          // the controls differ by state: one reload to get them right
          if (live.dataset.live !== s.state) location.reload();
          return;
        }
      } catch (e) {}
      setTimeout(tick, 3000);
    };
    setTimeout(tick, 3000);
  }
})();
