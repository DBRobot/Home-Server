{ ... }:
{
  # The forge and the mirror that keeps GitHub's copy current. Its runner
  # is the runner role: any box can run one, the forge runs on one.
  imports = [
    ./_sops.nix
    ../modules/forgejo.nix
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
