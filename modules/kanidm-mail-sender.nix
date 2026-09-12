{
  config,
  ...
}:
let
  base = config.dd.domain;
  user = "kanidm-mail-sender";
  relayAddress = "distributed.datacenter@gmail.com";
in
{
  users.users.${user} = {
    isSystemUser = true;
    group = user;
  };
  users.groups.${user} = { };

  # Both secrets in this file are sops values, so the whole config is a sops
  # template - the same shape as rclone.env in modules/secrets.nix - and nothing
  # has to assemble it at start.
  # No owner/group/mode: the unit takes this through systemd LoadCredential, the
  # same way harmonia takes its signing key, so it stays root-only here and
  # systemd hands the service a private copy. That also answers the sender's own
  # startup warning - it refuses to call a config secure when the uid reading it
  # also OWNS it, because that uid can then change the mode. Under
  # LoadCredential the copy is root-owned in a directory no other uid can reach.
  sops.templates."kanidm-mail-sender.toml" = {
    content = ''
      token = "${config.sops.placeholder.mail-sender-api-token}"
      instance_display_name = "Distributed Datacenter"
      instance_url = "https://idm.${base}"
      mail_from_address = "${relayAddress}"
      mail_reply_to_address = "${relayAddress}"
      mail_relay = "smtps://smtp.gmail.com"
      mail_username = "${relayAddress}"
      mail_password = "${config.sops.placeholder.ente-smtp-password}"
    '';
  };

  # kanidmd itself sends no mail - it enqueues, and this drains the queue. That
  # is why there is no smtp setting anywhere in server.toml: delivery is
  # deliberately a separate process holding separate credentials.
  systemd.services.kanidm-mail-sender = {
    description = "Deliver kanidm's queued mail through the gmail relay";
    after = [
      "kanidm.service"
      "kanidm-service-accounts.service"
      "network-online.target"
    ];
    requires = [
      "kanidm.service"
      "kanidm-service-accounts.service" # the account the sops token belongs to
    ];
    wants = [ "network-online.target" ];
    wantedBy = [ "multi-user.target" ];
    serviceConfig = {
      Type = "simple";
      User = user;
      Group = user;
      LoadCredential = [
        "config:${config.sops.templates."kanidm-mail-sender.toml".path}"
      ];
      # -c is the client config client.enable already writes; -m is the
      # credential systemd just placed, named by %d.
      #
      # mail_relay is a URL as of 1.11 and was a BARE HOSTNAME in 1.10, whose
      # example config said in as many words that it must not carry a scheme.
      # Upgrading without changing it fails at parse with "relative URL without
      # a base". smtps:// is implicit TLS on 465, which is what lettre's relay()
      # did by default before; msmtp in modules/mail.nix uses 587 + STARTTLS
      # against the same account, which would be smtp:// here.
      ExecStart = ''
        ${config.services.kanidm.package}/bin/kanidm-mail-sender \
          -c /etc/kanidm/config \
          -m %d/config
      '';
      Restart = "on-failure";
      RestartSec = 30;

      NoNewPrivileges = true;
      PrivateTmp = true;
      PrivateDevices = true;
      ProtectSystem = "strict";
      ProtectHome = true;
      ProtectKernelTunables = true;
      ProtectKernelModules = true;
      RestrictNamespaces = true;
      LockPersonality = true;
      SystemCallArchitectures = "native";
      SystemCallFilter = [ "@system-service" ];
    };
  };
}
