# The directory test: entries, invites and revocation across boxes.

start_all()
for m in (a, b, c):
    m.wait_for_unit("dd-verify.service")
    m.wait_for_open_port(4181)
a.wait_until_succeeds("journalctl -u dd-verify > /tmp/j && grep -q 'every peer pulled once' /tmp/j")
b.wait_until_succeeds("journalctl -u dd-verify > /tmp/j && grep -q 'every peer pulled once' /tmp/j")

env = "DD_KEYRING_FILE=/root/laptop.json"
client.succeed(f"{env} {nix['dd']} identity new --name sarah --directory http://a:4181/_dd/directory")
b.wait_until_succeeds("curl -sf http://localhost:4181/_dd/directory/sarah")

# a stranger's sarah on b: b asks a, which holds her under another root
client.fail(f"DD_KEYRING_FILE=/root/stranger.json {nix['dd']} identity new --name sarah --directory http://b:4181/_dd/directory")

# c never pulled its peer: it refuses a new name
out = client.fail(f"DD_KEYRING_FILE=/root/tom.json {nix['dd']} identity new --name tom --directory http://c:4181/_dd/directory 2>&1")
assert "503" in out or "not yet pulled" in out, out

# a device added through b alone reaches a
pub = client.succeed(f"DD_KEYRING_FILE=/root/phone.json {nix['dd']} device show | tail -1").strip()
client.succeed(f"{env} {nix['dd']} device admit {pub} --directory http://b:4181/_dd/directory")
a.wait_until_succeeds("curl -sf http://localhost:4181/_dd/directory/sarah -o /tmp/out && grep -q '\"version\":2' /tmp/out")

# b restarts and still has sarah at version 2
b.succeed("systemctl restart dd-verify")
b.wait_for_open_port(4181)
b.wait_until_succeeds("curl -sf http://localhost:4181/_dd/directory/sarah -o /tmp/out && grep -q '\"version\":2' /tmp/out")
