{ ... }:
{
  # The forge, its runner, and the mirror that keeps GitHub's copy current.
  imports = [
    ./_sops.nix
    ../modules/forgejo.nix
    ../modules/forgejo-runner.nix
    ../modules/forgejo-mirror.nix
  ];
  dd.forgejo.admin = "david";
  dd.backup.paths = [ "/vault/forgejo" ]; # repositories, lfs, custom config; its db is in the postgres dumps
  dd.forgejo.mirrors = [
    {
      repo = "david/Home-Server";
      to = "https://github.com/DBRobot/Home-Server.git";
      user = "DBRobot";
    }
  ];
}
