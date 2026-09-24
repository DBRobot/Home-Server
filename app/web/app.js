// The app's pages. One command, `status`, says which page applies: no name
// yet, a name whose entry does not list this device, or signed in. The
// admit page polls until the laptop has added this device.

const invoke = window.__TAURI__.core.invoke;
const $ = (id) => document.getElementById(id);
let timer = null;

function show(page) {
  for (const p of document.querySelectorAll(".page")) p.hidden = p.id !== "page-" + page;
}

function when(t) {
  return t ? new Date(t * 1000).toLocaleString() : "";
}

function render(st) {
  $("who").textContent = st.name ? `${st.name} · ${st.fingerprint}` : "";
  if (!st.name) return show("name");
  if (!st.admitted) {
    $("admit-cmd").textContent = `dd device admit ${st.public_key}`;
    $("admit-fp").textContent = st.fingerprint;
    $("admit-dirs").textContent = st.directories.map(([d, s]) => `${d.replace(/^https?:\/\//, "").replace(/\/_dd\/directory$/, "")}: ${s}`).join(" · ");
    return show("admit");
  }
  const e = st.entry;
  $("home-name").textContent = st.name;
  $("home-entry").textContent = `version ${e.version}, updated ${when(e.updated)}`;
  $("home-libraries").textContent = e.libraries;
  $("home-passkeys").textContent = e.passkeys;
  const gate = $("home-gate");
  gate.textContent = st.gate || "";
  gate.className = st.gate && st.gate.startsWith("accepted") ? "ok" : "error";
  const ul = $("home-devices");
  ul.replaceChildren(...e.devices.map((d) => {
    const li = document.createElement("li");
    li.className = d.this ? "this" : "";
    li.append(d.fingerprint);
    const tag = document.createElement("span");
    tag.className = "tag";
    tag.textContent = d.this ? "this device" : `added ${when(d.added)}`;
    li.append(tag);
    return li;
  }));
  show("home");
  media();
}

// Movies & TV: the libraries as folders and jellyfin on them, on this
// machine, in a window of its own
async function media(st) {
  try {
    st = st || await invoke("media_status");
  } catch (e) {
    $("media-text").textContent = String(e);
    return;
  }
  const text = $("media-text");
  if (st.running) {
    text.textContent = `Jellyfin is running on this machine with ${st.libraries} librar${st.libraries === 1 ? "y" : "ies"}, ${st.files} file(s), folders at ${st.at}.`;
  } else if (st.jellyfin) {
    text.textContent = "Your libraries as folders on this machine, and Jellyfin on them. Nothing is stored here in the clear except while it plays.";
  } else {
    text.textContent = "No Jellyfin on this machine: install it, or start the app with COMMONTY_JELLYFIN set.";
  }
  $("media-open").textContent = st.running ? "Show" : "Open";
  $("media-open").disabled = !st.jellyfin;
  $("media-close").hidden = !st.running;
}

$("media-open").addEventListener("click", async () => {
  const b = $("media-open");
  b.disabled = true;
  b.textContent = "Starting…";
  $("media-error").hidden = true;
  try {
    media(await invoke("media_open"));
  } catch (e) {
    $("media-error").textContent = String(e);
    $("media-error").hidden = false;
    b.disabled = false;
    b.textContent = "Open";
  }
});
$("media-close").addEventListener("click", async () => {
  await invoke("media_close");
  media();
});

async function refresh() {
  try {
    const st = await invoke("status");
    render(st);
    clearTimeout(timer);
    if (st.name && !st.admitted) timer = setTimeout(refresh, 5000);
  } catch (e) {
    $("error-text").textContent = String(e);
    show("error");
  }
}

$("name-form").addEventListener("submit", async (ev) => {
  ev.preventDefault();
  $("name-error").hidden = true;
  try {
    await invoke("set_name", { name: $("name").value });
    refresh();
  } catch (e) {
    $("name-error").textContent = String(e);
    $("name-error").hidden = false;
  }
});
$("admit-back").addEventListener("click", async () => { await invoke("forget"); refresh(); });
$("home-forget").addEventListener("click", async () => { await invoke("forget"); refresh(); });
$("error-retry").addEventListener("click", refresh);

refresh();
