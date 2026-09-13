{ config, pkgs, ... }:
{
  imports = [
    ./hardware-configuration.nix
    ../../modules/domain.nix
    ../../modules/zfs-datasets.nix
    ../../modules/llama-cpp.nix
    ../../modules/secrets.nix
    ../../modules/garage.nix
    ../../modules/ente.nix
    ../../modules/harmonia.nix
    ../../modules/acme.nix
    ../../modules/mail.nix
    ../../modules/alerting.nix
    ../../modules/metrics.nix
    ../../modules/grafana.nix
    ../../modules/postgres-backup.nix
    ../../modules/media-tier.nix
    ../../modules/jellyfin.nix
    ../../modules/archive-catalog.nix
    ../../modules/user-accounts.nix
    ../../modules/kanidm.nix
    ../../modules/llm-auth.nix
    ../../modules/webdav-media.nix
    ../../modules/kanidm-service-accounts.nix
    ../../modules/kanidm-mail-sender.nix
    ../../modules/kanidm-key-rotation.nix
    ../../modules/signup.nix
    ../../modules/verify.nix
  ];

  boot.loader.systemd-boot.enable = true;
  boot.loader.efi.canTouchEfiVariables = true;
  boot.supportedFilesystems = [ "zfs" ];
  boot.zfs.forceImportRoot = false;
  boot.zfs.extraPools = [
    "tank"
    "vault"
  ];

  # default c_max is ~all of RAM (measured 61.5G); with no swap that collides
  # with the model's mlocked 23G. Raise once photos land on the HDD pool.
  boot.extraModprobeConfig = ''
    options zfs zfs_arc_max=8589934592
  '';

  dd.domain = "distributed-datacenter.duckdns.org";

  networking.hostName = "node1";
  networking.hostId = "0195f284";
  networking.networkmanager.enable = true;

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

  time.timeZone = "America/New_York";

  users.users.admin = {
    isNormalUser = true;
    extraGroups = [
      "wheel"
      "media" # copy files into /srv/media without sudo
    ];
    openssh.authorizedKeys.keys = [
      "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIKOR0kI8YSFB9JwqTJJMB+h4EJCSscpdnnGGGaBNRqXj david-bascom@david-bascom-Legion-Pro-5-16ADR10"
    ];
  };

  services.tailscale = {
    enable = true;
    openFirewall = true;
    permitCertUid = "nginx"; # so nginx can fetch *.ts.net certs without root
  };

  # tailscale0 is the trusted side; the direct cable stays as it is
  networking.firewall.trustedInterfaces = [ "tailscale0" ];

  services.openssh = {
    enable = true;
    settings.PasswordAuthentication = false;
  };

  security.sudo.wheelNeedsPassword = false;

  services.zfs.autoSnapshot.enable = true;
  services.zfs.autoScrub.enable = true;
  services.logind.settings.Login.HandleLidSwitch = "ignore";
  services.logind.settings.Login.HandleLidSwitchExternalPower = "ignore";

  nix.settings.experimental-features = [
    "nix-command"
    "flakes"
  ];

  environment.systemPackages = with pkgs; [
    vim
    git
    htop
  ];

  system.stateVersion = "26.05";
}
