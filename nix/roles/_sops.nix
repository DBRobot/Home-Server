{ config, ... }:
{
  # Every role that declares a secret imports this. A box with no such role
  # has no sops at all - node2 is a recipient of nothing.
  sops.defaultSopsFile = ../../secrets + "/${config.networking.hostName}.yaml";
  # The box decrypts with the ssh host key it has had since install. No new
  # key material exists, and anyone holding it already owns the machine.
  sops.age.sshKeyPaths = [ "/etc/ssh/ssh_host_ed25519_key" ];
}
