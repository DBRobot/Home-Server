{ config, ... }:
{
  # The pool and garage on it. The key template here carries the photo and
  # media keys, which the photos and media roles declare: a storage-only box
  # is not yet a thing this untangles.
  imports = [
    ./_sops.nix
    ../modules/zfs-datasets.nix
    ../modules/garage.nix
  ];
  services.zfs.autoSnapshot.enable = true;
  services.zfs.autoScrub.enable = true;

  sops.secrets.garage-rpc-secret = { };
  # readable by the garage user so the CLI works for admin, not just the unit
  sops.templates."garage.env".owner = "garage";
  sops.templates."garage.env".content = ''
    GARAGE_RPC_SECRET=${config.sops.placeholder.garage-rpc-secret}
  '';
  sops.templates."garage-key.env".content = ''
    GARAGE_KEY_ID=${config.sops.placeholder.garage-key-id}
    GARAGE_KEY_SECRET=${config.sops.placeholder.garage-key-secret}
    GARAGE_MEDIA_KEY_ID=${config.sops.placeholder.garage-media-key-id}
    GARAGE_MEDIA_KEY_SECRET=${config.sops.placeholder.garage-media-key-secret}
  '';
}
