{ config, ... }:
let
  base = "distributed-datacenter.duckdns.org";
in
{
  # One wildcard cert for every subdomain. DNS-01 needs no inbound ports,
  # which is why nothing here opens 80/443 - tailscale0 is already trusted.
  # extraDomainNames must stay empty: duckdns allows a single TXT record, so a
  # cert covering both *.domain and domain deadlocks its own challenges.
  security.acme = {
    acceptTerms = true;
    defaults.email = "davidsprojects7@gmail.com";
    certs.${base} = {
      domain = "*.${base}";
      dnsProvider = "duckdns";
      environmentFile = config.sops.templates."duckdns.env".path;
      # not "nginx": kanidm terminates its own tls and needs to read
      # these too, so both services share a group instead
      group = "acmecerts";
    };
  };

  users.groups.acmecerts = { };
  users.users.nginx.extraGroups = [ "acmecerts" ];

  services.nginx = {
    enable = true;
    recommendedProxySettings = true;
  };
}
