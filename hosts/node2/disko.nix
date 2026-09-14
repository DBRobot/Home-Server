# node2's disks, through the fleet template: one NVMe, ZFS root.
import ../../fleet/layout.nix {
  device = "/dev/disk/by-id/nvme-Micron_2450_MTFDKBA512TFK_2311403FB25B";
}
