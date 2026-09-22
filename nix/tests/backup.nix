# Two storage boxes form the garage cluster, each backs itself up into it,
# and a box that has lost everything gets its files back from the other:
# the copy is on both boxes, reads survive a box going away, and the
# restore path is the one that matters, not the backup path.
{ pkgs, self, ... }:
let
  rpc = pkgs.writeText "garage.env" "GARAGE_RPC_SECRET=0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef\n";
  key = box: {
    id = "GK${builtins.substring 0 24 (builtins.hashString "sha256" box)}";
    secret = builtins.hashString "sha256" "${box}-secret";
  };
  storageBox = name: {
    imports = [
      ./box.nix
      ../modules/storage/garage.nix
      ../modules/storage/backup.nix
    ];
    dd.garage = {
      zone = "r-test";
      capacity = "1G";
      dataDir = "/srv/garage";
      publicAddr = "${name}:3901";
      envFile = rpc;
      setupEnvFiles = [
        (pkgs.writeText "backup-key.env" ''
          GARAGE_BACKUP_KEY_ID=${(key name).id}
          GARAGE_BACKUP_KEY_SECRET=${(key name).secret}
        '')
      ];
      setup = ''
        garage bucket create backups-${name} 2>/dev/null || true
        garage key import "$GARAGE_BACKUP_KEY_ID" "$GARAGE_BACKUP_KEY_SECRET" --yes -n backup-${name} 2>/dev/null || true
        garage bucket allow --read --write --owner backups-${name} --key "$GARAGE_BACKUP_KEY_ID" 2>/dev/null || true
      '';
    };
    dd.backup = {
      paths = [ "/srv/state" ];
      envFile = pkgs.writeText "restic.env" ''
        AWS_ACCESS_KEY_ID=${(key name).id}
        AWS_SECRET_ACCESS_KEY=${(key name).secret}
        AWS_DEFAULT_REGION=us-east-1
      '';
      passwordFile = pkgs.writeText "restic-password" "${name}-password\n";
    };
    environment.systemPackages = [
      pkgs.restic
      pkgs.awscli2
    ];
  };
in
{
  name = "backup";
  node.specialArgs = { inherit self; };
  defaults.virtualisation.memorySize = 1536;
  defaults.virtualisation.cores = 2; # services start in parallel instead of queueing on one thread
  defaults.virtualisation.diskSize = 4096;
  nodes = {
    a = storageBox "a";
    b = storageBox "b";
  };
  scriptEnv = {
    inherit rpc;
    a_env = (storageBox "a").dd.backup.envFile;
    a_password = (storageBox "a").dd.backup.passwordFile;
    b_env = (storageBox "b").dd.backup.envFile;
  };
}
