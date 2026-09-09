{
  config,
  pkgs,
  ...
}:
let
  base = "distributed-datacenter.duckdns.org";
  user = "kanidm-mail-sender";
  dir = "/var/lib/${user}";
  token = "/var/lib/kanidm-service-accounts/mail-sender.token";
  relayAddress = "distributed.datacenter@gmail.com";

  # kanidmd itself sends no mail - it enqueues, and this drains the queue. That
  # is why there is not an smtp setting anywhere in server.toml: delivery is
  # deliberately a separate process holding separate credentials.
  #
  # The token cannot come from sops. kanidm mints api tokens and will not accept
  # an imported one, so modules/kanidm-service-accounts.nix owns it and this
  # assembles the config around it at start.
  writeConfig = pkgs.writeShellScript "kanidm-mail-sender-config" ''
    set -euo pipefail
    umask 077
    cat > ${dir}/config.toml <<EOF
    token = "$(cat ${token})"
    instance_display_name = "Distributed Datacenter"
    instance_url = "https://idm.${base}"
    mail_from_address = "${relayAddress}"
    mail_reply_to_address = "${relayAddress}"
    mail_relay = "smtp.gmail.com"
    mail_username = "${relayAddress}"
    mail_password = "$(cat ${config.sops.secrets.ente-smtp-password.path})"
    EOF
    chown ${user} ${dir}/config.toml
  '';
in
{
  users.users.${user} = {
    isSystemUser = true;
    group = user;
    home = dir;
  };
  users.groups.${user} = { };

  systemd.services.kanidm-mail-sender = {
    description = "Deliver kanidm's queued mail through the gmail relay";
    after = [
      "kanidm.service"
      "kanidm-service-accounts.service"
      "network-online.target"
    ];
    requires = [
      "kanidm.service"
      "kanidm-service-accounts.service"
    ];
    wants = [ "network-online.target" ];
    wantedBy = [ "multi-user.target" ];
    serviceConfig = {
      Type = "simple";
      User = user;
      Group = user;
      StateDirectory = user;
      StateDirectoryMode = "0700";
      # "+" runs as root despite User=, which is what lets it read the
      # root-owned sops secret and the root-owned token before dropping down.
      ExecStartPre = "+${writeConfig}";
      # -c is the client config enableClient already writes; -m is ours. Note
      # lettre's relay() is implicit TLS on 465, where msmtp in modules/mail.nix
      # uses 587 + STARTTLS against the same account. Both are fine with gmail;
      # they are just different libraries with different defaults.
      ExecStart = ''
        ${config.services.kanidm.package}/bin/kanidm-mail-sender \
          -c /etc/kanidm/config \
          -m ${dir}/config.toml
      '';
      Restart = "on-failure";
      RestartSec = 30;

      NoNewPrivileges = true;
      PrivateTmp = true;
      ProtectSystem = "strict";
      ProtectHome = true;
      ReadWritePaths = [ dir ];
    };
  };
}
