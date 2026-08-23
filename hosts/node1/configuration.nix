{ config, pkgs, ... }:
{
    imports = [ ./hardware-configuration.nix ];

    boot.loader.systemd-boot.enable = true;
    boot.loader.efi.canTouchEfiVariables = true;
    boot.supportedFilesystems = [ "zfs" ];
    boot.zfs.forceImportRoot = false;

    networking.hostName = "node1";
    networking.hostId = "0195f284";
    networking.networkmanager.enable = true;

    time.timeZone = "America/New_York";

    users.users.admin = {
        isNormalUser = true;
        extraGroups = [ "wheel" ];
        openssh.authorizedKeys.keys = [
            "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIKOR0kI8YSFB9JwqTJJMB+h4EJCSscpdnnGGGaBNRqXj david-bascom@david-bascom-Legion-Pro-5-16ADR10"
        ];
    };

    services.openssh = {
        enable = true;
        settings.PasswordAuthentication = false;
    };

    security.sudo.wheelNeedsPassword = false;

    services.logind.settings.Login.HandleLidSwitch = "ignore";
    services.logind.settings.Login.HandleLidSwitchExternalPower = "ignore";

    environment.systemPackages = with pkgs; [ vim git htop ];

    system.stateVersion = "26.05";
}