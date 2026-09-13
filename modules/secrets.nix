{ config, ... }:
{
  sops = {
    defaultSopsFile = ../secrets/secrets.yaml;
    # The box decrypts with the ssh host key it has had since install. No new
    # key material exists, and anyone holding it already owns the machine.
    # Each box is a recipient of its own file only - see .sops.yaml.
    age.sshKeyPaths = [ "/etc/ssh/ssh_host_ed25519_key" ];

    secrets = {
      duckdns-token = { };
      harmonia-signing-key = { }; # read by systemd LoadCredential, no owner needed
      garage-rpc-secret = { };
      # ente's pre-start reads these as the ente user; sops defaults to
      # root-only, which made museum fail with "Permission denied"
      garage-key-id.owner = "ente";
      garage-key-secret.owner = "ente";
      ente-key-encryption.owner = "ente";
      ente-key-hash.owner = "ente";
      ente-jwt-secret.owner = "ente";
      ente-smtp-password.owner = "ente";
      grafana-secret-key.owner = "grafana";
      # the client secret jellyfin's sso plugin presents to the verifier's
      # per-box issuer; the seed script in modules/jellyfin.nix runs as root
      jellyfin-oauth-secret.owner = "dd-verify";
      # read only through sops templates below, so root-only is fine
      garage-media-key-id = { };
      garage-media-key-secret = { };
      rclone-crypt-password = { };
      rclone-crypt-salt = { };
      # Where machine mail actually goes. Read only through the msmtp aliases
      # template below, so root-only is right.
      alert-recipient = { };
    };

    # several consumers want an EnvironmentFile rather than a bare value
    templates = {
      # msmtp expands local names through this, so zed, smartd and the backup
      # alerts can all address "alerts" and the real destination stays here.
      # One place to change it, and nothing names a person in the repo.
      "msmtp-aliases".content = ''
        alerts: ${config.sops.placeholder.alert-recipient}
        default: ${config.sops.placeholder.alert-recipient}
      '';
      "duckdns.env".content = ''
        DUCKDNS_TOKEN=${config.sops.placeholder.duckdns-token}
      '';
      # readable by the garage user so the CLI works for admin, not just the unit
      "garage.env".owner = "garage";
      "garage.env".content = ''
        GARAGE_RPC_SECRET=${config.sops.placeholder.garage-rpc-secret}
      '';
      "garage-key.env".content = ''
        GARAGE_KEY_ID=${config.sops.placeholder.garage-key-id}
        GARAGE_KEY_SECRET=${config.sops.placeholder.garage-key-secret}
        GARAGE_MEDIA_KEY_ID=${config.sops.placeholder.garage-media-key-id}
        GARAGE_MEDIA_KEY_SECRET=${config.sops.placeholder.garage-media-key-secret}
      '';
      # rclone takes its whole config from the environment, so no config file
      # is written anywhere. PASSWORD/PASSWORD2 are rclone-obscured, which is
      # obfuscation not encryption - sops is what actually protects them.
      "rclone.env".owner = "media";
      "rclone.env".content = ''
        RCLONE_CONFIG_GARAGE_TYPE=s3
        RCLONE_CONFIG_GARAGE_PROVIDER=Other
        RCLONE_CONFIG_GARAGE_ENDPOINT=http://127.0.0.1:3900
        RCLONE_CONFIG_GARAGE_REGION=us-east-1
        RCLONE_CONFIG_GARAGE_ACCESS_KEY_ID=${config.sops.placeholder.garage-media-key-id}
        RCLONE_CONFIG_GARAGE_SECRET_ACCESS_KEY=${config.sops.placeholder.garage-media-key-secret}
        RCLONE_CONFIG_COLD_TYPE=crypt
        RCLONE_CONFIG_COLD_REMOTE=garage:media
        RCLONE_CONFIG_COLD_PASSWORD=${config.sops.placeholder.rclone-crypt-password}
        RCLONE_CONFIG_COLD_PASSWORD2=${config.sops.placeholder.rclone-crypt-salt}
      '';
    };
  };
}
