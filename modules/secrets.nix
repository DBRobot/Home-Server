{ config, ... }:
{
  sops = {
    defaultSopsFile = ../secrets/secrets.yaml;
    # node1 decrypts with the ssh host key it has had since install. No new
    # key material exists, and anyone holding it already owns the machine.
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
      peergos-s3-key-id = { };   # read by podman as root
      peergos-s3-key-secret = { };
    };

    # several consumers want an EnvironmentFile rather than a bare value
    templates = {
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
        PEERGOS_S3_KEY_ID=${config.sops.placeholder.peergos-s3-key-id}
        PEERGOS_S3_KEY_SECRET=${config.sops.placeholder.peergos-s3-key-secret}
      '';
      "peergos.env".content = ''
        PEERGOS_S3_KEY_ID=${config.sops.placeholder.peergos-s3-key-id}
        PEERGOS_S3_KEY_SECRET=${config.sops.placeholder.peergos-s3-key-secret}
      '';
    };
  };
}
