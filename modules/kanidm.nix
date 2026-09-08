{ config, pkgs, ... }:
let
  base = "distributed-datacenter.duckdns.org";
  host = "idm.${base}";
  certDir = "/var/lib/acme/${base}";
in
{
  # kanidm insists on terminating tls itself - it will not serve plaintext
  # behind nginx like everything else here - so it needs to read the acme
  # cert. modules/acme.nix puts those in the acmecerts group for this.
  users.users.kanidm.extraGroups = [ "acmecerts" ];

  services.kanidm = {
    enableServer = true;
    # without this the cli has no /etc/kanidm/config and every
    # onboarding command needs an explicit --url
    enableClient = true;
    clientSettings.uri = "https://${host}";
    # no default: nixpkgs ships several majors, same as garage. The
    # WithSecretProvisioning variant is what lets oauth2 client secrets be
    # provisioned declaratively rather than clicked in a ui.
    package = pkgs.kanidmWithSecretProvisioning_1_10;

    serverSettings = {
      # Changing this later is a documented migration, not a config edit, so
      # it is deliberately the same as the origin host. Note this bakes in a
      # duckdns name - moving to a real domain later means that migration.
      domain = host;
      origin = "https://${host}";
      bindaddress = "127.0.0.1:8443";
      ldapbindaddress = "127.0.0.1:3636";
      tls_chain = "${certDir}/fullchain.pem";
      tls_key = "${certDir}/key.pem";
    };

    # Accounts and groups are declared; credentials deliberately are not.
    # People enrol their own password or passkey through kanidm's self-service
    # flow, so a password change never needs a rebuild.
    provision = {
      enable = true;
      adminPasswordFile = config.sops.secrets.kanidm-admin-password.path;
      idmAdminPasswordFile = config.sops.secrets.kanidm-idm-admin-password.path;

      groups.users = { };
      groups.admins = { };

      systems.oauth2.grafana = {
        displayName = "Grafana";
        originUrl = "https://grafana.${base}/login/generic_oauth";
        originLanding = "https://grafana.${base}/";
        # the secret is ours, not kanidm's: provisioning it from sops means
        # both sides read one source instead of copying a generated value
        basicSecretFile = config.sops.secrets.grafana-oauth-secret.path;
        preferShortUsername = true; # "david", not the full spn
        scopeMaps.users = [
          "openid"
          "profile"
          "email"
          "groups"
        ];
      };

      systems.oauth2.jellyfin = {
        displayName = "Jellyfin";
        originUrl = "https://jellyfin.${base}/sso/OID/redirect/kanidm";
        originLanding = "https://jellyfin.${base}/";
        basicSecretFile = config.sops.secrets.jellyfin-oauth-secret.path;
        preferShortUsername = true;
        scopeMaps.users = [
          "openid"
          "profile"
          "email"
          "groups"
        ];
      };

      persons.david = {
        displayName = "David";
        mailAddresses = [ "davidsprojects7@gmail.com" ];
        groups = [
          "users"
          "admins"
        ];
      };
    };
  };

  services.nginx.virtualHosts.${host} = {
    useACMEHost = base;
    forceSSL = true;
    locations."/" = {
      proxyPass = "https://127.0.0.1:8443";
      proxyWebsockets = true;
      extraConfig = ''
        # the backend cert is issued for the public name, not 127.0.0.1
        proxy_ssl_verify off;
        proxy_ssl_server_name on;
        client_max_body_size 0;
      '';
    };
  };
}
