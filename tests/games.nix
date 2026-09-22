# A game server is an instance of a template, made at run time by a member
# through the manager, and neither a deploy nor a reboot loses it. Here the
# "game" is a web server, so nothing is fetched from steam: a member asks for
# one, the guest boots and serves on the port it was given, a switch leaves
# its process alone, a reboot brings it back with its world, another member
# cannot touch it, and a stop is a clean one.
{ pkgs, self, ... }:
{
  name = "games";
  node.specialArgs = { inherit self; };
  nodes.box = {
    imports = [
      ./box.nix
      ../modules/games.nix
    ];
    dd.games.catalogue.probe = {
      name = "Probe";
      exec = "python3";
      args = [
        "-m"
        "http.server"
        "8080"
        "--directory"
        "/instance/saves/0"
      ];
      ports = [
        {
          proto = "tcp";
          guest = 8080;
        }
      ];
      saves = [ "world" ];
      memory = 768;
      cores = 1;
    };
    # a guest inside this guest: needs a builder with kvm, like every VM test
    # here, and nesting on (the kernel default)
    dd.games.portBase = 27015;
    dd.box.tailnet = "100.64.0.9";
    # the test framework leaves the switch script out of a test box
    system.switch.enable = true;
    environment.systemPackages = [ pkgs.curl pkgs.jq ];
    virtualisation.memorySize = 3072;
    virtualisation.cores = 2;
    virtualisation.diskSize = 8192;
  };
  testScript =
    { nodes, ... }:
    ''
    m = "curl -s -o /dev/null -w '%{http_code}' -X POST -H 'x-dd-user: tom' http://127.0.0.1:4182"
    d = "/var/lib/dd-games/instances/probe1"
    up = "curl -sf -m 5 http://127.0.0.1:27015/ >/dev/null"
    box.wait_for_unit("dd-games.service")
    box.wait_for_open_port(4182)

    # nobody: no page. A member: the catalogue.
    assert box.succeed("curl -s -o /dev/null -w '%{http_code}' http://127.0.0.1:4182/").strip() == "401"
    assert "Probe" in box.succeed("curl -sf -H 'x-dd-user: tom' http://127.0.0.1:4182/")

    # tom asks for one; the manager, which is not root, may start it
    assert box.succeed(f"{m}/create/probe").strip() == "303"
    box.wait_until_succeeds(up, timeout=1500)
    assert box.succeed(f"cat {d}/status").strip() == "running"
    page = box.succeed("curl -sf -H 'x-dd-user: tom' http://127.0.0.1:4182/")
    assert "100.64.0.9:27015" in page, page

    # one each: a second is refused
    box.succeed(f"{m}/create/probe")
    box.fail("test -e /var/lib/dd-games/instances/probe2")

    # its world is files on the box
    box.succeed(f"echo hello > {d}/saves/0/world.txt")
    assert "hello" in box.succeed("curl -sf http://127.0.0.1:27015/world.txt")

    # a deploy: the switch runs, the game's process is the same one after
    pid = box.succeed("systemctl show -p MainPID --value dd-game@probe1.service").strip()
    box.succeed("${nodes.box.system.build.toplevel}/bin/switch-to-configuration test 2>&1 | tail -3")
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
    assert box.succeed(f"{m}/delete/probe1").strip() == "303"
    box.fail(f"test -e {d}")
  '';
}
