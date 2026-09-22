# The games test: a member starts a server, a deploy and a reboot leave it, the world outlives it.

m = "curl -s -o /dev/null -w '%{http_code}' -X POST -H 'x-dd-user: tom' http://127.0.0.1:4182"
d = "/var/lib/dd-games/instances/probe1"
up = "curl -sf -m 5 http://127.0.0.1:27015/ >/dev/null"
world = d + "/server/world"
box.wait_for_unit("dd-games.service")
box.wait_for_open_port(4182)

# nobody: no page. A member: the catalogue.
assert box.succeed("curl -s -o /dev/null -w '%{http_code}' http://127.0.0.1:4182/").strip() == "401"
assert "Probe" in box.succeed("curl -sf -H 'x-dd-user: tom' http://127.0.0.1:4182/")
assert "Server name" in box.succeed("curl -sf -H 'x-dd-user: tom' http://127.0.0.1:4182/game/probe")
# the demo sees the card without the button, and the page code is served
demo = box.succeed("curl -sf -H 'x-dd-user: demo' http://127.0.0.1:4182/game/probe")
assert "Start a server" not in demo and "can look" in demo
box.succeed("curl -sf http://127.0.0.1:4182/static/games.css | grep -q ribbon")
box.succeed("curl -sf http://127.0.0.1:4182/static/games.js | grep -q Escape")

# tom asks for one; the manager, which is not root, may start it
assert box.succeed(f"{m}/create/probe -d SERVER_NAME=toms").strip() == "303"
box.wait_until_succeeds(up, timeout=1500)
assert box.succeed(f"cat {d}/status").strip() == "running"
page = box.succeed("curl -sf -H 'x-dd-user: tom' http://127.0.0.1:4182/")
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
