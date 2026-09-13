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

  # Device registration, reachable from a client. Nothing else is exposed:
  # /verify stays internal to nginx.
  services.nginx.virtualHosts."files.${base}".locations."/_dd/register" = {
    proxyPass = "http://127.0.0.1:${toString port}/register";
    extraConfig = ''
      limit_req zone=signup burst=4 nodelay;
    '';
  };
}
