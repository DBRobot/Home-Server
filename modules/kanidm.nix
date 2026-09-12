{
  config,
  pkgs,
  lib,
  ...
}:
let
  base = config.dd.domain;
  host = "idm.${base}";
  certDir = "/var/lib/acme/${base}";

in
{
  # kanidm insists on terminating tls itself - it will not serve plaintext
  # behind nginx like everything else here - so it needs to read the acme
  # cert. modules/acme.nix puts those in the acmecerts group for this.
  users.users.kanidm.extraGroups = [ "acmecerts" ];

  # NSS and PAM backed by kanidm, so a person is the same uid on every node -
  # which shared storage needs and per-node `users.users` cannot give. nss puts
  # kanidm ahead of files and its module returns NOTFOUND when unixd is down,
  # so anything still in /etc/passwd stays a working fallback.
  #
  # Note kanidm-unixd will NOT resolve a name that also exists in /etc/passwd:
  # that is what allow_local_account_override guards, so a remote idp cannot
  # hijack a local account. Accounts are therefore declared in kanidm ONLY -
  # see modules/user-accounts.nix.
  services.kanidm = {
    server.enable = true;
    unix.enable = true;
    unix.settings = {
      kanidm.pam_allowed_login_groups = [ "users" ];
      # match where the directories actually are, rather than unixd's /home
      # default which nothing on this host uses
      home_prefix = "/srv/users/";
      home_attr = "name"; # /srv/users/david, not /srv/users/<uuid>
      # Without these, unixd resolves accounts by SPN and `id david` answers
      # "david@idm.distributed-datacenter.duckdns.org" - which is a valid unix
      # name but not the one the webdav path, the acls and every other service
      # were built around.
      uid_attr_map = "name";
      gid_attr_map = "name";
      default_shell = "${pkgs.shadow}/bin/nologin";
    };
    # without this the cli has no /etc/kanidm/config and every
    # onboarding command needs an explicit --url
    client.enable = true;
    client.settings.uri = "https://${host}";
    # no default: nixpkgs ships several majors, same as garage. The
    # WithSecretProvisioning variant is what lets oauth2 client secrets be
    # provisioned declaratively rather than clicked in a ui.
    #
    # Upgrades must be SEQUENTIAL - 1.10 to 1.12 is not supported, only 1.10 to
    # 1.11 to 1.12 - so this cannot be left to drift. 1.10 reached end of life
    # on 2026-08-31. `kanidmd domain upgrade-check` reported PASS (domain level
    # 14 -> 15) before this bump; the level itself is raised by the server on
    # first start, there is no command for it.
    package = pkgs.kanidmWithSecretProvisioning_1_11;

    server.settings = {
      # Changing this later is a documented migration, not a config edit, so
      # it is deliberately the same as the origin host. Note this bakes in a
      # duckdns name - moving to a real domain later means that migration.
      domain = host;
      origin = "https://${host}";
      bindaddress = "127.0.0.1:8443";
      ldapbindaddress = "127.0.0.1:3636";
      tls_chain = "${certDir}/fullchain.pem";
      tls_key = "${certDir}/key.pem";
      # The module's default is versions = 0, and 0 means OFF: the backups
      # directory was empty when it was needed. A raw copy of kanidm.db is not
      # a backup either - sqlite runs in WAL mode and the .db alone is
      # "database disk image is malformed". This is kanidm's own dump format,
      # restorable with `kanidmd database restore`.
      online_backup = {
        path = "/var/lib/kanidm/backups";
        schedule = "00 22 * * *";
        versions = 7;
      };
    };

    # Accounts and groups are declared; credentials deliberately are not.
    # People enrol their own password or passkey through kanidm's self-service
    # flow, so a password change never needs a rebuild.
    provision = {
      enable = true;
      # DEFAULTS TO TRUE, and it deletes. kanidm-provision tracks every entity
      # it created (via ext_idm_provisioned_entities) and removes any of them
      # that later vanish from the state file - which is exactly what happened
      # when the roster left this file: the person it had created went with
      # it, recycle bin and all. People are runtime state owned by kanidm's
      # database now; provisioning must never be the thing that removes one.
      autoRemove = false;
      adminPasswordFile = config.sops.secrets.kanidm-admin-password.path;
      idmAdminPasswordFile = config.sops.secrets.kanidm-idm-admin-password.path;

      # No roster here, encrypted or otherwise. People come from signup and
      # live only in kanidm's database - the one copy that has to exist. This
      # repo is public, and a list of persons is a list of everyone using the
      # service; the fewer copies the better, and provisioning never deletes
      # persons it does not know about, so leaving them out is safe.

      # overwriteMembers is false on both because membership is now set at
      # RUNTIME, not here: modules/signup.nix adds people to `pending`, and
      # whatever grants entitlement later adds them to `users`. The option
      # defaults to true, which would reset both lists to the declared members
      # on every provisioning run and silently undo every signup and every
      # promotion. Append mode is the cost of that: removing someone from the
      # roster no longer removes them from the group, which is correct now that
      # the roster is not the authority on membership.
      groups.users = {
        overwriteMembers = false;
      };
      # append mode here too, now that no roster supplies the members list:
      # the default (true) would reset admins to empty on every run
      groups.admins = {
        overwriteMembers = false;
      };

      # Where signup puts people. Deliberately referenced by NO scopeMap
      # anywhere, so a member of it can authenticate and reach nothing - kanidm
      # refuses to issue a token for a client whose scope maps do not match any
      # group they are in. That is the whole access gate.
      groups.pending = {
        overwriteMembers = false;
      };

      # The dd cli. A public client: no secret exists, because a secret
      # shipped inside a binary on someone's laptop is not a secret. PKCE is
      # what stops an intercepted code being redeemed by anyone else.
      #
      # The cli binds 127.0.0.1:0 so concurrent logins cannot collide, which
      # means the port is not known ahead of time - enableLocalhostRedirects
      # is what permits that rather than pinning one port.
      # oauth2-proxy's own client. Confidential - it runs on node1 and can
      # hold a secret - and it exists only to give oauth2-proxy an issuer to
      # discover. The tokens it actually verifies are dd's, accepted through
      # extra-jwt-issuers in modules/llm-auth.nix.
      systems.oauth2.llm = {
        displayName = "LLM gateway";
        originUrl = "https://llm.${base}/oauth2/callback";
        originLanding = "https://llm.${base}/";
        basicSecretFile = config.sops.secrets.llm-oauth-secret.path;
        preferShortUsername = true;
        scopeMaps.users = [
          "openid"
          "profile"
          "email"
          "groups"
        ];
      };

      systems.oauth2.dd = {
        displayName = "Distributed Datacenter CLI";
        public = true;
        enableLocalhostRedirects = true;
        originUrl = "http://127.0.0.1:8080/callback";
        originLanding = "https://idm.${base}/";
        preferShortUsername = true;
        scopeMaps.users = [
          "openid"
          "profile"
          "email"
          "groups"
          # without this kanidm issues no refresh token, and every access
          # token expiry would mean another browser round trip
          "offline_access"
        ];
      };

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
        # /r/ not /redirect/: the plugin config has NewPath=false, so it uses
        # the old short path. Must be an exact match or kanidm rejects
        # the authorise with invalid_origin.
        originUrl = "https://jellyfin.${base}/sso/OID/r/kanidm";
        # the sso entrypoint, not the root: jellyfin's root is its own
        # login form, so landing there from kanidm's app list asks for
        # a password instead of starting the sso flow
        originLanding = "https://jellyfin.${base}/sso/OID/p/kanidm";
        basicSecretFile = config.sops.secrets.jellyfin-oauth-secret.path;
        preferShortUsername = true;
        scopeMaps.users = [
          "openid"
          "profile"
          "email"
          "groups"
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
