{ ... }:
{
  # What is true of this machine and no other. Everything it runs is a role
  # in fleet/boxes.json.
  imports = [ ./hardware-configuration.nix ];

  boot.zfs.extraPools = [
    "tank"
    "vault"
  ];
  # default c_max is ~all of RAM (measured 61.5G); with no swap that collides
  # with the model's mlocked 23G. Raise once photos land on the HDD pool.
  boot.extraModprobeConfig = ''
    options zfs zfs_arc_max=8589934592
  '';

  networking.hostId = "0195f284";
  networking.networkmanager.ensureProfiles.profiles.direct-link = {
    connection = {
      id = "direct-link";
      type = "ethernet";
      autoconnect = true;
      autoconnect-priority = -999;
    };
    ethernet.mac-address = "44:ED:57:10:00:40"; # not interface-name; that encodes the USB port
    ipv4 = {
      method = "manual";
      address1 = "10.10.10.2/24";
      gateway = "10.10.10.1"; # NATed out the Legion's wifi until home internet exists
      dns = "1.1.1.1;9.9.9.9;";
    };
    ipv6.method = "link-local";
  };
}
