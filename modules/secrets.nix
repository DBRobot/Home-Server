{ config, ... }:
{
  sops = {
    defaultSopsFile = ../secrets/secrets.yaml;
    # node1 decrypts with the ssh host key it has had since install. No new
    # key material exists, and anyone holding it already owns the machine.
    age.sshKeyPaths = [ "/etc/ssh/ssh_host_ed25519_key" ];

    secrets = {
      duckdns-token = { };
      minio-root-user = { };
      minio-root-password = { };
      ente-s3-key = { };
      ente-s3-secret = { };
      ente-key-encryption = { };
      ente-key-hash = { };
      ente-jwt-secret = { };
    };

    # several consumers want an EnvironmentFile rather than a bare value
    templates = {
      "duckdns.env".content = ''
        DUCKDNS_TOKEN=${config.sops.placeholder.duckdns-token}
      '';
      "minio.env".content = ''
        MINIO_ROOT_USER=${config.sops.placeholder.minio-root-user}
        MINIO_ROOT_PASSWORD=${config.sops.placeholder.minio-root-password}
      '';
    };
  };
}
