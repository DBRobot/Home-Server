# A box moves itself to a signed release and to nothing else. The forge
# here is a web server with one file; the release key is made in the test
# and the box gets only its public half. A good release switches the box,
# an older counter is refused, a forged signature is refused, and a release
# that leaves the box unhealthy rolls back on its own.
{ pkgs, self, ... }:
let
  dd = "${self.packages.${pkgs.stdenv.hostPlatform.system}.dd}/bin/dd";
in
{
  name = "release";
  node.specialArgs = { inherit self; };
  defaults.virtualisation.memorySize = 1536;
  defaults.virtualisation.diskSize = 4096;
  nodes = {
    forge = {
      services.nginx = {
        enable = true;
        virtualHosts.forge.root = "/srv/release";
      };
      networking.firewall.enable = false;
      environment.systemPackages = [ pkgs.git ];
    };
    box = {
      imports = [
        ./box.nix
        ../modules/agent.nix
      ];
      services.openssh.enable = true;
      dd.agent = {
        enable = true;
        url = "http://forge/current.json";
        publicKeyFile = "/root/release.pub"; # made below, not at build time
        probeSeconds = 10;
      };
      # the same box with a mark on it: what a release moves it to
      specialisation.marked.configuration.environment.etc."dd-marker".text = "release 2\n";
      # and a broken one: ssh gone, which the probe must catch
      specialisation.broken.configuration = {
        environment.etc."dd-marker".text = "release 3\n";
        services.openssh.enable = pkgs.lib.mkForce false;
      };
    };
  };
  testScript =
    { nodes, ... }:
    let
      base = nodes.box.system.build.toplevel;
      marked = nodes.box.specialisation.marked.configuration.system.build.toplevel;
      broken = nodes.box.specialisation.broken.configuration.system.build.toplevel;
    in
    ''
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
      pub = forge.succeed(f"{env} ${dd} release init --repo /root/repo | tail -1").strip()
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
          forge.succeed(f"{env} ${dd} release sign /root/payload.json --out /root/current.json")
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
      publish(1, "${base}")
      agent()
      box.succeed("[ $(cat /var/lib/dd-agent/counter) = 1 ]")

      # release 2 moves it, and leaves no guard behind
      publish(2, "${marked}")
      agent()
      box.succeed("! systemctl is-active dd-agent-guard.timer")
      box.succeed("[ $(readlink /run/current-system) = ${marked} ]")
      box.succeed("grep -q 'release 2' /etc/dd-marker")
      box.succeed("[ $(cat /var/lib/dd-agent/counter) = 2 ]")
      box.succeed("grep -q 'dd_agent_counter{box=\"box\"} 2' /var/lib/dd-facts/dd_agent.prom")

      # an older release is refused, whatever it names
      publish(1, "${base}")
      agent(expect_ok=False)
      box.succeed("[ $(readlink /run/current-system) = ${marked} ]")
      box.succeed("[ $(cat /var/lib/dd-agent/counter) = 2 ]")

      # a file the key did not sign is refused
      publish(5, "${base}", tamper=True)
      agent(expect_ok=False)
      box.succeed("[ $(readlink /run/current-system) = ${marked} ]")
      box.succeed("journalctl -u dd-agent > /tmp/j && grep -q 'release refused' /tmp/j")

      # a release that leaves the box without ssh comes back by itself
      publish(3, "${broken}")
      agent(expect_ok=False)
      box.succeed("[ $(readlink /run/current-system) = ${marked} ]")
      box.wait_for_unit("sshd.service")
      box.succeed("grep -q 'release 2' /etc/dd-marker")
      box.succeed("[ $(cat /var/lib/dd-agent/counter) = 2 ]")
      box.succeed("grep -q 'result=\"rollback\"' /var/lib/dd-facts/dd_agent.prom")
      box.succeed("! systemctl is-active dd-agent-guard.service")
      box.succeed("! systemctl is-active dd-agent-guard.timer")
    '';
}
