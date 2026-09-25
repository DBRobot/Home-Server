# The library test: rclone over the gate, ciphertext in the bucket.
import json

box.wait_for_unit("garage.service")
box.wait_for_open_port(3901)
box.succeed("systemctl restart garage-setup.service")
box.wait_until_succeeds(f"set -a; . {nix['rpc']}; garage layout show > /tmp/l && grep -q 'Current cluster layout version: [1-9]' /tmp/l")
box.succeed("systemctl restart garage-setup.service")  # buckets and keys, now that the layout exists
box.wait_for_open_port(4181)

# a member with a library, made on this box with dd (the root is here)
env = "DD_KEYRING_FILE=/root/keys.json"
dirs = "--directory http://127.0.0.1:4181/_dd/directory"
box.succeed(f"{env} {nix['dd']} identity new --name sarah {dirs}")
out = box.succeed(f"{env} {nix['dd']} library new {dirs}")
lib = out.split("library ")[1].split(":")[0].strip()
assert len(lib) == 32, out
# a new library opens on something: the pages and the mount expect these,
# and a listing that hid folders is how an empty-looking library happens
listing = box.succeed(f"{env} {nix['dd']} library ls {lib} {dirs}")
for folder in ["Files/", "Movies/", "Shows/"]:
    assert folder in listing, (folder, listing)

# a folder made from the terminal, in a library that already exists
box.succeed(f"{env} {nix['dd']} library mkdir {lib} Archive {dirs}")

# what rclone needs: the library key as password, the id as salt, the gate
# as a webdav remote with the device token. dd prints the key here for
# exactly this (it is the member's own key, on the member's own machine)
key = box.succeed(f"{env} {nix['dd']} library key {lib} {dirs}").strip()
token = box.succeed(f"{env} {nix['dd']} token").strip()
box.succeed(f"""cat > /root/rclone.conf <<EOF
[dav]
type = webdav
url = http://127.0.0.1:4181/_dd/dav/{lib}
vendor = other
bearer_token = {token}

[lib]
type = crypt
remote = dav:
password = $(rclone obscure '{key}')
password2 = $(rclone obscure '{lib}')
EOF""")
rc = "rclone --config /root/rclone.conf"

# in, listed, back
box.succeed("mkdir -p /root/src/Movies/Big\\ Buck\\ Bunny\\ \\(2008\\) && head -c 3000000 /dev/urandom > '/root/src/Movies/Big Buck Bunny (2008)/film.mkv' && echo 'a note' > /root/src/Files/note.txt 2>/dev/null || (mkdir -p /root/src/Files && echo 'a note' > /root/src/Files/note.txt)")
box.succeed(f"{rc} copy /root/src lib:")
listed = box.succeed(f"{rc} lsf -R lib:")
assert "Movies/Big Buck Bunny (2008)/film.mkv" in listed and "Files/note.txt" in listed, listed
box.succeed(f"{rc} copy lib:Movies /root/back")
box.succeed("cmp '/root/src/Movies/Big Buck Bunny (2008)/film.mkv' '/root/back/Big Buck Bunny (2008)/film.mkv'")
assert box.succeed(f"{rc} cat lib:Files/note.txt").strip() == "a note"

# dd reads what rclone wrote, and the other way
assert "Files/note.txt" in box.succeed(f"{env} {nix['dd']} library ls {lib} {dirs}")
box.succeed(f"echo 'from dd' > /root/dd.txt && {env} {nix['dd']} library put {lib} /root/dd.txt --as Files/dd.txt {dirs}")
assert box.succeed(f"{rc} cat lib:Files/dd.txt").strip() == "from dd"

# the bucket: ciphertext under names nobody reads, nothing plain
aws = "AWS_ACCESS_KEY_ID=GK0123456789abcdef01234567 AWS_SECRET_ACCESS_KEY=0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef aws --endpoint-url http://127.0.0.1:3900 --region garage s3api list-objects-v2 --bucket libraries --query 'Contents[].Key' --output json"
keys = box.succeed(aws).strip()
keys = json.loads(keys or "[]")
assert all(k.startswith(lib + "/") for k in keys), keys
assert not any("film.mkv" in k or "note.txt" in k or "Movies" in k for k in keys), keys
box.fail("grep -rl 'a note' /srv/garage")

# delete is a move into the trash; nothing leaves the bucket
box.succeed(f"{rc} deletefile lib:Files/note.txt")
assert "Files/note.txt" not in box.succeed(f"{rc} lsf -R lib:")
keys_after = json.loads(box.succeed(aws).strip() or "[]")
assert any(k.startswith(f"{lib}/trash/") for k in keys_after), keys_after
assert len(keys_after) == len(keys), (keys, keys_after)

# A browser has a cookie and no device token, and the gate takes it:
# without this the Files and Movies pages could not read a single name.
# The demo is the account that signs in without a passkey, so it is the
# one a test can be. The cookie is Secure and domain-scoped, which curl
# will not store for a plain http call to a loopback address, so it is
# read off the response and sent back by hand.
demo_lib = "e14dbb2a30e3096a5a9bc42ace4599b1"
head = box.succeed("curl -s -D - -o /dev/null http://127.0.0.1:4181/_dd/demo")
cookie = [l for l in head.splitlines() if l.lower().startswith("set-cookie:")]
assert cookie, head
jar = cookie[0].split(":", 1)[1].split(";")[0].strip()
assert jar.startswith("dd_session="), jar
as_demo = f"-H 'Cookie: {jar}'"

cfg = json.loads(box.succeed(f"curl -s {as_demo} http://127.0.0.1:4181/_dd/config"))
assert cfg["demoLibrary"]["id"] == demo_lib, cfg
# its own library opens with that cookie
box.succeed(f"curl -s -o /dev/null -w '%{{http_code}}' {as_demo} -X PROPFIND http://127.0.0.1:4181/_dd/dav/{demo_lib}/ | grep -q 207")
# a member's does not
box.succeed(f"curl -s -o /dev/null -w '%{{http_code}}' {as_demo} -X PROPFIND http://127.0.0.1:4181/_dd/dav/{lib}/ | grep -q 403")
# and the demo writes nothing, even in its own
box.succeed(f"curl -s -o /dev/null -w '%{{http_code}}' {as_demo} -X MKCOL http://127.0.0.1:4181/_dd/dav/{demo_lib}/x | grep -q 403")
# the config says nothing about a library to anyone who is not the demo
assert "demoLibrary" not in json.loads(box.succeed("curl -s http://127.0.0.1:4181/_dd/config"))

# a stranger's token opens nothing; no token, nothing
box.succeed(f"DD_KEYRING_FILE=/root/tom.json {nix['dd']} identity new --name tom {dirs}")
tom = box.succeed(f"DD_KEYRING_FILE=/root/tom.json {nix['dd']} token").strip()
box.succeed(f"curl -s -o /dev/null -w '%{{http_code}}' -X PROPFIND -H 'Authorization: Bearer {tom}' http://127.0.0.1:4181/_dd/dav/{lib}/ | grep -q 403")
box.succeed(f"curl -s -o /dev/null -w '%{{http_code}}' -X PROPFIND http://127.0.0.1:4181/_dd/dav/{lib}/ | grep -q 401")
