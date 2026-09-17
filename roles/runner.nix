{ ... }:
{
  # A CI runner for the forge, registered under this box's name with the
  # nix label. Stateless: any box can run one, jobs land on whichever is
  # free, and CI keeps going while another box is being reinstalled.
  imports = [
    ./_sops.nix
    ../modules/forgejo-runner.nix
  ];
}
