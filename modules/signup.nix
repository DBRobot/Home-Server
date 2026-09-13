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
  worker = "signup-worker";
  spool = "/var/lib/signup/spool";
  bin = "${self.packages.${pkgs.stdenv.hostPlatform.system}.signup}/bin/signup";

  # Everything both halves share. The one thing they must NOT share is the
  # token, so that is set per unit below.
  env = {
    SIGNUP_KANIDM_URL = "https://idm.${base}";
    SIGNUP_GROUP = "pending";
    SIGNUP_PHOTOS_URL = "https://photos.${base}";
    SIGNUP_INTENT_TTL = "86400"; # a day, so an evening signup survives til morning
    SIGNUP_SPOOL = spool;
  };
  hardening = {
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
    ReadWritePaths = [ spool ];
    Restart = "on-failure";
    RestartSec = 5;
  };
in
{
  # Two users, one group. The page writes requests into the spool and the
  # worker consumes them; the setgid directory keeps every file in the shared
  # group whichever side made it.
  users.users.${user} = {
    isSystemUser = true;
    group = user;
  };
  users.users.${worker} = {
    isSystemUser = true;
    group = user;
  };
  users.groups.${user} = { };

  systemd.tmpfiles.rules = [
    "d /var/lib/signup 0750 ${worker} ${user} -"
    "d ${spool} 2770 ${worker} ${user} -"
  ];

  systemd.services.signup = {
    description = "Self-service account signup (the page; read-only token)";
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
    environment = env // {
      SIGNUP_BIND = "127.0.0.1:${toString port}";
      SIGNUP_TOKEN_FILE = config.sops.secrets.signup-readonly-token.path;
    };
    serviceConfig = hardening // {
      Type = "simple";
      User = user;
      Group = user;
      UMask = "0007"; # spool files must be group-readable for the worker
      ExecStart = "${bin} serve";
    };
  };

  # The privileged half. No listener, no port: it reads the spool and talks to
  # kanidm, nothing else, and it re-checks every request before acting.
  systemd.services.signup-worker = {
    description = "Self-service account signup (the worker; read-write token)";
    after = [
      "kanidm.service"
      "kanidm-service-accounts.service"
      "network-online.target"
    ];
    requires = [
      "kanidm.service"
      "kanidm-service-accounts.service"
    ];
    wants = [ "network-online.target" ];
    wantedBy = [ "multi-user.target" ];
    environment = env // {
      SIGNUP_TOKEN_FILE = config.sops.secrets.signup-api-token.path;
    };
    serviceConfig = hardening // {
      Type = "simple";
      User = worker;
      Group = user;
      ExecStart = "${bin} work";
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
