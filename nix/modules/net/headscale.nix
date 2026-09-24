# The fleet's own network control server. Every member's devices and every
# box are nodes of one Headscale tailnet: the app joins it with a key the
# gate hands an admitted device, a box joins it with the boxes' key made
# here. Relays and NAT traversal come from Tailscale's public DERP servers
# until the fleet has a public box of its own, so the control server is
# plain https and rides the front door like the pages do.
#
# Names on this network: Headscale's DNS answers every host nginx serves
# here with this box's address on it (extra records, written once this box
# has joined), so a device on the network reaches the gate directly and
# never through Cloudflare.
{
  config,
  pkgs,
  lib,
  ...
}:
let
  cfg = config.dd.headscale;
  base = config.dd.domain;
  host = "headscale.${base}";
  port = 8085;
  state = "/var/lib/headscale";
  # who may reach what: everyone on the network reaches every box, boxes
  # reach each other; members' devices do not see one another
  policy = pkgs.writeText "headscale-policy.json" (
    builtins.toJSON {
      tagOwners."tag:box" = [ "boxes@" ];
      acls = [
        {
          action = "accept";
          src = [ "*" ];
          dst = [ "tag:box:*" ];
        }
      ];
    }
  );
  # every name nginx serves on this box, answered on the network with this
  # box's network address (written by the join keeper, dd.net.afterJoin)
  names = lib.filter (n: lib.hasSuffix ".${base}" n || n == base) (
    builtins.attrNames config.services.nginx.virtualHosts
  );
in
{
  options.dd.headscale = {
    enable = lib.mkEnableOption "the fleet's network control server on this box";
    # what the verifier reads to mint join keys, and what a box on this
    # machine joins with; both made once by the seed
    # in the verifier's own state dir: headscale's is its own alone
    apiKeyFile = lib.mkOption {
      type = lib.types.str;
      default = "/var/lib/dd-verify/headscale-api.key";
      readOnly = true;
    };
    boxKeyFile = lib.mkOption {
      type = lib.types.str;
      default = "${state}/box.key";
      readOnly = true;
    };
    url = lib.mkOption {
      type = lib.types.str;
      default = "https://${host}";
      description = "what a device is told to join; plain http on the box's own port in a test";
    };
    behindNginx = lib.mkOption {
      type = lib.types.bool;
      default = true;
      description = "serve the control server on its public name through nginx (off in a test with no certificate)";
    };
  };

  config = lib.mkIf cfg.enable {
    services.headscale = {
      enable = true;
      address = "127.0.0.1";
      inherit port;
      settings = {
        server_url = cfg.url;
        # Tailscale's public relays, until the fleet has a public box
        derp = {
          urls = [ "https://controlplane.tailscale.com/derpmap/default" ];
          server.enabled = false;
        };
        dns = {
          magic_dns = true;
          base_domain = "net.${base}";
          nameservers.global = [
            "1.1.1.1"
            "9.9.9.9"
          ];
          extra_records_path = "${state}/extra-records.json";
        };
        policy = {
          mode = "file";
          path = "${policy}";
        };
        # nodes stay until removed; a laptop off for a month is still yours
        ephemeral_node_inactivity_timeout = "30m";
      };
    };

    # the names file exists before the first start (empty) and is filled
    # once this box has joined
    systemd.tmpfiles.rules = [ "f ${state}/extra-records.json 0644 headscale headscale - []" ];

    # made once: the boxes' user and their join key, and the verifier's api
    # key. Files, not secrets in sops, because this box makes them for
    # itself; node2's copy of the box key travels through sops by hand.
    systemd.services.headscale-seed = {
      description = "Seed the network: the boxes' user and key, the verifier's api key";
      after = [ "headscale.service" ];
      requires = [ "headscale.service" ];
      wantedBy = [ "multi-user.target" ];
      path = [
        config.services.headscale.package
        pkgs.jq
        pkgs.coreutils
      ];
      serviceConfig = {
        Type = "oneshot";
        RemainAfterExit = true;
      };
      script = ''
        set -eu
        for _ in $(seq 1 60); do headscale users list >/dev/null 2>&1 && break; sleep 1; done
        # an empty list is json null here
        who='(. // []) | .[] | select(.name == "boxes") | .id'
        id=$(headscale users list -o json | jq -r "$who")
        if [ -z "$id" ]; then
          headscale users create boxes >/dev/null
          id=$(headscale users list -o json | jq -r "$who")
        fi
        if [ ! -s ${cfg.boxKeyFile} ]; then
          headscale preauthkeys create --user "$id" --reusable --expiration 3650d --tags tag:box > ${cfg.boxKeyFile}.tmp
          chmod 0400 ${cfg.boxKeyFile}.tmp
          mv ${cfg.boxKeyFile}.tmp ${cfg.boxKeyFile}
        fi
        if [ ! -s ${cfg.apiKeyFile} ]; then
          mkdir -p "$(dirname ${cfg.apiKeyFile})"
          headscale apikeys create --expiration 3650d > ${cfg.apiKeyFile}.tmp
          chown root:dd-verify ${cfg.apiKeyFile}.tmp
          chmod 0440 ${cfg.apiKeyFile}.tmp
          mv ${cfg.apiKeyFile}.tmp ${cfg.apiKeyFile}
        fi
      '';
    };

    # the names, once this box is on its own network and has an address
    # there (run by the join keeper). Headscale watches the file.
    dd.net.afterJoin = ''
      ip=$(tailscale --socket ${config.dd.net.socket} ip -4)
      jq -n --arg ip "$ip" '[${
        lib.concatMapStringsSep ", " (n: ''{"name": "${n}", "type": "A", "value": $ip}'') names
      }]' > ${state}/extra-records.json.tmp
      chown headscale:headscale ${state}/extra-records.json.tmp
      mv ${state}/extra-records.json.tmp ${state}/extra-records.json
      echo "the fleet's names answer with $ip on the network"
    '';

    # the gate hands admitted devices their join keys through headscale's api
    dd.verify.network = {
      api = "http://127.0.0.1:${toString port}";
      url = cfg.url;
      keyFile = cfg.apiKeyFile;
    };
    # the verifier starts after the key exists
    systemd.services.dd-verify.after = [ "headscale-seed.service" ];
    systemd.services.dd-verify.wants = [ "headscale-seed.service" ];

    # this box joins its own network with the key it just made: a member's
    # device reaching the fleet arrives here, so this end has to be on it
    dd.net.enable = true;
    dd.net.keyFile = cfg.boxKeyFile;
    dd.net.controlUrl = cfg.url;
    systemd.services.commonty-net-up.after = [ "headscale-seed.service" ];
    systemd.services.commonty-net-up.wants = [ "headscale-seed.service" ];

    # Reached from outside through the front door (the gateway role lists
    # the host as public) and from the boxes by the pinned name. The
    # control protocol is an http upgrade that is not websocket, and the
    # door's proxy strips it: the app carries it over a real websocket
    # (its bridge, app/net/bridge.go), which the server takes as well; a
    # box's stock tailscaled reaches this box directly. Both upgrades pass
    # nginx the same way.
    services.nginx.virtualHosts.${host} = lib.mkIf cfg.behindNginx {
      useACMEHost = base;
      forceSSL = true;
      locations."/" = {
        proxyPass = "http://127.0.0.1:${toString port}";
        proxyWebsockets = true;
        extraConfig = ''
          proxy_buffering off;
          proxy_read_timeout 1h;
        '';
      };
    };
  };
}
