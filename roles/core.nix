{ config, lib, ... }:
{
  # What every box is, before any role: its identity in the directory, a
  # replica of the directory, its own metrics, and a way in for the admin.
  imports = [
    ../modules/domain.nix
    ../modules/box.nix
    ../modules/verify.nix
    ../modules/metrics.nix
    ../modules/zfs-datasets.nix
    ../modules/backup.nix
    ../modules/agent.nix
  ];

  # the box moves itself to the signed release; the key that signs lives on
  # the laptop that builds, its public half in the repo
  dd.agent = {
    enable = true;
    url = "https://git.${config.dd.domain}/david/Home-Server/raw/branch/releases/current.json";
    publicKey = lib.fileContents ../fleet/release.pub;
  };

  # prometheus history cannot be backfilled, so it is in the box's backup
  dd.backup.paths = [ "/var/lib/prometheus2" ];
  dd.domain = "distributed-datacenter.duckdns.org";
  # every box holds a directory replica; the gateway role turns this into
  # the full verifier with the browser login
  dd.verify.role = lib.mkDefault "directory";

  boot.loader.systemd-boot.enable = true;
  boot.loader.efi.canTouchEfiVariables = true;
  boot.supportedFilesystems = [ "zfs" ];
  # The pool that holds root was created by the installer, under the
  # installer's host id. The first boot of a template box would refuse it
  # ("previously in use from another system") and stop in the initrd; with
  # no other system on the disk that refusal protects nothing. Force it.
  boot.zfs.forceImportRoot = true;

  time.timeZone = "America/New_York";

  users.users.admin = {
    isNormalUser = true;
    extraGroups = [ "wheel" ];
    openssh.authorizedKeys.keys = [
      "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIKOR0kI8YSFB9JwqTJJMB+h4EJCSscpdnnGGGaBNRqXj david-bascom@david-bascom-Legion-Pro-5-16ADR10"
    ];
  };

  services.tailscale = {
    enable = true;
    openFirewall = true;
  };
  # tailscale0 is the trusted side; whatever cable or wifi a box has stays as it is
  networking.firewall.trustedInterfaces = [ "tailscale0" ];
  networking.networkmanager.enable = true;
  # never invent a "Wired connection 1": on a first boot NetworkManager can
  # see the cable before the declared profile exists, make a dhcp profile
  # for it, and keep preferring that one. node2 came up on a dhcp address
  # after a reinstall this way; a box has the profiles its hardware file
  # declares and nothing else
  networking.networkmanager.settings.main.no-auto-default = "*";

  services.openssh = {
    enable = true;
    settings.PasswordAuthentication = false;
  };
  security.sudo.wheelNeedsPassword = false;

  # a laptop being a server: the lid is furniture
  services.logind.settings.Login.HandleLidSwitch = "ignore";
  services.logind.settings.Login.HandleLidSwitchExternalPower = "ignore";

  nix.settings.experimental-features = [
    "nix-command"
    "flakes"
  ];

  system.stateVersion = "26.05";
}
