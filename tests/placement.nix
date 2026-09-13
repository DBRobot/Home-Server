# Evaluation only: a module that reads plaintext registers itself, and
# registering is all that happens - no box is trusted and none refuses.
{
  pkgs,
  self,
  lib,
  ...
}:
let
  cfg =
    (lib.nixosSystem {
      modules = [
        ./box.nix
        ../modules/jellyfin.nix
        { nixpkgs.hostPlatform = pkgs.stdenv.hostPlatform.system; }
      ];
      specialArgs = { inherit self; };
    }).config;
  labels = cfg.dd.box.plaintext;
  refusals = builtins.filter (a: !a.assertion && lib.hasInfix "plaintext" a.message) cfg.assertions;
in
assert builtins.any (l: lib.hasInfix "jellyfin" l) labels;
assert refusals == [ ];
pkgs.runCommand "plaintext-label" { } "echo ok > $out"
