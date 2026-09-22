{ config, ... }:
{
  # Game servers a member starts for themselves (modules/games.nix): every
  # steam dedicated server the pelican eggs know, installed anonymously by
  # steamcmd in a guest of its own. That a given server exists is the box's
  # state, not the repo's.
  imports = [ ../modules/games.nix ];

  dd.games.enable = true;
  # node1's 62G has room for a couple of worlds beside everything else
  dd.games.memoryMiB = 20480;

  dd.home.services = [
    {
      name = "Games";
      url = "https://games.${config.dd.domain}/";
      description = "Start a server for you and your friends. It stays up.";
      icon = "games";
      color = "#7a4fb5";
      rank = 35;
      # the demo may look at what runs here, not start anything
      demo = "read";
    }
  ];

  # each server's directory (the game and its world, as the egg lays them
  # out) and its record; not the guest's own disk
  dd.backup.paths = [ "/var/lib/dd-games/instances" ];
  dd.backup.exclude = [ "/var/lib/dd-games/instances/*/disk.qcow2" ];
}
