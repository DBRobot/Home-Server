# One-off, after the move to commonty.org: the photos password of an account
# linked before the move is the old passkey's secret, and a passkey answers
# only on the name it was made for. This serves a page under the old name
# that asks that passkey once, with the old certificate still on the box.
# Remove after the accounts are re-linked.
{ config, lib, ... }:
let
  old = "distributed-datacenter.duckdns.org";
in
{
  services.nginx.virtualHosts."files.${old}" = {
    forceSSL = true;
    sslCertificate = "/var/lib/acme/${old}/fullchain.pem";
    sslCertificateKey = "/var/lib/acme/${old}/key.pem";
    locations."= /".return = "302 /_dd/recover/";
    locations."/_dd/recover/" = {
      alias = "${./recover}/";
      index = "index.html";
    };
  };
  # the tailnet answers the old name too, for as long as this is here
  services.dnsmasq.settings.address = lib.mkAfter [ "/${old}/${config.dd.box.tailnet}" ];
  services.dnsmasq.settings.local = lib.mkAfter [ "/${old}/" ];
}
