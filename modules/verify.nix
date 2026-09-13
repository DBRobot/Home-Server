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
  port = 4181; # 4180 is oauth2-proxy, which this fronts
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

  config = {
    users.users.${user} = {
      isSystemUser = true;
      group = user;
    };
    users.groups.${user} = { };

    # The verifier answers nginx's auth_request for every protected service on
    # this box. A bearer biscuit is checked against the public keys the user's
    # own devices registered; it holds no key that can sign, so there is nothing
    # in it worth stealing. Everything else is relayed to oauth2-proxy, so
    # browser sessions and legacy kanidm tokens keep working while services move.
    systemd.services.dd-verify = {
      description = "Verify user-signed tokens; relay the rest to oauth2-proxy";
      after = [
        "network-online.target"
      ]
      ++ lib.optional full "oauth2-proxy.service";
      wants = [ "network-online.target" ];
      wantedBy = [ "multi-user.target" ];
      environment = {
        VERIFY_ROLE = cfg.role;
        # full sits behind nginx on localhost. directory has no nginx and no
        # public name yet, so it listens on every interface and the firewall
        # lets only the tailnet in: 4181 is not in allowedTCPPorts.
        VERIFY_BIND = if full then "127.0.0.1:${toString port}" else "0.0.0.0:${toString port}";
        VERIFY_DIR = "/var/lib/dd-verify/keys";
      }
      // lib.optionalAttrs full {
        VERIFY_UPSTREAM_AUTH = "http://127.0.0.1:4180/oauth2/auth";
        # the browser login: passkeys scoped to the whole domain, so one login
        # covers every service on this box and the session cookie rides along
        VERIFY_DOMAIN = base;
        # the per-box issuer for jellyfin, the one service that speaks nothing
        # but oidc. its key is generated on first start and trusted by exactly
        # this client on exactly this box.
        VERIFY_OIDC_ISSUER = "https://jellyfin.${base}/_dd/oidc";
        VERIFY_OIDC_CLIENT_ID = "jellyfin";
        VERIFY_OIDC_CLIENT_SECRET_FILE = config.sops.secrets.jellyfin-oauth-secret.path;
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
    # login and enrolment pages, device registration, and on jellyfin's the
    # per-box issuer. /verify itself stays internal to nginx. A 401 from
    # auth_request lands a browser on the login page and back where it was.
    services.nginx.virtualHosts = lib.mkIf full (
      builtins.listToAttrs (
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
            }
            # the bootstrap for a first browser passkey is an oauth2-proxy login,
            # and its callback is per-host - files and llm already carry the
            # /oauth2/ path, these two need it as well. Added INSIDE locations:
            # `//` on the vhost would replace the whole set, which is exactly the
            # bug that once dropped /_dd/ from these two hosts.
            //
              lib.optionalAttrs
                (builtins.elem h [
                  "grafana"
                  "jellyfin"
                ])
                {
                  "/oauth2/" = {
                    proxyPass = "http://127.0.0.1:4180";
                    extraConfig = "proxy_set_header X-Scheme $scheme;";
                  };
                };
          })
          [
            "files"
            "llm"
            "grafana"
            "jellyfin"
          ]
      )
    );
  };
}
