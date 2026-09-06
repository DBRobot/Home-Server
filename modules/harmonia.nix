{ config, ... }:
{
  # Serves node1's own /nix/store as a binary cache. Nothing is duplicated -
  # it exposes what is already there, so it costs no extra disk.
  #
  # No firewall port is opened: trustedInterfaces already covers tailscale0,
  # so this is reachable from the tailnet and nowhere else. CI joins the
  # tailnet to fetch, which is why it never needs to be public.
  services.harmonia.cache = {
    enable = true;
    signKeyPaths = [ config.sops.secrets.harmonia-signing-key.path ];
    settings = {
      bind = "[::]:5000";
      priority = 30; # lower than cache.nixos.org (40), so node1 is preferred
    };
  };

  # No owner override needed: the module uses systemd LoadCredential, so
  # systemd reads the key as root and passes it in. Unlike ente, which reads
  # its secrets directly as its own user and needed them chowned.
}
