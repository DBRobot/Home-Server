{ config, ... }:
let
  base = config.dd.domain;
  host = "grafana.${base}";
in
{
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

      # Kanidm is the only way in. Its own login form stays enabled as a
      # break-glass path: if kanidm is down, oidc is down, and locking
      # yourself out of the dashboards that would tell you why is a bad
      # failure mode.
      # No oidc, no kanidm. nginx asks the verifier who this is (a passkey
      # session on this box, or a device-signed token) and passes the name in
      # a header grafana is told to trust from this proxy alone.
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

    provision = {
      enable = true;
      datasources.settings.datasources = [
        {
          name = "prometheus";
          type = "prometheus";
          url = "http://127.0.0.1:9090";
          isDefault = true;
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
        auth_request /oauth2/auth;
        auth_request_set $auth_user $upstream_http_x_auth_request_preferred_username;
        proxy_set_header X-WEBAUTH-USER $auth_user;
        error_page 401 = @login;
      '';
    };
    locations."= /oauth2/auth" = {
      proxyPass = "http://127.0.0.1:4181/verify";
      extraConfig = ''
        internal;
        proxy_pass_request_body off;
        proxy_set_header Content-Length "";
        proxy_set_header X-Original-URI $request_uri;
        client_max_body_size 0;
      '';
    };
  };
}
