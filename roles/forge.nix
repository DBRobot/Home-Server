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
  dd.forgejo.mirrors = [
    {
      repo = "david/Home-Server";
      to = "https://github.com/DBRobot/Home-Server.git";
      user = "DBRobot";
    }
  ];
}
