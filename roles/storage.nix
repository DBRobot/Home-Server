{ config, ... }:
{
  # A node of the garage cluster and the datasets its pool holds. Which
  # pool, how much of it, and where the blocks live are facts of the
  # machine (hosts/<box>/hardware.nix and fleet/boxes.json); this binds the
  # secrets: the cluster's rpc secret, shared by every storage box, and
  # this box's own backup key and password, shared with nobody.
  imports = [
    ./_sops.nix
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
  dd.garage.envFile = config.sops.templates."garage.env".path;

  # this box's backup key: one bucket, this box's, nothing else
  sops.secrets.garage-backup-key-id = { };
  sops.secrets.garage-backup-key-secret = { };
  sops.secrets.restic-password = { };
  sops.templates."garage-backup-key.env".content = ''
    GARAGE_BACKUP_KEY_ID=${config.sops.placeholder.garage-backup-key-id}
    GARAGE_BACKUP_KEY_SECRET=${config.sops.placeholder.garage-backup-key-secret}
  '';
  sops.templates."restic.env".content = ''
    AWS_ACCESS_KEY_ID=${config.sops.placeholder.garage-backup-key-id}
    AWS_SECRET_ACCESS_KEY=${config.sops.placeholder.garage-backup-key-secret}
    AWS_DEFAULT_REGION=us-east-1
  '';
  dd.backup.envFile = config.sops.templates."restic.env".path;
  dd.backup.passwordFile = config.sops.secrets.restic-password.path;
  dd.garage.setupEnvFiles = [ config.sops.templates."garage-backup-key.env".path ];
  dd.garage.setup = ''
    garage bucket create backups-${config.networking.hostName} 2>/dev/null || true
    garage key import "$GARAGE_BACKUP_KEY_ID" "$GARAGE_BACKUP_KEY_SECRET" --yes -n backup-${config.networking.hostName} 2>/dev/null || true
    garage bucket allow --read --write --owner backups-${config.networking.hostName} --key "$GARAGE_BACKUP_KEY_ID" 2>/dev/null || true
  '';
}
