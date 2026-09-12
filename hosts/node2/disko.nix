{ ... }:
{
  disko.devices = {
    disk = {
      nvme = {
        type = "disk";
        device = "/dev/disk/by-id/nvme-Micron_2450_MTFDKBA512TFK_2311403FB25B";
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
                mountOptions = [ "fmask=0022" "dmask=0022" ];
              };
            };
            root = {
              size = "64G";
              content = {
                type = "filesystem";
                format = "ext4";
                mountpoint = "/";
              };
            };
            tank = {
              size = "100%";
              content = {
                type = "zfs";
                pool = "tank";
              };
            };
          };
        };
      };
    };
    zpool = {
      tank = {
        type = "zpool";
        mode = "create";
        options.cachefile = "none";
        rootFsOptions.compression = "zstd";
        datasets = { };
      };
    };
  };
}