{
  config,
  pkgs,
  lib,
  ...
}:
let
  base = config.dd.domain;
in
{
  # One runner, on this box, in host mode: a job runs as the runner user with
  # nix on the path and builds straight into this box's store. So the closure
  # a green check just built is already here, and a deploy is a switch.
  #
  # Host mode means a job is code running on node1 as the runner's dynamic
  # user. Fine while every author is the admin; the day strangers can open
  # pull requests here this becomes a container runner or a separate box.
  services.gitea-actions-runner = {
    package = pkgs.forgejo-runner;
    instances.node1 = {
      enable = true;
      name = "node1";
      url = "https://git.${base}";
      # a registration token, minted once with `forgejo actions
      # generate-runner-token` and kept in sops; the runner registers on
      # first start and keeps its own credential in its state dir after
      tokenFile = config.sops.templates."forgejo-runner.env".path;
      labels = [ "nix:host" ];
      hostPackages = with pkgs; [
        bash
        coreutils
        curl
        gawk
        gitMinimal
        gnused
        gnugrep
        gnutar
        gzip
        zstd
        nodejs # javascript actions (checkout) need it
        wget
        nix
        openssh
      ];
    };
  };

  # Not the module's DynamicUser: systemd mounts a dynamic user's state
  # directory noexec (idmapped), and a host-mode job builds and runs things
  # there - every cargo build script died with "Permission denied". A plain
  # system user gets an ordinary directory.
  users.users.forgejo-runner = {
    isSystemUser = true;
    group = "forgejo-runner";
    home = "/var/lib/gitea-runner";
  };
  users.groups.forgejo-runner = { };
  systemd.services.gitea-runner-node1.serviceConfig = {
    DynamicUser = lib.mkForce false;
    User = lib.mkForce "forgejo-runner";
    Group = "forgejo-runner";
  };

  sops.secrets.forgejo-runner-token = { };
  # read by systemd itself, before the unit drops to its dynamic user, so the
  # file can stay root-only
  sops.templates."forgejo-runner.env".content = ''
    TOKEN=${config.sops.placeholder.forgejo-runner-token}
  '';
}
