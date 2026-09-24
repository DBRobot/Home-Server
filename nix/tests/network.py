# The network test: a box on its own network, a device handed the way in.
import json

box.wait_for_unit("headscale.service")
box.wait_for_unit("headscale-seed.service")
# the seed's work: the boxes' user, a box key, the gate's api key
box.succeed("headscale users list -o json | jq -e '.[] | select(.name == \"boxes\")'")
box.succeed("test -s /var/lib/headscale/box.key && test -s /var/lib/dd-verify/headscale-api.key")
# readable by the verifier and nobody else (systemd reowns its state dir on start)
owner = box.succeed("stat -c '%G:%a' /var/lib/dd-verify/headscale-api.key").strip()
assert owner == "dd-verify:440", owner
# the box on its own network, as a box (the keeper retries until it is)
box.wait_until_succeeds("tailscale --socket /run/commonty-net/tailscaled.sock status --json | jq -e '.BackendState == \"Running\"'", timeout=180)
ip = box.succeed("tailscale --socket /run/commonty-net/tailscaled.sock ip -4").strip()
assert ip.startswith("100."), ip
box.succeed("headscale nodes list -o json | jq -e '.[] | select(.name == \"box\") | .tags | index(\"tag:box\")'")
# names on the network: every host nginx serves here, at this address
box.wait_until_succeeds("jq -e 'length > 0' /var/lib/headscale/extra-records.json", timeout=60)
records = json.loads(box.succeed("cat /var/lib/headscale/extra-records.json"))
assert all(r["value"] == ip for r in records), records
# a member's device: made here, admitted by its own root, asks the gate
env = "DD_KEYRING_FILE=/root/keys.json"
box.wait_for_open_port(4181)
box.succeed(f"{env} {nix['dd']} identity new --name sarah --directory http://127.0.0.1:4181/_dd/directory")
token = box.succeed(f"{env} {nix['dd']} token").strip()
got = json.loads(box.succeed(f"curl -sf -X POST -H 'Authorization: Bearer {token}' http://127.0.0.1:4181/_dd/network/join"))
assert got["control_url"] == "http://127.0.0.1:8085", got
assert got["user"] == "sarah", got
assert len(got["key"]) > 20, got
# in headscale: sarah exists, with a key that is hers alone and once (a
# false in this json is a field left out)
box.succeed("headscale users list -o json | jq -e '.[] | select(.name == \"sarah\")'")
keys = json.loads(box.succeed("headscale preauthkeys list -o json"))
hers = [k for k in keys if k.get("user", {}).get("name") == "sarah"]
assert len(hers) == 1 and not hers[0].get("reusable"), keys
# no token, no key; a stranger's token, no key
box.fail("curl -sf -X POST http://127.0.0.1:4181/_dd/network/join")
box.fail("curl -sf -X POST -H 'Authorization: Bearer nope' http://127.0.0.1:4181/_dd/network/join")
