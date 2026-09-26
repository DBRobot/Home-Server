{ config, lib, ... }:
let
  base = config.dd.domain;
  host = "grafana.${base}";
  sock = "/run/grafana/grafana.sock";
in
{
  config = {
    # reads plaintext: only on a box whose owner is trusted with it (modules/box.nix)
    dd.box.plaintext = [ "grafana (metrics)" ];

    services.grafana = {
      enable = true;
      settings = {
        # A unix socket, not a port. auth.proxy believes the header it is
        # given, and grafana has no way to tell nginx from anything else on
        # this box - which runs CI jobs and game guests. A socket nginx
        # alone can open is the way to say "from nginx" that holds.
        server = {
          protocol = "socket";
          socket = sock;
          socket_mode = "0660";
          domain = host;
          root_url = "https://${host}/";
        };
        analytics.reporting_enabled = false;

        # No oidc. nginx asks the verifier who this is (a passkey session on
        # this box, or a device-signed token) and passes the name in a header
        # grafana trusts because nothing else can reach the socket.
        "auth.proxy" = {
          enabled = true;
          header_name = "X-WEBAUTH-USER";
          header_property = "username";
          auto_sign_up = true;
          enable_login_token = false;
        };
        # disable_login_form only hides the form. Grafana's built-in admin
        # keeps whatever password it was created with - nixpkgs' default is
        # published - and basic auth answers it whether or not a form is
        # drawn. The account is never wanted here: nginx says who you are.
        auth.disable_login_form = true;
        "auth.basic".enabled = false;
        security.disable_initial_admin_creation = true;
        # $__file{} is grafana's own indirection, so the key never enters the
        # nix store - same property as every other secret here.
        security.secret_key = "$__file{${config.sops.secrets.grafana-secret-key.path}}";
      };

      # One datasource: every box through thanos on this box, told apart by
      # the box label. There used to be one per box, grafana reaching into
      # each box's prometheus over the tailnet - a box talking to a box for
      # no reason the fleet view does not already cover.
      provision = {
        enable = true;
        datasources.settings = {
          # what is not listed here goes, including the per-box ones
          prune = true;
          datasources = lib.optional config.services.thanos.query.enable {
            name = "fleet";
            uid = "fleet";
            type = "prometheus";
            url = "http://127.0.0.1:10903";
            isDefault = true;
            jsonData.timeInterval = "15s";
          };
        };
        dashboards.settings.providers = [
          {
            name = "dd";
            options.path = ./dashboards;
          }
        ];
      };
    };

    # grafana makes the socket; nginx needs the group to open it, and the
    # directory keeps anything else on the box out of the path entirely
    systemd.services.grafana.serviceConfig.RuntimeDirectoryMode = lib.mkForce "0750";
    # the whole attribute, not just its value: naming users.users.nginx
    # where nginx does not run leaves a user with no group and no kind
    users.users = lib.mkIf config.services.nginx.enable {
      nginx.extraGroups = [ "grafana" ];
    };
    services.nginx.virtualHosts.${host} = {
      useACMEHost = base;
      forceSSL = true;
      locations."/" = {
        proxyPass = "http://unix:${sock}:";
        proxyWebsockets = true; # live dashboards
        extraConfig = ''
          # who is this? the verifier says, from a passkey session on this box
          # or a device-signed token. grafana listens on a socket in a
          # directory only nginx and grafana may enter, so nothing else on
          # this box can set the header for itself.
          auth_request /_dd/verify;
          auth_request_set $auth_user $upstream_http_x_auth_request_preferred_username;
          proxy_set_header X-WEBAUTH-USER $auth_user;
          # the name is the whole credential here; the token that proved it
          # has no business going any further
          proxy_set_header Authorization "";
          proxy_set_header Cookie $dd_cookie_stripped;
          error_page 401 = @login;
          error_page 403 = @waiting;
        '';
      };
    };
  };
}
