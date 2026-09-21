{
  config,
  lib,
  pkgs,
  self,
  ...
}:
let
  cfg = config.dd.verify;
  full = cfg.role == "full";
  base = config.dd.domain;
  port = 4181;
  user = "dd-verify";
in
{
  # A box that runs services verifies for them and serves its users' browser
  # login. A box that runs nothing else can still hold the directory: the
  # signed entries are self-authenticating, so a copy on a stranger's box is
  # worth exactly as much as the one here and costs that box no secret.
  options.dd.verify.role = lib.mkOption {
    type = lib.types.enum [
      "full"
      "directory"
    ];
    default = "full";
    description = "full: auth_request for the services on this box plus the directory; directory: the directory alone, on the tailnet.";
  };
  # The other boxes' directories. Pulled every five minutes through the same
  # accept rule a client's update gets; a new name from the network is taken
  # only once every peer has been pulled and none knows it under another
  # root. A box with no peers is on its own and takes new names at once.
  options.dd.verify.syncSeconds = lib.mkOption {
    type = lib.types.int;
    default = 300;
    description = "How often a box pulls its peers' directories. Tests set it low.";
  };
  options.dd.verify.peers = lib.mkOption {
    type = lib.types.listOf lib.types.str;
    default = [ ];
    description = "Directory urls of the other boxes, e.g. https://files.example/_dd/directory.";
  };
  # the front door: one tile per service this box offers, declared by the
  # roles that run them, shown at home.<domain> to whoever is signed in
  options.dd.home.services = lib.mkOption {
    type = lib.types.listOf (
      lib.types.submodule {
        options = {
          name = lib.mkOption { type = lib.types.str; };
          url = lib.mkOption { type = lib.types.str; };
          description = lib.mkOption { type = lib.types.str; };
          icon = lib.mkOption {
            type = lib.types.str;
            default = "";
            description = "photos, videos, files, chat, code or metrics; anything else draws a plain mark";
          };
          color = lib.mkOption { type = lib.types.str; };
          rank = lib.mkOption {
            type = lib.types.int;
            default = 50;
            description = "tiles are shown in rank order";
          };
          demo = lib.mkOption {
            type = lib.types.nullOr lib.types.str;
            default = null;
            description = "What the demo account may do on this tile's host, enforced at the gate: `full` (the service's own permissions are the limit), `read` (no writing method), `rate:N` (reads free, N other requests an hour). Null: nothing; the tile is greyed for it. The demo exists when any tile grants something.";
          };
        };
      }
    );
    default = [ ];
  };

  options.dd.members = lib.mkOption {
    type = lib.types.attrsOf (lib.types.listOf lib.types.str);
    description = "fleet/members.json: `members`, ids let in, and `revoked`, ids shut out (dd member add|remove). Comes with the signed release; a box cannot add to it.";
    default = {
      members = [ ];
      revoked = [ ];
    };
  };
  options.dd.verify.photos = lib.mkOption {
    type = lib.types.nullOr (
      lib.types.submodule {
        options = {
          api = lib.mkOption {
            type = lib.types.str;
            description = "museum's address, e.g. https://api.<domain>";
          };
          emailSuffix = lib.mkOption {
            type = lib.types.str;
            description = "the address suffix museum takes our verification code for; a person's ente address is <name><suffix>";
          };
          codeFile = lib.mkOption {
            type = lib.types.str;
            description = "file holding that code, readable by the verifier";
          };
        };
      }
    );
    default = null;
    description = "Photos opened by passkey: the ente account made and opened in the browser (pages::photos). Set by the photos role.";
  };
  options.dd.verify.releasePublicKey = lib.mkOption {
    type = lib.types.nullOr lib.types.str;
    description = "The release key, base64: what signs an invite (dd invite). Every box holds invites; the full one checks grants against this.";
    default = null;
  };

  config = {
    users.users.${user} = {
      isSystemUser = true;
      group = user;
    };
    users.groups.${user} = { };

    # The verifier answers nginx's auth_request for every protected service on
    # this box. A bearer biscuit is checked against the devices in the user's
    # own signed entry; a cookie against the passkeys enrolled here. It holds
    # no key that can sign, so there is nothing in it worth stealing, and it
    # asks no identity server because there is none.
    systemd.services.dd-verify = {
      description = "Verify user-signed tokens and this box's passkey sessions";
      after = [
        "network-online.target"
      ];
      wants = [ "network-online.target" ];
      wantedBy = [ "multi-user.target" ];
      environment = {
        VERIFY_ROLE = cfg.role;
        # full sits behind nginx on localhost. directory has no nginx and no
        # public name yet, so it listens on every interface and the firewall
        # lets only the tailnet in: 4181 is not in allowedTCPPorts.
        VERIFY_BIND = if full then "127.0.0.1:${toString port}" else "0.0.0.0:${toString port}";
        VERIFY_DIR = "/var/lib/dd-verify/keys";
        VERIFY_PEERS = lib.concatStringsSep "," cfg.peers;
        VERIFY_SYNC_SECS = toString cfg.syncSeconds;
      }
      // lib.optionalAttrs (full && cfg.photos != null) {
        VERIFY_PHOTOS_API = cfg.photos.api;
        VERIFY_PHOTOS_SUFFIX = cfg.photos.emailSuffix;
        VERIFY_PHOTOS_CODE_FILE = cfg.photos.codeFile;
      }
      // lib.optionalAttrs (cfg.releasePublicKey != null) {
        VERIFY_RELEASE_PUB = cfg.releasePublicKey;
      }
      // lib.optionalAttrs full {
        # the browser login: passkeys scoped to the whole domain, so one login
        # covers every service on this box and the session cookie rides along
        VERIFY_DOMAIN = base;
        VERIFY_MEMBERS = builtins.toJSON config.dd.members;
        # our Rust for the browser, next to the pages that use it
        VERIFY_WEB_DIR = "${self.packages.${pkgs.stdenv.hostPlatform.system}.web}";
        # the per-box issuer for jellyfin, the one service that speaks nothing
        # but oidc. its key is generated on first start and trusted by exactly
        # this client on exactly this box.
        VERIFY_OIDC_ISSUER = "https://jellyfin.${base}/_dd/oidc";
        VERIFY_OIDC_CLIENT_ID = "jellyfin";
        VERIFY_OIDC_CLIENT_SECRET_FILE = config.sops.secrets.jellyfin-oauth-secret.path;
        VERIFY_HOME = builtins.toJSON (
          map (t: {
            inherit (t)
              name
              url
              description
              icon
              color
              demo
              ;
          }) (
            lib.sort (a: b: a.rank < b.rank) config.dd.home.services
          )
        );
        VERIFY_OIDC_REDIRECT = "https://jellyfin.${base}/sso/OID/r/dd";
      };
      serviceConfig = {
        Type = "simple";
        User = user;
        Group = user;
        StateDirectory = "dd-verify";
        StateDirectoryMode = "0700";
        ExecStart = "${self.packages.${pkgs.stdenv.hostPlatform.system}.verify}/bin/verify";
        Restart = "on-failure";
        RestartSec = 5;
        NoNewPrivileges = true;
        PrivateTmp = true;
        PrivateDevices = true;
        ProtectSystem = "strict";
        ProtectHome = true;
        ProtectKernelTunables = true;
        ProtectKernelModules = true;
        ProtectControlGroups = true;
        RestrictAddressFamilies = [
          "AF_INET"
          "AF_INET6"
        ];
        RestrictNamespaces = true;
        LockPersonality = true;
        MemoryDenyWriteExecute = true;
        SystemCallArchitectures = "native";
        SystemCallFilter = [ "@system-service" ];
      };
    };

    # The verifier's browser side on every vhost that has one: the passkey
    # login and enrolment pages, the directory, and on jellyfin's the per-box
    # issuer. /_dd/verify is the auth_request target and internal to nginx. A
    # 401 from auth_request lands a browser on the login page and back where
    # it was.
    services.nginx.virtualHosts = lib.mkIf full (
      lib.mkMerge [
        (builtins.listToAttrs (
        map
          (h: {
            name = "${h}.${base}";
            value.locations = {
              # No limit_req here. jellyfin fetches discovery and jwks from this
              # very box in quick succession, and a per-ip limit that counted those
              # answered its sso with 503 the moment anything else probed /_dd/.
              # Every abusable endpoint there demands a credential first.
              "/_dd/" = {
                proxyPass = "http://127.0.0.1:${toString port}/_dd/";
                extraConfig = ''
                  proxy_set_header X-Original-URI $request_uri;
                '';
              };
              "@login".extraConfig = ''
                return 302 /_dd/login?rd=$request_uri;
              '';
              # signed in, not a member: the home page says so
              "@waiting".extraConfig = ''
                return 302 https://home.${base}/;
              '';
              # the subrequest every protected location makes
              "= /_dd/verify" = {
                proxyPass = "http://127.0.0.1:${toString port}/verify";
                extraConfig = ''
                  internal;
                  proxy_pass_request_body off;
                  proxy_set_header Content-Length "";
                  proxy_set_header X-Original-URI $request_uri;
                  # the demo's leash is held in the verifier: it needs the
                  # method and, via Host, where the request was going
                  proxy_set_header X-Original-Method $request_method;
                  proxy_set_header X-Original-Host $host;
                  # The subrequest inherits nothing from the location that
                  # made it, so it ran with nginx's default 1m body limit.
                  # When a large PUT was still arriving as the auth phase ran,
                  # discarding it tripped that limit: "auth request unexpected
                  # status: 413", surfaced as a 500. The limit belongs on the
                  # dav location; here it must not exist.
                  client_max_body_size 0;
                '';
              };
            };
          })
          [
            "files"
            "llm"
            "grafana"
            "jellyfin"
            "git"
            "home"
            "photos"
          ]
        ))
        # the front door itself: home.<domain> is the verifier's page and
        # nothing else. The bare domain cannot carry it (the certificate is
        # the wildcard alone), so the gateway sends it here.
        {
          "home.${base}" = {
            useACMEHost = base;
            forceSSL = true;
            locations."= /" = {
              proxyPass = "http://127.0.0.1:${toString port}/_dd/home";
              extraConfig = "proxy_set_header X-Original-URI $request_uri;";
            };
          };
        }
      ]
    );
  };
}
