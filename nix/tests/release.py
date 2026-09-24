# The release test: a signed release moves a box; a bad one is rolled back; a tampered one is refused.

import json
start_all()
forge.wait_for_unit("nginx.service")
box.wait_for_unit("dd-verify.service")
box.wait_for_open_port(4181)
box.wait_for_unit("sshd.service")

# the release key lives on the forge machine here, standing in for the
# laptop; the box gets the public half
env = "DD_KEYRING_FILE=/root/keys.json"
forge.succeed("git init -q /root/repo && mkdir -p /root/repo/fleet")
pub = forge.succeed(f"{env} {nix['dd']} release init --repo /root/repo | tail -1").strip()
box.succeed(f"echo '{pub}' > /root/release.pub")

def nar_hash(path):
    info = json.loads(box.succeed(f"nix --extra-experimental-features nix-command path-info --json {path}"))
    entry = info.get(path) or next(e for e in info if e["path"] == path)
    return entry["narHash"]

def publish(counter, path, tamper=False):
    payload = json.dumps({
        "counter": counter, "rev": "0123456789abcdef", "issued": 1,
        "boxes": {"box": {"path": path, "nar_hash": nar_hash(path)}},
    })
    forge.succeed(f"cat > /root/payload.json <<'EOF'\n{payload}\nEOF")
    forge.succeed(f"{env} {nix['dd']} release sign /root/payload.json --out /root/current.json")
    if tamper:
        forge.succeed("sed -i 's/\"counter\": [0-9]*/\"counter\": 99/' /root/current.json")
    forge.succeed("mkdir -p /srv/release && install -m 644 /root/current.json /srv/release/current.json")

def agent(expect_ok=True):
    # a switch can make systemd re-exec, which cuts systemctl's wait
    # short while the agent is still running: wait for the unit itself
    box.succeed("systemctl start --no-block dd-agent.service")
    box.wait_until_succeeds("[ $(systemctl is-active dd-agent.service) != active ] && [ $(systemctl is-active dd-agent.service) != activating ]", timeout=300)
    if expect_ok:
        box.succeed("! systemctl is-failed dd-agent.service")
    else:
        box.succeed("systemctl is-failed dd-agent.service")

# release 1 names what the box already runs: nothing to do, counter kept
publish(1, f"{nix['base']}")
agent()
box.succeed("[ $(cat /var/lib/dd-agent/counter) = 1 ]")

# release 2 moves it, and leaves no guard behind
publish(2, f"{nix['marked']}")
agent()
box.succeed("! systemctl is-active dd-agent-guard.timer")
box.succeed(f"[ $(readlink /run/current-system) = {nix['marked']} ]")
box.succeed("grep -q 'release 2' /etc/dd-marker")
box.succeed("[ $(cat /var/lib/dd-agent/counter) = 2 ]")
box.succeed("grep -q 'dd_agent_counter{box=\"box\"} 2' /var/lib/dd-facts/dd_agent.prom")

# an older release is refused, whatever it names
publish(1, f"{nix['base']}")
agent(expect_ok=False)
box.succeed(f"[ $(readlink /run/current-system) = {nix['marked']} ]")
box.succeed("[ $(cat /var/lib/dd-agent/counter) = 2 ]")

# a file the key did not sign is refused
publish(5, f"{nix['base']}", tamper=True)
agent(expect_ok=False)
box.succeed(f"[ $(readlink /run/current-system) = {nix['marked']} ]")
box.succeed("journalctl -u dd-agent > /tmp/j && grep -q 'release refused' /tmp/j")

# a release that leaves the box without ssh comes back by itself
publish(3, f"{nix['broken']}")
agent(expect_ok=False)
box.succeed(f"[ $(readlink /run/current-system) = {nix['marked']} ]")
box.wait_for_unit("sshd.service")
box.succeed("grep -q 'release 2' /etc/dd-marker")
box.succeed("[ $(cat /var/lib/dd-agent/counter) = 2 ]")
box.succeed("grep -q 'result=\"rollback\"' /var/lib/dd-facts/dd_agent.prom")
box.succeed("! systemctl is-active dd-agent-guard.service")
box.succeed("! systemctl is-active dd-agent-guard.timer")

# a release that leaves the box healthy but cut off from the fleet is not a
# release this box keeps: the switch may not take away what it had
publish(4, f"{nix['stranded']}")
agent(expect_ok=False)
box.succeed(f"[ $(readlink /run/current-system) = {nix['marked']} ]")
box.succeed("grep -q 'release 2' /etc/dd-marker")
box.succeed("[ $(cat /var/lib/dd-agent/counter) = 2 ]")
box.succeed("journalctl -u dd-agent > /tmp/j2 && grep -q 'could no longer reach the fleet' /tmp/j2")
