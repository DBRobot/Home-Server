# The games test: a member starts a server, a deploy and a reboot leave it, the world outlives it.

sock = "--unix-socket /run/dd-games/dd-games.sock"
m = "curl -s -o /dev/null -w '%{http_code}' " + sock + " -X POST -H 'x-dd-user: tom' http://games"
d = "/var/lib/dd-games/instances/probe1"
up = "curl -sf -m 5 http://127.0.0.1:27015/ >/dev/null"
world = d + "/server/world"
box.wait_for_unit("dd-games.service")
box.wait_until_succeeds("test -S /run/dd-games/dd-games.sock", timeout=60)

# no port at all: the manager believes x-dd-user, so only what can open
# its socket may ask. Anything else on this box (a CI job, a game guest)
# used to be able to name itself through 127.0.0.1:4182.
box.fail("curl -s -m 3 -o /dev/null http://127.0.0.1:4182/")

# nobody: no page. A member: the catalogue.
assert box.succeed(f"curl -s -o /dev/null -w '%{{http_code}}' {sock} http://games/").strip() == "401"
assert "Probe" in box.succeed(f"curl -sf {sock} -H 'x-dd-user: tom' http://games/")
assert "Server name" in box.succeed(f"curl -sf {sock} -H 'x-dd-user: tom' http://games/game/probe")
# the demo sees the card without the button, and the page code is served
demo = box.succeed(f"curl -sf {sock} -H 'x-dd-user: demo' http://games/game/probe")
assert "Start a server" not in demo and "can look" in demo
box.succeed(f"curl -sf {sock} http://games/static/games.css | grep -q ribbon")
box.succeed(f"curl -sf {sock} http://games/static/games.js | grep -q Escape")

# tom asks for one; the manager, which is not root, may start it
assert box.succeed(f"{m}/create/probe -d SERVER_NAME=toms").strip() == "303"
box.wait_until_succeeds(up, timeout=1500)
assert box.succeed(f"cat {d}/status").strip() == "running"
page = box.succeed(f"curl -sf {sock} -H 'x-dd-user: tom' http://games/")
assert "100.64.0.9:27015" in page, page

# one each: a second is refused
box.succeed(f"{m}/create/probe -d SERVER_NAME=again")
box.fail("test -e /var/lib/dd-games/instances/probe2")

# its world is files on the box
box.succeed(f"echo hello > {world}/world.txt")
assert "hello" in box.succeed("curl -sf http://127.0.0.1:27015/world.txt")

# a deploy: the switch runs, the game's process is the same one after
pid = box.succeed("systemctl show -p MainPID --value dd-game@probe1.service").strip()
box.succeed(f"{nix['toplevel']}/bin/switch-to-configuration test 2>&1 | tail -3")
assert box.succeed("systemctl show -p MainPID --value dd-game@probe1.service").strip() == pid
box.succeed("curl -sf -m 5 http://127.0.0.1:27015/world.txt")

# someone else can see it and cannot stop it
other = m.replace("tom", "ann")
box.succeed(f"{other}/stop/probe1")
box.succeed(up)

# and cannot read its log, which is where a game server writes passwords,
# addresses and whatever a player types at it
own = box.succeed(f"curl -s -o /dev/null -w '%{{http_code}}' {sock} -H 'x-dd-user: tom' http://games/server/probe1/state").strip()
assert own == "200", own
theirs = box.succeed(f"curl -s -o /dev/null -w '%{{http_code}}' {sock} -H 'x-dd-user: ann' http://games/server/probe1/state").strip()
assert theirs == "404", theirs

# a reboot: nothing in the configuration names this server, and it is back
box.shutdown()
box.start()
box.wait_for_unit("dd-games.service")
box.wait_until_succeeds("curl -sf -m 5 http://127.0.0.1:27015/world.txt | grep -q hello", timeout=1500)

# a stop is a shutdown of the guest, not a kill, and it stays stopped
assert box.succeed(f"{m}/stop/probe1").strip() == "303"
box.wait_until_fails("curl -sf -m 3 http://127.0.0.1:27015/", timeout=120)
box.wait_until_succeeds("systemctl show -p ActiveState dd-game@probe1.service | grep -q inactive", timeout=120)
assert "Result=success" in box.succeed("systemctl show -p Result dd-game@probe1.service")
# the world outlives the server: kept, then a new server starts with it
assert box.succeed(f"{m}/keep/probe1").strip() == "303"
box.fail(f"test -e {d}")
name = box.succeed("ls /var/lib/dd-games/worlds/tom").strip()
assert "hello" in box.succeed(f"cat /var/lib/dd-games/worlds/tom/{name}/world/world.txt")
assert box.succeed(f"{m}/worlds/{name}/start").strip() == "303"
box.wait_until_succeeds("curl -sf -m 5 http://127.0.0.1:27015/world.txt | grep -q hello", timeout=600)
box.succeed("test -e /var/lib/dd-games/instances/probe1/server/world/world.txt")
assert box.succeed(f"{m}/stop/probe1").strip() == "303"
box.wait_until_succeeds("systemctl show -p ActiveState dd-game@probe1.service | grep -q inactive", timeout=120)
assert box.succeed(f"{m}/delete/probe1").strip() == "303"
box.fail(f"test -e {d}")
