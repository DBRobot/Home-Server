{ config, ... }:
{
  sops = {
    defaultSopsFile = ../secrets/secrets.yaml;
    # node1 decrypts with the ssh host key it has had since install. No new
    # key material exists, and anyone holding it already owns the machine.
    age.sshKeyPaths = [ "/etc/ssh/ssh_host_ed25519_key" ];

    secrets = {
      duckdns-token = { };
      garage-rpc-secret = { };
      garage-key-id = { };
      garage-key-secret = { };
      ente-key-encryption = { };
      ente-key-hash = { };
      ente-jwt-secret = { };
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
      '';
    };
  };
}
