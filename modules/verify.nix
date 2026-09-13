{
  config,
  pkgs,
  self,
  ...
}:
let
  base = config.dd.domain;
  port = 4181; # 4180 is oauth2-proxy, which this fronts
  user = "dd-verify";
in
{
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
      "oauth2-proxy.service"
    ];
    wants = [ "network-online.target" ];
    wantedBy = [ "multi-user.target" ];
    environment = {
      VERIFY_BIND = "127.0.0.1:${toString port}";
      VERIFY_DIR = "/var/lib/dd-verify/keys";
      VERIFY_UPSTREAM_AUTH = "http://127.0.0.1:4180/oauth2/auth";
      # the id token that bootstraps a user's first device is the dd client's
      VERIFY_ISSUER = "https://idm.${base}/oauth2/openid/dd";
      VERIFY_AUDIENCE = "dd";
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
  services.nginx.virtualHosts = builtins.listToAttrs (
    map
      (h: {
        name = "${h}.${base}";
        value = {
          locations."/_dd/" = {
            proxyPass = "http://127.0.0.1:${toString port}/_dd/";
            extraConfig = ''
              limit_req zone=signup burst=8 nodelay;
              proxy_set_header X-Original-URI $request_uri;
            '';
          };
          locations."@login".extraConfig = ''
            return 302 /_dd/login?rd=$request_uri;
          '';
        }
        // (
          # the bootstrap for a first browser passkey is an oauth2-proxy login,
          # and its callback is per-host - files and llm already carry the
          # /oauth2/ path, these two need it as well
          if
            builtins.elem h [
              "grafana"
              "jellyfin"
            ]
          then
            {
              locations."/oauth2/" = {
                proxyPass = "http://127.0.0.1:4180";
                extraConfig = "proxy_set_header X-Scheme $scheme;";
              };
            }
          else
            { }
        );
      })
      [
        "files"
        "llm"
        "grafana"
        "jellyfin"
      ]
  );
}
