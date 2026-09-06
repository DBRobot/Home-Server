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
