# Declared ZFS datasets. Adding one here is all it takes — the oneshot
# below creates it if missing and re-applies properties on every rebuild.
# It never destroys anything, so it is safe to run against live data.
{ pkgs, lib, ... }:
let
  datasets = {
    "tank/photos" = { "com.sun:auto-snapshot" = "true"; };
  };

  zfs = "${pkgs.zfs}/bin/zfs";

  mkDataset = name: props:
    lib.concatStringsSep "\n" (
      [ "${zfs} list -H ${name} >/dev/null 2>&1 || ${zfs} create ${name}" ]
      ++ lib.mapAttrsToList (k: v: "${zfs} set ${k}=${v} ${name}") props
    );
in
{
  systemd.services.zfs-datasets = {
    description = "Ensure declared ZFS datasets exist with their properties";
    wantedBy = [ "multi-user.target" ];
    after = [ "zfs-import.target" "zfs-mount.service" ];
    serviceConfig = {
      Type = "oneshot";
      RemainAfterExit = true;
    };
    script = lib.concatStringsSep "\n" (lib.mapAttrsToList mkDataset datasets);
  };
}
