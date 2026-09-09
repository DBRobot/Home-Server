{
  config,
  ...
}:
let
  base = "distributed-datacenter.duckdns.org";
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
  sops.templates."kanidm-mail-sender.toml" = {
    # root-owned and group-readable, not owned by the service user. The sender
    # checks this itself and warns twice at startup otherwise: a config the
    # running uid OWNS can have its own permissions changed by that uid, so
    # "readonly to the running uid" is only true if someone else owns it.
    owner = "root";
    group = user;
    mode = "0440";
    content = ''
      token = "${config.sops.placeholder.mail-sender-api-token}"
      instance_display_name = "Distributed Datacenter"
      instance_url = "https://idm.${base}"
      mail_from_address = "${relayAddress}"
      mail_reply_to_address = "${relayAddress}"
      mail_relay = "smtp.gmail.com"
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
      # -c is the client config enableClient already writes; -m is ours. Note
      # lettre's relay() is implicit TLS on 465, where msmtp in modules/mail.nix
      # uses 587 + STARTTLS against the same account. Both are fine with gmail;
      # they are just different libraries with different defaults.
      ExecStart = ''
        ${config.services.kanidm.package}/bin/kanidm-mail-sender \
          -c /etc/kanidm/config \
          -m ${config.sops.templates."kanidm-mail-sender.toml".path}
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
