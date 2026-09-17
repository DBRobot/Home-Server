{ ... }:
{
  # What is true of this machine and no other. Everything it runs is a role
  # in fleet/boxes.json. hardware-configuration.nix is the output of
  # `nixos-generate-config --no-filesystems --show-hardware-config` on the
  # box, unedited: disko owns the mounts, the generator owns the rest.
  imports = [
    ./hardware-configuration.nix
    ./disko.nix
  ];

  # root is on tank, so the pool imports at boot on its own

  networking.hostId = "083f7ed3";

  # garage's blocks on the one pool there is. Content-addressed ciphertext:
  # no snapshots, nothing to gain from them
  dd.zfs.datasets."tank/garage" = {
    mountpoint = "/srv/garage";
    recordsize = "1M";
    "com.sun:auto-snapshot" = "false";
  };
  networking.networkmanager.ensureProfiles.profiles.direct-link = {
    connection = {
      id = "direct-link";
      type = "ethernet";
      autoconnect = true;
      autoconnect-priority = -999;
    };
    ethernet.mac-address = "00:E0:4C:3A:46:58";
    ipv4 = {
      method = "manual";
      address1 = "10.10.10.135/24";
      gateway = "10.10.10.1"; # NATed out the Legion's wifi
      dns = "1.1.1.1;9.9.9.9;";
    };
    ipv6.method = "link-local";
  };

  # no suspend on close, on battery or not, no idle action, and the USB
  # ethernet adapter must never autosuspend - it is the only way in
  services.logind.settings.Login = {
    HandleLidSwitchDocked = "ignore";
    HandlePowerKey = "ignore";
    IdleAction = "ignore";
  };
  systemd.sleep.settings.Sleep = {
    AllowSuspend = "no";
    AllowHibernation = "no";
    AllowHybridSleep = "no";
    AllowSuspendThenHibernate = "no";
  };
  boot.kernelParams = [ "usbcore.autosuspend=-1" ];
}
