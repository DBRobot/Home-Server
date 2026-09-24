{ config, lib, ... }:
{
  # A box with a public name: certificates, nginx, the browser login and the
  # per-box issuer for jellyfin. The verifier's full role lives here.
  imports = [
    ./_sops.nix
    ../modules/gate/acme.nix
    ../modules/gate/public.nix
    ../modules/net/headscale.nix
  ];
  # the fleet's own network is run from here
  dd.headscale.enable = true;
  # the front door on the open internet: a Cloudflare tunnel this box opens
  # outward (the house line is carrier nat; nothing can be forwarded here).
  # Off, the names point at this box's tailnet address and no outsider is
  # answered; the same unit keeps the records either way.
  dd.public = {
    enable = true;
    # the gate's own pages and the sign-up flow; modules add their own
    hosts = [
      "home"
      "accounts"
    ];
    tunnel = "d0534bff-f478-48ab-a949-65e2e3c14c39";
    credentialsFile = config.sops.secrets.cloudflared-credentials.path;
    tokenFile = config.sops.templates."cloudflare.env".path;
  };
  dd.verify.role = "full";
  dd.verify.oidcSecretFile = config.sops.secrets.jellyfin-oauth-secret.path;
  services.tailscale.permitCertUid = "nginx"; # so nginx can fetch *.ts.net certs without root

  sops.secrets.cloudflare-token = { };
  sops.secrets.cloudflared-credentials = { }; # systemd hands it to cloudflared as a credential
  # read by the verifier's per-box oidc issuer for jellyfin; the seed script
  # in modules/jellyfin.nix runs as root
  sops.secrets.jellyfin-oauth-secret.owner = "dd-verify";
  sops.templates."cloudflare.env".content = ''
    CF_DNS_API_TOKEN=${config.sops.placeholder.cloudflare-token}
  '';

  # the bare domain: where the front door is
  services.nginx.virtualHosts.${config.dd.domain} = {
    forceSSL = true;
    useACMEHost = config.dd.domain;
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
