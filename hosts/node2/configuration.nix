{ config, lib, pkgs, ... }:
{
  # hardware-configuration.nix is generated on-node2 by nixos-generate-config
  # during install and committed then; it does not exist in the repo yet.
  imports = [
    ./disko.nix
  ];

  boot.loader.systemd-boot.enable = true;
  boot.loader.efi.canTouchEfiVariables = true;
  boot.supportedFilesystems = [ "zfs" ];
  boot.zfs.forceImportRoot = false;
  boot.zfs.extraPools = [
    "tank"
  ];

  networking.hostName = "node2";
  networking.hostId = "083f7ed3";
  networking.networkmanager.enable = true;

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

  time.timeZone = "America/New_York";

  users.users.admin = {
    isNormalUser = true;
    extraGroups = [
      "wheel"
    ];
    openssh.authorizedKeys.keys = [
      "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIKOR0kI8YSFB9JwqTJJMB+h4EJCSscpdnnGGGaBNRqXj david-bascom@david-bascom-Legion-Pro-5-16ADR10"
    ];
  };

  services.tailscale = {
    enable = true;
    openFirewall = true;
  };

  networking.firewall.trustedInterfaces = [ "tailscale0" ];

  services.openssh = {
    enable = true;
    settings.PasswordAuthentication = false;
  };

  security.sudo.wheelNeedsPassword = false;

  nix.settings.experimental-features = [
    "nix-command"
    "flakes"
  ];

  system.stateVersion = "26.05";

  nixpkgs.hostPlatform = lib.mkDefault "x86_64-linux";
}