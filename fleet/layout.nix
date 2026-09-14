# The one disk layout every box gets. No sizes to decide: an ESP the
# firmware needs, and the rest of the boot disk is a ZFS pool that root is a
# dataset on. Root keeps a reservation so a full pool can never brick the
# box; everything else is datasets with no size at all. Other disks become
# a pool each, named by what they are (modules/zfs-datasets.nix adds the
# datasets a role wants).
#
#   imports = [ (import ../../fleet/layout.nix { device = "/dev/disk/by-id/..."; }) ];
{
  device,
  pool ? "tank",
  rootReservation ? "32G",
}:
{ lib, ... }:
{
  # disko's rehearsal vm gives every disk 4G, too small for the root
  # reservation; a sparse 64G costs nothing
  disko.tests.extraConfig.virtualisation.emptyDiskImages = lib.mkForce [ 65536 ];

  disko.devices = {
    disk.boot = {
      type = "disk";
      inherit device;
      content = {
        type = "gpt";
        partitions = {
          ESP = {
            size = "1G";
            type = "EF00";
            content = {
              type = "filesystem";
              format = "vfat";
              mountpoint = "/boot";
              mountOptions = [
                "fmask=0022"
                "dmask=0022"
              ];
            };
          };
          pool = {
            size = "100%";
            content = {
              type = "zfs";
              inherit pool;
            };
          };
        };
      };
    };
    zpool.${pool} = {
      type = "zpool";
      mode = "";
      options = {
        ashift = "12";
        autotrim = "on";
        cachefile = "none";
      };
      rootFsOptions = {
        compression = "zstd";
        acltype = "posixacl";
        xattr = "sa";
        atime = "off";
        mountpoint = "none";
        "com.sun:auto-snapshot" = "false";
      };
      datasets = {
        # the system, as three datasets: a rollback of / never touches /nix
        # (the store is content-addressed; rolling it back gains nothing and
        # would drop paths newer generations need) or /home
        "system" = {
          type = "zfs_fs";
          options.mountpoint = "none";
        };
        "system/root" = {
          type = "zfs_fs";
          mountpoint = "/";
          options = {
            mountpoint = "legacy";
            reservation = rootReservation;
            "com.sun:auto-snapshot" = "true";
          };
        };
        "system/nix" = {
          type = "zfs_fs";
          mountpoint = "/nix";
          options = {
            mountpoint = "legacy";
            atime = "off";
          };
        };
        "system/var" = {
          type = "zfs_fs";
          mountpoint = "/var";
          options = {
            mountpoint = "legacy";
            "com.sun:auto-snapshot" = "true";
          };
        };
        "home" = {
          type = "zfs_fs";
          mountpoint = "/home";
          options = {
            mountpoint = "legacy";
            "com.sun:auto-snapshot" = "true";
          };
        };
      };
    };
  };
}
