# A box on the fleet's own network: a second tailscaled beside the one on
# the owner's Tailscale account, against the fleet's Headscale, with its
# own state, socket, port and interface. Both run until the fleet's names
# and every address have moved over and the old one is retired by hand.
# Names on this network come from Headscale; the box keeps its own
# resolver and takes none from here (accept-dns off). The control server's
# name resolves to the gateway's tailnet address, never through Cloudflare.
{
  config,
  pkgs,
  lib,
  ...
}:
let
  cfg = config.dd.net;
  base = config.dd.domain;
in
{
  options.dd.net = {
    enable = lib.mkEnableOption "this box on the fleet's own network";
    keyFile = lib.mkOption {
      type = lib.types.str;
      description = "file holding the boxes' join key (made by headscale-seed on the control box; carried to the others through sops)";
    };
    controlUrl = lib.mkOption {
      type = lib.types.str;
      default = "https://headscale.${base}";
      description = "the fleet's control server";
    };
    socket = lib.mkOption {
      type = lib.types.str;
      default = "/run/commonty-net/tailscaled.sock";
      readOnly = true;
    };
  };

  config = lib.mkIf cfg.enable {
    systemd.services.commonty-net = {
      description = "The fleet's own network: tailscaled against the fleet's Headscale";
      wantedBy = [ "multi-user.target" ];
      after = [ "network-online.target" ];
      wants = [ "network-online.target" ];
      path = [
        pkgs.iproute2
        pkgs.procps
        pkgs.getent
        pkgs.kmod
      ];
      serviceConfig = {
        ExecStart = ''
          ${pkgs.tailscale}/bin/tailscaled \
            --state=/var/lib/commonty-net/tailscaled.state \
            --socket=${cfg.socket} \
            --port=41642 \
            --tun=commonty0 \
            --no-logs-no-support
        '';
        StateDirectory = "commonty-net";
        StateDirectoryMode = "0700";
        RuntimeDirectory = "commonty-net";
        Restart = "on-failure";
        RestartSec = 5;
      };
    };

    # joined with the boxes' key; the key carries the tag the policy grants
    # to, so the node asks for none. The owner's tailscale keeps the
    # firewall, this one stays out of netfilter
    systemd.services.commonty-net-up = {
      description = "Join the fleet's own network";
      after = [ "commonty-net.service" ];
      requires = [ "commonty-net.service" ];
      wantedBy = [ "multi-user.target" ];
      path = [ pkgs.tailscale ];
      serviceConfig = {
        Type = "oneshot";
        RemainAfterExit = true;
      };
      script = ''
        set -eu
        for _ in $(seq 1 30); do
          tailscale --socket ${cfg.socket} status >/dev/null 2>&1 && break
          sleep 1
        done
        if tailscale --socket ${cfg.socket} status --json | ${pkgs.jq}/bin/jq -e '.BackendState == "Running"' >/dev/null; then
          exit 0
        fi
        for _ in $(seq 1 20); do
          [ -s ${cfg.keyFile} ] && break
          sleep 3
        done
        tailscale --socket ${cfg.socket} up \
          --login-server=${cfg.controlUrl} \
          --auth-key=file:${cfg.keyFile} \
          --hostname=${config.networking.hostName} \
          --accept-dns=false \
          --accept-routes=false \
          --netfilter-mode=off
      '';
    };

    networking.firewall.trustedInterfaces = [ "commonty0" ];
    networking.firewall.allowedUDPPorts = [ 41642 ];
    networking.networkmanager.unmanaged = [ "interface-name:commonty0" ];
  };
}
