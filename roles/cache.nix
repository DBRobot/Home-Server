{ ... }:
{
  # The nix binary cache other boxes and the deploy pull closures from.
  imports = [
    ./_sops.nix
    ../modules/harmonia.nix
  ];
  sops.secrets.harmonia-signing-key = { }; # read by systemd LoadCredential, no owner needed
}
