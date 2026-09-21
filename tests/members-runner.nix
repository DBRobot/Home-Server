# A member's job runs in a box that is not this one. The module puts every
# container under gVisor on a network that reaches nothing of ours; here a
# container tries the tailnet, the lan and the box itself, and gets nowhere,
# while the kernel it sees is gVisor's, not the host's.
{ pkgs, self, ... }:
let
  probe = pkgs.dockerTools.buildImage {
    name = "probe";
    tag = "1";
    copyToRoot = pkgs.buildEnv {
      name = "probe-root";
      paths = [
        pkgs.busybox
        pkgs.curl
      ];
      pathsToLink = [ "/bin" ];
    };
    config.Cmd = [ "/bin/sh" ];
  };
in
{
  name = "members-runner";
  node.specialArgs = { inherit self; };
  nodes = {
    box = {
      imports = [
        ./box.nix
        ../modules/members-runner.nix
      ];
      # no token: the runner itself would need the forge; the sandbox is
      # what is tested
      networking.firewall.enable = pkgs.lib.mkForce true;
      networking.firewall.allowedTCPPorts = [ 8080 ];
      virtualisation.memorySize = 2048;
      virtualisation.diskSize = 4096;
      # something of ours to try to reach: a service on the box
      systemd.services.ours = {
        wantedBy = [ "multi-user.target" ];
        script = "${pkgs.python3}/bin/python3 -m http.server 8080 --bind 0.0.0.0";
      };
    };
    other = {
      # a second box on the lan, also ours
      networking.firewall.enable = false;
      systemd.services.ours = {
        wantedBy = [ "multi-user.target" ];
        script = "${pkgs.python3}/bin/python3 -m http.server 8080 --bind 0.0.0.0";
      };
    };
  };
  testScript = ''
    start_all()
    box.wait_for_unit("docker.service")
    other.wait_for_unit("ours.service")
    box.wait_for_unit("ours.service")
    box.succeed("docker load < ${probe}")
    assert "runsc" in box.succeed("docker info --format '{{.DefaultRuntime}}'")

    run = lambda cmd: box.execute(f"docker run --rm probe:1 /bin/sh -c '{cmd}'")

    # the kernel a job sees is gVisor's
    status, out = run("uname -a")
    assert "gVisor" in out or "4.4.0" in out, out

    # nothing of ours answers: the box itself, and another box on the lan
    for target in ("192.168.1.1", "192.168.1.2"):
        status, out = run(f"curl -sS -m 5 http://{target}:8080/ >/dev/null; echo rc=$?")
        assert "rc=0" not in out, f"{target} was reachable from a job: {out}"

    # and the tailnet range is dropped before it leaves
    status, out = run("curl -sS -m 5 http://100.100.100.100/ >/dev/null; echo rc=$?")
    assert "rc=0" not in out, out

    # names are asked of the box's own nameservers, not docker's embedded
    # one, which gVisor cannot reach
    status, out = run("cat /etc/resolv.conf")
    assert "127.0.0.11" not in out, out

    # the job cannot see the host's files or its docker
    status, out = run("ls /nix/store | head -1; ls /var/run/docker.sock; echo done")
    assert "docker.sock" not in out.replace("cannot access", ""), out
  '';
}
