{
  config,
  pkgs,
  self,
  ...
}:
let
  base = config.dd.domain;
  host = "signup.${base}";
  port = 8083; # 8080 museum, 8081 llama, 8443 kanidm, 4180 oauth2-proxy
  user = "signup";
in
{
  users.users.${user} = {
    isSystemUser = true;
    group = user;
  };
  users.groups.${user} = { };

  systemd.services.signup = {
    description = "Self-service account signup";
    after = [
      "kanidm.service"
      "kanidm-service-accounts.service"
      "network-online.target"
    ];
    requires = [
      "kanidm.service"
      "kanidm-service-accounts.service" # the account the sops token belongs to
    ];
    wants = [ "network-online.target" ];
    wantedBy = [ "multi-user.target" ];
    environment = {
      SIGNUP_BIND = "127.0.0.1:${toString port}";
      # The public name rather than 127.0.0.1:8443 - kanidm's cert is issued for
      # this name, and going back in through nginx is what makes it verify.
      SIGNUP_KANIDM_URL = "https://idm.${base}";
      SIGNUP_TOKEN_FILE = config.sops.secrets.signup-api-token.path;
      SIGNUP_GROUP = "pending";
      SIGNUP_PHOTOS_URL = "https://photos.${base}";
      SIGNUP_INTENT_TTL = "86400"; # a day, so an evening signup survives til morning
    };
    serviceConfig = {
      Type = "simple";
      User = user;
      Group = user;
      ExecStart = "${self.packages.${pkgs.stdenv.hostPlatform.system}.signup}/bin/signup";
      Restart = "on-failure";
      RestartSec = 5;

      # It holds a token that can create accounts, so it gets no more of the
      # machine than it needs: no writable paths at all, and no way to gain any.
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

  # A signup endpoint with no human approving anything is exactly the shape that
  # gets hammered, so the limit lives in nginx rather than in the service: it
  # costs nothing and it applies before a request reaches anything that can
  # write to kanidm.
  services.nginx.appendHttpConfig = ''
    limit_req_zone $binary_remote_addr zone=signup:10m rate=6r/m;
  '';

  services.nginx.virtualHosts.${host} = {
    useACMEHost = base;
    forceSSL = true;

    locations."/" = {
      proxyPass = "http://127.0.0.1:${toString port}";
      extraConfig = ''
        # burst lets a person retype a rejected form without being blocked,
        # nodelay so the retry is answered rather than queued
        limit_req zone=signup burst=4 nodelay;
        limit_req_status 429;
      '';
    };

    locations."= /health" = {
      proxyPass = "http://127.0.0.1:${toString port}/health";
      extraConfig = "access_log off;";
    };
  };
}
