#!/usr/bin/env python3
"""What an egg's daemon does, in the guest: install the server with the
egg's own script, fill its files from the settings, render its startup
line, run it, tell the box when it is ready, and stop it the egg's way.

The record is /instance/instance.json (dd-games writes it); the game's
own output goes to /instance/game.log (systemd appends it); its status to
/instance/status.
"""
import json, os, re, shlex, shutil, signal, subprocess, sys, time

REC = "/instance/instance.json"
HOME = os.environ.get("HOME", "/var/lib/game")
# the egg's /home/container: on the instance's share, so the game and its
# world are files on the box, backed up, and survive this guest's disk
SERVER = "/instance/server"

def say(s):
    with open("/instance/status", "w") as f:
        f.write(s)

say("starting")
rec = json.load(open(REC))
recipe, env = rec["recipe"], dict(rec["env"])
env["SERVER_IP"] = "0.0.0.0"
env.setdefault("SERVER_PORT", str(next(p["port"] for p in rec["ports"] if p["var"] == "SERVER_PORT")))
env["SERVER_MEMORY"] = str(rec["memory"])
env["STARTUP"] = recipe["startup"]

def fill(template, extra=None):
    """{{VAR}} and {{server.build.env.VAR}} the way the eggs write them"""
    def one(m):
        k = m.group(1).split(".")[-1]
        if extra and k in extra: return extra[k]
        return env.get(k, m.group(0) if k not in env else "")
    return re.sub(r"{{\s*([A-Za-z0-9_.]+)\s*}}", one, template)

def install():
    """the egg's script, as it wrote it: /mnt/server is the install dir"""
    os.makedirs(SERVER, exist_ok=True)
    # installed once, a server starts straight away: re-checking gigabytes
    # against steam on every restart is minutes for nothing. A person turns
    # "Update on start" on when they want the game's next version.
    if os.path.exists(os.path.join(SERVER, ".installed")) and env.get("AUTO_UPDATE", "0") in ("0", "false", ""):
        return
    say("updating")
    # the eggs are edited on every platform: some carry windows line endings
    script = recipe["install"].replace("\r\n", "\n").replace("\r", "\n")
    # /mnt/server and /home/container exist in the guest as links to the
    # share, so the eggs' paths need no rewriting; chown of them is a no-op
    # every egg's first lines fetch steamcmd into the install dir; ours is on the path
    # the eggs fetch steam's own steamcmd and run it: a 32-bit binary that
    # wants steam's runtime, which steam-run provides around the whole script
    stub = "\n".join([
        "chown() { :; }; export -f chown",
        "apt() { :; }; apt-get() { :; }; sudo() { \"$@\"; }; export -f apt apt-get sudo",
        "export HOME=%s USER=game" % SERVER,
    ])
    r = subprocess.run(["steam-run", "bash", "-c", stub + "\n" + script], env={**os.environ, **env, "HOME": SERVER}, cwd=SERVER)
    if r.returncode != 0:
        say("failed: the install script")
        sys.exit(r.returncode)
    open(os.path.join(SERVER, ".installed"), "w").write(str(int(time.time())))

def rewrite_files():
    """config.files: find/replace in the egg's formats"""
    files = recipe.get("files") or {}
    for rel, spec in files.items():
        # some eggs name the file from the container's root
        rel = rel.replace("/home/container/", "").replace("/mnt/server/", "").lstrip("/")
        path = os.path.join(SERVER, rel)
        parser, find = spec.get("parser"), spec.get("find") or {}
        if not find: continue
        os.makedirs(os.path.dirname(path), exist_ok=True)
        try:
            if parser == "ini":
                rewrite_ini(path, find)
            elif parser == "json":
                rewrite_json(path, find)
            elif parser == "properties":
                rewrite_properties(path, find)
            elif parser == "yaml":
                rewrite_yaml(path, find)
            elif parser == "xml":
                rewrite_xml(path, find)
            elif parser == "file":
                rewrite_file(path, find)
        except Exception as e:
            print("config %s: %s" % (rel, e), flush=True)

def rewrite_ini(path, find):
    lines = open(path).read().split("\n") if os.path.exists(path) else []
    for key, val in find.items():
        val = fill(str(val))
        m = re.match(r"\[(.+?)\]\.(.+)", key)
        if not m: continue
        sect, name = m.group(1), m.group(2)
        out, cur, done, seen = [], None, False, False
        for ln in lines:
            s = ln.strip()
            if s.startswith("[") and s.endswith("]"):
                if cur == sect and not done:
                    out.append("%s=%s" % (name, val)); done = True
                cur = s[1:-1]; seen = seen or cur == sect
            elif cur == sect and re.match(r"\s*%s\s*=" % re.escape(name), ln) and not done:
                ln = "%s=%s" % (name, val); done = True
            out.append(ln)
        if not seen: out += ["[%s]" % sect, "%s=%s" % (name, val)]
        elif not done: out.append("%s=%s" % (name, val))
        lines = out
    open(path, "w").write("\n".join(lines))

def rewrite_json(path, find):
    data = json.load(open(path)) if os.path.exists(path) else {}
    for key, val in find.items():
        val = fill(str(val))
        cur = data
        parts = key.split(".")
        for p in parts[:-1]:
            cur = cur.setdefault(p, {})
        old = cur.get(parts[-1])
        if isinstance(old, bool): val = val.lower() in ("1", "true", "yes")
        elif isinstance(old, int): 
            try: val = int(val)
            except ValueError: pass
        cur[parts[-1]] = val
    json.dump(data, open(path, "w"), indent=2)

def rewrite_properties(path, find):
    lines = open(path).read().split("\n") if os.path.exists(path) else []
    for key, val in find.items():
        val = fill(str(val)); done = False
        for i, ln in enumerate(lines):
            if re.match(r"\s*%s\s*=" % re.escape(key), ln):
                lines[i] = "%s=%s" % (key, val); done = True
        if not done: lines.append("%s=%s" % (key, val))
    open(path, "w").write("\n".join(lines))

def rewrite_yaml(path, find):
    # the eggs only set flat or dotted scalar keys; a line rewrite covers them
    lines = open(path).read().split("\n") if os.path.exists(path) else []
    for key, val in find.items():
        val = fill(str(val)); leaf = key.split(".")[-1]; done = False
        for i, ln in enumerate(lines):
            m = re.match(r"(\s*)%s\s*:" % re.escape(leaf), ln)
            if m: lines[i] = "%s%s: %s" % (m.group(1), leaf, val); done = True; break
        if not done: lines.append("%s: %s" % (leaf, val))
    open(path, "w").write("\n".join(lines))

def rewrite_xml(path, find):
    text = open(path).read() if os.path.exists(path) else ""
    for key, val in find.items():
        val = fill(str(val)); leaf = key.split(".")[-1]
        text = re.sub(r"(<%s>)[^<]*(</%s>)" % (re.escape(leaf), re.escape(leaf)), lambda m: m.group(1) + val + m.group(2), text)
    open(path, "w").write(text)

def rewrite_file(path, find):
    text = open(path).read() if os.path.exists(path) else ""
    lines = text.split("\n")
    for key, val in find.items():
        val = fill(str(val)); done = False
        for i, ln in enumerate(lines):
            if ln.startswith(key):
                lines[i] = val; done = True
        if not done: lines.append(val)
    open(path, "w").write("\n".join(lines))

def run():
    cmd = fill(recipe["startup"]).replace("\r", "")
    say("starting")
    ready = recipe.get("ready")
    print("== %s" % cmd, flush=True)
    shell = ["bash", "-c", cmd]
    if recipe.get("wine"):
        # proton and wine need a display and their own prefix
        os.environ.setdefault("WINEPREFIX", os.path.join(SERVER, ".wine"))
        os.environ.setdefault("DISPLAY", ":99")
        subprocess.Popen(["Xvfb", ":99", "-screen", "0", "1024x768x16"], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        time.sleep(1)
    p = subprocess.Popen(shell, cwd=SERVER, env={**os.environ, **env, "HOME": SERVER},
                         stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True, bufsize=1)
    stop = recipe.get("stop") or "^C"
    def on_stop(*_):
        if stop.lstrip("^") == "C" and stop.startswith("^"):
            p.send_signal(signal.SIGINT)
        else:
            try:
                p.stdin.write(stop + "\n"); p.stdin.flush()
            except Exception:
                p.send_signal(signal.SIGINT)
        for _ in range(60):
            if p.poll() is not None: break
            time.sleep(1)
        if p.poll() is None: p.terminate()
    signal.signal(signal.SIGTERM, on_stop)
    signal.signal(signal.SIGINT, on_stop)
    # ready: the egg's line in the output, or, since some eggs' lines are
    # stale, the server's port answering; a thread watches the port
    up = [ready is None]
    port = int(env.get("SERVER_PORT", "0") or 0)
    def mark():
        if not up[0]:
            up[0] = True; say("running")
    if up[0]: say("running")
    import socket, threading
    def watch_port():
        while not up[0] and p.poll() is None:
            time.sleep(3)
            try:
                out = subprocess.run(["ss", "-Hlun", "-lt"], capture_output=True, text=True).stdout
                if re.search(r":%d\b" % port, out): mark()
            except Exception:
                pass
    if port: threading.Thread(target=watch_port, daemon=True).start()
    for line in p.stdout:
        sys.stdout.write(line); sys.stdout.flush()
        if ready and ready in line: mark()
    rc = p.wait()
    say("stopped" if rc == 0 else "failed: exit %d" % rc)
    sys.exit(rc)

def restore_world():
    """a kept world, handed over with the record: its files over the install"""
    src = "/instance/world"
    if not os.path.isdir(src): return
    n = 0
    for root, dirs, files in os.walk(src):
        for f in files:
            p = os.path.join(root, f); rel = os.path.relpath(p, src)
            dest = os.path.join(SERVER, rel)
            os.makedirs(os.path.dirname(dest), exist_ok=True)
            shutil.copy2(p, dest); n += 1
    shutil.rmtree(src)
    print("== world restored: %d files" % n, flush=True)

install()
restore_world()
rewrite_files()
run()
