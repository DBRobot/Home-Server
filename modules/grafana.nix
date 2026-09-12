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
      "auth.generic_oauth" = {
        enabled = true;
        name = "Kanidm";
        client_id = "grafana";
        client_secret = "$__file{${config.sops.secrets.grafana-oauth-secret.path}}";
        scopes = "openid profile email groups";
        auth_url = "https://idm.${base}/ui/oauth2";
        token_url = "https://idm.${base}/oauth2/token";
        api_url = "https://idm.${base}/oauth2/openid/grafana/userinfo";
        use_pkce = true;
        # kanidm returns groups as full spns, hence the contains() rather
        # than a bare equality test
        role_attribute_path = "contains(groups[*], 'admins@idm.${base}') && 'Admin' || 'Viewer'";
      };
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
    };
  };
}
