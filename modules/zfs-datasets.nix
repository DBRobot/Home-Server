# Declared ZFS datasets. Adding one here is all it takes — the oneshot
# below creates it if missing and re-applies properties on every rebuild.
# It never destroys anything, so it is safe to run against live data.
{ pkgs, lib, ... }:
let
  datasets = {
    "vault/photos" = {
      recordsize = "1M"; # photos are large sequential reads
      "com.sun:auto-snapshot" = "true"; # cheap on immutable files, saves you from rm -rf
    };

    # The ente key hierarchy lives in postgres on the ext4 root: no snapshots,
    # no checksums, no redundancy. Without it the 78G of blobs on vault cannot
    # be decrypted by anyone, including with the recovery key, because
    # master_key_encrypted_with_recovery_key is itself a row in that database.
    # Dumps land here: different physical disk to the source, and they inherit
    # snapshots and any future replication.
    "vault/backups" = {
      recordsize = "128K"; # small compressible dumps, not media
      compression = "zstd";
      "com.sun:auto-snapshot" = "true";
    };

    # Media is the one dataset where losing the single vdev costs only time:
    # it is all re-rippable. No snapshots either - a snapshot of a library is
    # the size of the library.
    "vault/media" = {
      recordsize = "1M"; # large sequential reads
      compression = "off"; # h264/hevc is already compressed
      "com.sun:auto-snapshot" = "false";
    };

    "tank/models" = {
      recordsize = "1M"; # large sequential reads of GGUF weights
      compression = "off"; # zstd is inherited pool-wide; weights are incompressible
      primarycache = "metadata"; # llama-server mlocks the weights; ARC caching them is waste
      "com.sun:auto-snapshot" = "false"; # a snapshot of a 23G model costs 23G
    };
  };

  zfs = "${pkgs.zfs}/bin/zfs";

  mkDataset =
    name: props:
    lib.concatStringsSep "\n" (
      [ "${zfs} list -H ${name} >/dev/null 2>&1 || ${zfs} create ${name}" ]
      ++ lib.mapAttrsToList (k: v: "${zfs} set ${k}=${v} ${name}") props
    );
in
{
  systemd.services.zfs-datasets = {
    description = "Ensure declared ZFS datasets exist with their properties";
    wantedBy = [ "multi-user.target" ];
    after = [
      "zfs-import.target"
      "zfs-mount.service"
    ];
    serviceConfig = {
      Type = "oneshot";
      RemainAfterExit = true;
    };
    script = lib.concatStringsSep "\n" (lib.mapAttrsToList mkDataset datasets);
  };
}
