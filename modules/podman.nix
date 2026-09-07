{ ... }:
{
  # Container runtime for services that have no nixpkgs module of their own.
  # Podman rather than docker: no daemon, so each container is an ordinary
  # systemd unit with no second source of truth for state. It is also what
  # oci-containers defaults to on any stateVersion >= 22.05.
  virtualisation.podman = {
    enable = true;
    # Images are pinned by digest, so old ones accumulate on every bump and
    # nothing ever collects them.
    autoPrune = {
      enable = true;
      dates = "weekly";
    };
  };

  virtualisation.oci-containers.backend = "podman";
}
