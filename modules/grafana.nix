{ config, ... }:
let
  base = "distributed-datacenter.duckdns.org";
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
      # Infrastructure, not family-facing: it stays on the tailnet, so
      # anonymous access is fine until kanidm oidc is wired up.
      analytics.reporting_enabled = false;
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
