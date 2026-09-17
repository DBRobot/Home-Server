{ config, ... }:
{
  # A box with a public name: certificates, nginx, the browser login and the
  # per-box issuer for jellyfin. The verifier's full role lives here.
  imports = [
    ./_sops.nix
    ../modules/acme.nix
  ];
  dd.verify.role = "full";
  services.tailscale.permitCertUid = "nginx"; # so nginx can fetch *.ts.net certs without root

  sops.secrets.duckdns-token = { };
  # read by the verifier's per-box oidc issuer for jellyfin; the seed script
  # in modules/jellyfin.nix runs as root
  sops.secrets.jellyfin-oauth-secret.owner = "dd-verify";
  sops.templates."duckdns.env".content = ''
    DUCKDNS_TOKEN=${config.sops.placeholder.duckdns-token}
  '';

  # the bare domain: no certificate covers it (duckdns allows one TXT record,
  # the wildcard has it), so plain http answers with where the front door is
  services.nginx.virtualHosts.${config.dd.domain} = {
    listen = [
      {
        addr = "0.0.0.0";
        port = 80;
      }
    ];
    locations."/".return = "301 https://home.${config.dd.domain}$request_uri";
  };

  # garage's public face: ente's browser uploads and any other s3 client
  # reach the cluster through the gateway's name
  services.nginx.virtualHosts."s3.${config.dd.domain}" = {
    useACMEHost = config.dd.domain;
    forceSSL = true;
    locations."/" = {
      proxyPass = "http://127.0.0.1:3900";
      extraConfig = ''
        client_max_body_size 0;
        proxy_request_buffering off;
      '';
    };
  };
}
