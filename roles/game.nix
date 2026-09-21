{ config, ... }:
{
  # Game servers a member starts for themselves (modules/games.nix). What is
  # here is what can run: the catalogue. Every entry is a dedicated server
  # steamcmd installs anonymously. That a given server exists is the box's
  # state, not the repo's.
  imports = [ ../modules/games.nix ];

  dd.games.catalogue = {
    satisfactory = {
      name = "Satisfactory";
      description = "A factory with friends. Claim the server in the game's server manager the first time you connect.";
      appId = 1690800;
      exec = "FactoryServer.sh";
      args = [
        "-multihome=0.0.0.0"
        "-Port=7777"
        "-log"
        "-unattended"
      ];
      ports = [
        {
          proto = "udp";
          guest = 7777;
        }
        {
          proto = "tcp";
          guest = 7777;
        }
      ];
      saves = [ ".config/Epic/FactoryGame/Saved/SaveGames" ];
      memory = 12288;
      cores = 4;
    };
    valheim = {
      name = "Valheim";
      description = "A viking world for up to ten. Join by address from the game's server list.";
      appId = 896660;
      exec = "valheim_server.x86_64";
      args = [
        "-nographics"
        "-batchmode"
        "-name"
        "Distributed Datacenter"
        "-port"
        "2456"
        "-world"
        "World"
        "-password"
        "changeme-in-game"
        "-public"
        "0"
      ];
      ports = [
        {
          proto = "udp";
          guest = 2456;
        }
        {
          proto = "udp";
          guest = 2457;
        }
      ];
      saves = [ ".config/unity3d/IronGate/Valheim" ];
      memory = 4096;
      cores = 2;
    };
  };
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

  # the worlds and the records, not the game files in each guest's disk
  dd.backup.paths = [ "/var/lib/dd-games/instances" ];
  dd.backup.exclude = [ "/var/lib/dd-games/instances/*/disk.qcow2" ];
}
