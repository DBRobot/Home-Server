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
}
