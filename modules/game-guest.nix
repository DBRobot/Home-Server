# The machine a game server runs in: a small NixOS guest under qemu, the same
# for every game and every instance. What makes it one game and not another
# is a file the host hands it, /instance/instance.json: the steam app to
# install, the program to run, its arguments, where its saves live. The
# program is a big closed binary, often modded; it gets a kernel of its own,
# a disk of its own, and the network qemu gives it, and nothing of the box.
{
  pkgs,
  lib,
  modulesPath,
  ...
}:
{
  imports = [ "${modulesPath}/virtualisation/qemu-vm.nix" ];
  system.stateVersion = "26.05";

  virtualisation = {
    graphics = false;
    # the host overrides both for the instance (QEMU_OPTS: the last -m wins)
    memorySize = 4096;
    cores = 2;
    # sparse: the game's files and steam's live here, per instance
    diskSize = 65536;
    # the instance's directory on the host: its description, and its saves,
    # which so stay ordinary files the box can back up
    sharedDirectories.instance = {
      source = ''"$DD_INSTANCE_DIR"'';
      target = "/instance";
      # files keep the host's ownership: qemu runs as the box's games user,
      # which owns the instance, and the guest's game user has the same id
      securityModel = "none";
    };
  };

  networking.hostName = "game";
  networking.firewall.enable = false; # qemu forwards exactly the instance's ports
  documentation.enable = false;
  services.getty.autologinUser = lib.mkForce null;

  # what a record's exec may name besides a steam app (the test's stand-in)
  environment.systemPackages = [ pkgs.python3 ];

  users.users.game = {
    isSystemUser = true;
    uid = 951;
    group = "game";
    home = "/var/lib/game";
    createHome = true;
  };
  users.groups.game.gid = 951;

  systemd.services.game = {
    description = "The game this instance is";
    wantedBy = [ "multi-user.target" ];
    after = [ "network-online.target" ];
    wants = [ "network-online.target" ];
    path = [
      pkgs.steamcmd
      pkgs.steam-run
      pkgs.jq
      pkgs.coreutils
      pkgs.bash
      # what a record's exec may name as a bare command
      "/run/current-system/sw"
    ];
    serviceConfig = {
      User = "game";
      WorkingDirectory = "/var/lib/game";
      Restart = "on-failure";
      RestartSec = 15;
      # the game's own output, readable on the box beside the record
      StandardOutput = "append:/instance/game.log";
      StandardError = "inherit";
    };
    environment.HOME = "/var/lib/game";
    script = ''
      cfg=/instance/instance.json
      say() { echo "$1" > /instance/status; }
      app=$(jq -r '.steam.appId // empty' "$cfg")
      mkdir -p "$HOME/app"

      # each save path the game declares is a directory on the instance's
      # share, linked into place: saves are files on the box, not bytes in
      # this disk image
      i=0
      jq -r '.saves[]?' "$cfg" | while read -r p; do
        mkdir -p "/instance/saves/$i" "$(dirname "$HOME/$p")"
        [ -L "$HOME/$p" ] || { rm -rf "''${HOME:?}/$p"; ln -s "/instance/saves/$i" "$HOME/$p"; }
        i=$((i + 1))
      done

      if [ -n "$app" ]; then
        say updating
        steamcmd +force_install_dir "$HOME/app" +login anonymous +app_update "$app" +quit
      fi
      say running
      exe=$(jq -r .exec "$cfg")
      mapfile -t args < <(jq -r '.args[]?' "$cfg")
      if [ -n "$app" ]; then
        cd "$HOME/app"
        exec steam-run "./$exe" "''${args[@]}"
      else
        exec "$exe" "''${args[@]}"
      fi
    '';
  };

  # the host asks for a clean stop over acpi; the game gets a minute
  systemd.services.game.serviceConfig.TimeoutStopSec = 60;
}
