{
  config,
  pkgs,
  lib,
  ...
}:
let
  base = config.dd.domain;
  host = "git.${base}";
  port = 3001; # 3000 is grafana
  cfg = config.dd.forgejo;
in
{
  # The one account that administers the forge. Every other account is made
  # on first visit by the reverse-proxy login below; this one is made ahead
  # of time so that the same login lands on an admin.
  options.dd.forgejo.admin = lib.mkOption {
    type = lib.types.str;
    description = "Directory name of the person who administers this forge.";
  };

  config = {
    # Private repositories here are private from the world, not from whoever
    # runs this box. The forge reads the code; that is what a forge is. What
    # must be private from the box too goes through the dd remote helper,
    # which stores ciphertext in an ordinary repository here.
    services.forgejo = {
      enable = true;
      package = pkgs.forgejo-lts;
      # on the pool, with snapshots: this is people's work
      stateDir = "/vault/forgejo";
      database.type = "postgres"; # the instance ente already runs; socket auth
      lfs.enable = true;
      settings = {
        DEFAULT.APP_NAME = "Distributed Datacenter";
        server = {
          DOMAIN = host;
          ROOT_URL = "https://${host}/";
          HTTP_ADDR = "127.0.0.1";
          HTTP_PORT = port;
          # git over ssh goes through the box's own sshd on 22: forgejo keeps
          # the forgejo user's authorized_keys, sshd does the rest
          START_SSH_SERVER = false;
          SSH_DOMAIN = host;
          SSH_PORT = 22;
          LANDING_PAGE = "explore";
        };
        # Nobody signs up here and nobody has a password here. nginx asks the
        # verifier who this is and says so in a header; forgejo trusts that
        # header from this box alone and makes the account on first sight.
        service = {
          DISABLE_REGISTRATION = true;
          ENABLE_REVERSE_PROXY_AUTHENTICATION = true;
          ENABLE_REVERSE_PROXY_AUTHENTICATION_API = true;
          ENABLE_REVERSE_PROXY_AUTO_REGISTRATION = true;
          ENABLE_REVERSE_PROXY_EMAIL = false;
          ENABLE_PASSWORD_SIGNIN_FORM = false;
          REQUIRE_SIGNIN_VIEW = false; # public repos are public
          DEFAULT_KEEP_EMAIL_PRIVATE = true;
          NO_REPLY_ADDRESS = "noreply.${base}";
        };
        security = {
          REVERSE_PROXY_AUTHENTICATION_USER = "X-WEBAUTH-USER";
          REVERSE_PROXY_TRUSTED_PROXIES = "127.0.0.0/8,::1";
          REVERSE_PROXY_LIMIT = 1;
        };
        session.COOKIE_SECURE = true;
        repository = {
          DEFAULT_PRIVATE = "public";
          DEFAULT_PUSH_CREATE_PRIVATE = false;
          ENABLE_PUSH_CREATE_USER = true;
        };
        mailer.ENABLED = false;
        actions.ENABLED = true; # the runner is modules/forgejo-runner.nix
        "ui.meta".DESCRIPTION = "git for the distributed datacenter";
        other.SHOW_FOOTER_VERSION = false;
      };
    };

    # The state dir is a dataset (modules/zfs-datasets.nix); do not start
    # before it is mounted, or forgejo would happily initialise itself onto
    # the root disk underneath the mountpoint.
    systemd.services.forgejo = {
      after = [ "zfs-datasets.service" ];
      requires = [ "zfs-datasets.service" ];
      unitConfig.RequiresMountsFor = "/vault/forgejo";
    };

    # The admin account, made before it is ever logged into. The password is
    # random and never used: the password form is off, the header is the
    # login. Idempotent - "already exists" is the normal case.
    systemd.services.forgejo-admin = {
      description = "Make sure the forge's admin account exists";
      after = [ "forgejo.service" ];
      requires = [ "forgejo.service" ];
      wantedBy = [ "multi-user.target" ];
      path = [ pkgs.forgejo-lts ];
      environment = {
        USER = "forgejo";
        HOME = "/vault/forgejo";
        FORGEJO_WORK_DIR = "/vault/forgejo";
        FORGEJO_CUSTOM = "/vault/forgejo/custom";
      };
      serviceConfig = {
        Type = "oneshot";
        User = "forgejo";
        Group = "forgejo";
      };
      script = ''
        for i in $(seq 1 30); do
          ${pkgs.curl}/bin/curl -fs http://127.0.0.1:${toString port}/api/healthz >/dev/null && break
          sleep 2
        done
        if forgejo admin user list --admin | ${pkgs.gnugrep}/bin/grep -qw '${cfg.admin}'; then
          exit 0
        fi
        forgejo admin user create --admin --username '${cfg.admin}' \
          --email '${cfg.admin}@${config.services.forgejo.settings.service.NO_REPLY_ADDRESS}' \
          --random-password --must-change-password=false >/dev/null
        echo "admin ${cfg.admin} created"
      '';
    };

    services.nginx.virtualHosts.${host} = {
      useACMEHost = base;
      forceSSL = true;
      locations."/" = {
        proxyPass = "http://127.0.0.1:${toString port}";
        extraConfig = ''
          client_max_body_size 0; # pushes over https, lfs
          # who is this? the verifier says. The header the client sent is
          # replaced either way, so nobody names themselves.
          auth_request /_dd/verify;
          auth_request_set $auth_user $upstream_http_x_auth_request_preferred_username;
          proxy_set_header X-WEBAUTH-USER $auth_user;
          # nobody: still let them in, as nobody. Public repos are public and
          # forgejo shows private ones to no one it does not know.
          error_page 401 = @anonymous;
        '';
      };
      locations."@anonymous" = {
        proxyPass = "http://127.0.0.1:${toString port}";
        extraConfig = ''
          client_max_body_size 0;
          proxy_set_header X-WEBAUTH-USER "";
        '';
      };
      # forgejo's own sign-in page is a password form that is switched off;
      # the box's passkey page is the sign-in
      locations."= /user/login".extraConfig = ''
        return 302 /_dd/login?rd=$arg_redirect_to;
      '';
    };
  };
}
