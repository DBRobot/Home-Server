{ config, lib, ... }:
let
  base = config.dd.domain;
  host = "grafana.${base}";
  cfg = config.dd.grafana;
in
{
  options.dd.grafana.boxes = lib.mkOption {
    type = lib.types.attrsOf lib.types.str;
    default = { };
    description = "Every box's prometheus, by box name, as a url grafana can reach.";
  };

  config = {
    # reads plaintext: only on a box whose owner is trusted with it (modules/box.nix)
    dd.box.plaintext = [ "grafana (metrics)" ];

    services.grafana = {
      enable = true;
      settings = {
        server = {
          http_addr = "127.0.0.1";
          http_port = 3000;
          domain = host;
          root_url = "https://${host}/";
        };
        analytics.reporting_enabled = false;

        # No oidc. nginx asks the verifier who this is (a passkey session on
        # this box, or a device-signed token) and passes the name in a header
        # grafana is told to trust from this proxy alone.
        "auth.proxy" = {
          enabled = true;
          header_name = "X-WEBAUTH-USER";
          header_property = "username";
          auto_sign_up = true;
          enable_login_token = false;
        };
        auth.disable_login_form = true;
        # $__file{} is grafana's own indirection, so the key never enters the
        # nix store - same property as every other secret here.
        security.secret_key = "$__file{${config.sops.secrets.grafana-secret-key.path}}";
      };

      # One datasource per box, each box's own prometheus over the tailnet:
      # nothing is scraped centrally, this reads. The list is the box list,
      # by hand until the box list is data. The "Box" dashboard picks one.
      provision = {
        enable = true;
        datasources.settings = {
          # datasources.yaml is authoritative: a box that leaves the list is
          # removed here too
          deleteDatasources = [
            {
              name = "prometheus";
              orgId = 1;
            }
          ];
          datasources =
            lib.mapAttrsToList (name: url: {
              inherit name url;
              type = "prometheus";
              uid = name;
              isDefault = name == "node1";
              jsonData.timeInterval = "15s";
            }) cfg.boxes
            # every box at once, through thanos: the box label tells them apart
            ++ lib.optional config.services.thanos.query.enable {
              name = "fleet";
              uid = "fleet";
              type = "prometheus";
              url = "http://127.0.0.1:10903";
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

    services.nginx.virtualHosts.${host} = {
      useACMEHost = base;
      forceSSL = true;
      locations."/" = {
        proxyPass = "http://127.0.0.1:3000";
        proxyWebsockets = true; # live dashboards
        extraConfig = ''
          # who is this? the verifier says, from a passkey session on this box
          # or a device-signed token. grafana trusts the header from this proxy
          # only (auth.proxy above), so nothing else can set it.
          auth_request /_dd/verify;
          auth_request_set $auth_user $upstream_http_x_auth_request_preferred_username;
          proxy_set_header X-WEBAUTH-USER $auth_user;
          error_page 401 = @login;
        '';
      };
    };
  };
}
