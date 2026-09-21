{
  config,
  pkgs,
  lib,
  ...
}:
let
  cfg = config.dd.membersRunner;
  base = config.dd.domain;
  name = "members-${config.networking.hostName}";
  # the job containers' own network; what leaves it is decided below
  net = "members";
  subnet = "172.30.0.0/24";
in
{
  options.dd.membersRunner.tokenFile = lib.mkOption {
    type = lib.types.nullOr lib.types.str;
    default = null;
    description = "Registration token file for the members' runner (a global token: every repo). Null: the sandbox alone, no runner registered.";
  };

  # CI for everyone's repos, on an owned box, without the job being able to
  # reach the box. A second runner, registered for every repo, runs each
  # job in a container under gVisor: the job sees a sandboxed kernel, not
  # this one. The container has no secrets, no volume from the host, no
  # docker socket, a cpu and memory cap, and a network that reaches the
  # internet and nothing of ours: the tailnet, the lan, this box. The host
  # runners (modules/forgejo-runner.nix) keep the owner's repos; a
  # member's workflow says `runs-on: members` and lands here.
  config = {
    dd.box.plaintext = [ "members-runner (runs members' code, sandboxed)" ];

    virtualisation.docker = {
      enable = true;
      daemon.settings = {
        runtimes.runsc.path = "${pkgs.gvisor}/bin/runsc";
        default-runtime = "runsc";
        # nothing here needs the legacy bridge to reach the host
        icc = false;
      };
    };

    services.gitea-actions-runner.instances.${name} = lib.mkIf (cfg.tokenFile != null) {
      enable = true;
      inherit name;
      url = "https://git.${base}";
      tokenFile = cfg.tokenFile;
      # the image a member's job runs in: the same one act uses for ubuntu
      labels = [ "members:docker://ghcr.io/catthehacker/ubuntu:act-22.04" ];
      settings = {
        runner = {
          capacity = 2;
          timeout = "1h";
        };
        container = {
          network = net;
          privileged = false;
          # nothing of the host mounted; no socket handed in
          valid_volumes = [ ];
          docker_host = "-";
          options = "--cpus=4 --memory=4g --pids-limit=1024 --security-opt=no-new-privileges";
          force_pull = false;
        };
        cache.enabled = false;
      };
    };
    systemd.services."gitea-runner-${name}" = lib.mkIf (cfg.tokenFile != null) {
      after = [
        "docker.service"
        "members-network.service"
      ];
      requires = [
        "docker.service"
        "members-network.service"
      ];
      serviceConfig.SupplementaryGroups = [ "docker" ];
    };

    # the jobs' network exists before the runner starts
    systemd.services.members-network = {
      description = "The docker network members' jobs run in";
      after = [ "docker.service" ];
      requires = [ "docker.service" ];
      wantedBy = [ "multi-user.target" ];
      path = [ pkgs.docker ];
      serviceConfig = {
        Type = "oneshot";
        RemainAfterExit = true;
      };
      script = ''
        docker network inspect ${net} >/dev/null 2>&1 || docker network create --subnet ${subnet} ${net}
      '';
    };

    # What a job may reach: the internet. Docker's DOCKER-USER chain runs
    # before its own forwarding rules; established replies come back, the
    # rest of what is ours is dropped before it leaves the bridge.
    networking.firewall.extraCommands = ''
      iptables -N DOCKER-USER 2>/dev/null || true
      iptables -F DOCKER-USER
      iptables -A DOCKER-USER -m conntrack --ctstate ESTABLISHED,RELATED -j RETURN
      for dst in 100.64.0.0/10 10.0.0.0/8 192.168.0.0/16 172.16.0.0/12 169.254.0.0/16 127.0.0.0/8; do
        iptables -A DOCKER-USER -s ${subnet} -d $dst -j DROP
      done
      iptables -A DOCKER-USER -j RETURN
      # and the box itself, on any of its addresses
      iptables -I nixos-fw 1 -s ${subnet} -j DROP
    '';
    networking.firewall.extraStopCommands = ''
      iptables -F DOCKER-USER 2>/dev/null || true
    '';
  };
}
