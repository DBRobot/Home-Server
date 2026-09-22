# The front door on the open internet, or not. Off, the fleet is reachable
# on the tailnet only and the name points at the gateway's tailnet address.
# On, the name points at the box's public address (kept current), 80 and
# 443 open on the box's other interfaces, and only the hosts listed here
# answer there: the rest stay tailnet-only however the name resolves,
# because nginx is told which server names take a public connection.
{
  config,
  pkgs,
  lib,
  ...
}:
let
  cfg = config.dd.public;
  base = config.dd.domain;
in
{
  options.dd.public = {
    enable = lib.mkEnableOption "the front door on the open internet";
    hosts = lib.mkOption {
      type = lib.types.listOf lib.types.str;
      default = [
        "home"
        "accounts"
      ];
      description = "the subdomains that answer a connection from outside the tailnet: the gate's own pages (sign-in, sign-up, the demo) and what they need. Everything else is tailnet-only.";
    };
    duckdnsDomain = lib.mkOption {
      type = lib.types.str;
      description = "the duckdns name, without .duckdns.org";
    };
    tokenFile = lib.mkOption {
      type = lib.types.path;
      description = "file holding DUCKDNS_TOKEN";
    };
  };

  config = {
    # the tailnet's own view of the names. With the door open the name
    # resolves to the public address for everyone, and a member's browser
    # would knock on the public side of its own gateway and be refused for
    # every host but the public ones. So a member asks the gateway instead:
    # Tailscale's split DNS sends <base> to this box's tailnet address (the
    # one setting in the admin console), and this answers every name under
    # it with the tailnet address. Nothing else is answered or forwarded;
    # the boxes themselves have the same answer in /etc/hosts (flake.nix).
    services.dnsmasq = {
      enable = true;
      resolveLocalQueries = false;
      settings = {
        interface = "tailscale0";
        except-interface = "lo";
        bind-dynamic = true;
        no-resolv = true;
        no-hosts = true;
        address = [ "/${base}/${config.dd.box.tailnet}" ];
      };
    };

    # the name follows the switch: the public address when on, the tailnet
    # address when off. Every ten minutes, and at every switch, so flipping
    # takes effect within the ttl.
    systemd.services.dd-dns = {
      description = "Point the fleet's name where the front door is";
      wantedBy = [ "multi-user.target" ];
      after = [ "network-online.target" ];
      wants = [ "network-online.target" ];
      serviceConfig = {
        Type = "oneshot";
        EnvironmentFile = cfg.tokenFile;
        DynamicUser = true;
      };
      path = [
        pkgs.curl
        pkgs.iproute2
      ];
      script = ''
        set -euo pipefail
        if [ "${lib.boolToString cfg.enable}" = true ]; then
          # what the world sees this box as
          ip=$(curl -fsS -4 --max-time 10 https://api.ipify.org)
        else
          ip=${lib.escapeShellArg config.dd.box.tailnet}
        fi
        out=$(curl -fsS --max-time 10 "https://www.duckdns.org/update?domains=${cfg.duckdnsDomain}&token=$DUCKDNS_TOKEN&ip=$ip")
        [ "$out" = OK ] || { echo "duckdns said: $out" >&2; exit 1; }
        echo "${base} -> $ip"
      '';
    };
    systemd.timers.dd-dns = {
      wantedBy = [ "timers.target" ];
      timerConfig = {
        OnBootSec = "2min";
        OnUnitActiveSec = "10min";
      };
    };

    # the door itself: 80 and 443 on every interface only when on (the
    # tailnet interface is trusted already)
    networking.firewall.allowedTCPPorts = lib.mkIf cfg.enable [
      80
      443
    ];

    # a connection from outside the tailnet gets only the public hosts. The
    # map says, per connection and host, whether the door is shut; every
    # server block checks it first (commonHttpConfig applies to all), so a
    # public connection to a tailnet-only name gets a closed connection,
    # not a page, and nothing behind the gate is reachable however the name
    # resolves.
    services.nginx.appendHttpConfig = lib.mkIf cfg.enable ''
        # per address: a ceremony a second, ten in hand
        limit_req_zone $binary_remote_addr zone=dd_ceremony:1m rate=1r/s;
        # and a lid on how many connections one address holds open
        limit_conn_zone $binary_remote_addr zone=dd_conn:1m;
      geo $dd_inside {
        default 0;
        100.64.0.0/10 1;
        127.0.0.0/8 1;
        ::1 1;
      }
      map "$dd_inside:$host" $dd_shut {
        default 1;
        "~^1:" 0;
        ${lib.concatMapStringsSep "\n  " (h: ''"~^0:${h}\\.${lib.escapeRegex base}$" 0;'') cfg.hosts}
      }
    '';
    services.nginx.virtualHosts = lib.mkIf cfg.enable (
      lib.genAttrs (map (h: "${h}.${base}") config.dd.verify.hosts) (_: {
        extraConfig = lib.mkBefore ''
          if ($dd_shut) { return 444; }
          # what a public connection may send: pages and json, never a
          # photo (uploads go to services that are not public); and a lid
          # on how many connections one address holds open
          client_max_body_size 1m;
          client_body_timeout 15s;
          client_header_timeout 15s;
          limit_conn dd_conn 20;
        '';
        # the ceremonies a stranger may start: sign-up, sign-in, enrol, a
        # code. A person clicks these a few times; a script gets told to
        # wait. The rest of /_dd/ stays unlimited (jellyfin's sso polls it)
        locations."~ ^/_dd/(join|login|enrol|redeem)/" = {
          proxyPass = "http://127.0.0.1:${toString config.dd.verify.port}";
          extraConfig = ''
            limit_req zone=dd_ceremony burst=10 nodelay;
            limit_req_status 429;
            proxy_set_header X-Original-URI $request_uri;
          '';
        };
      })
    );
  };
}
