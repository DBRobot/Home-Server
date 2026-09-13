# Evaluation only, no vm: an untrusted box configured with a plaintext
# service must trip the placement assertion, naming the service; a trusted
# one must not. Only the assertions are evaluated, not the whole system,
# so this needs no certificates, secrets or disks.
{
  pkgs,
  self,
  lib,
  ...
}:
let
  assertionsFor =
    trusted:
    (lib.nixosSystem {
      modules = [
        ./box.nix
        ../modules/jellyfin.nix
        {
          dd.box.ownerTrusted = trusted;
          nixpkgs.hostPlatform = pkgs.stdenv.hostPlatform.system;
        }
      ];
      specialArgs = { inherit self; };
    }).config.assertions;
  tripped =
    trusted:
    builtins.filter (a: !a.assertion && lib.hasInfix "not owner-trusted" a.message) (
      assertionsFor trusted
    );
  untrusted = tripped false;
  trustedOk = tripped true == [ ];
in
assert builtins.length untrusted == 1;
assert lib.hasInfix "jellyfin" (builtins.head untrusted).message;
assert trustedOk;
pkgs.runCommand "placement-rule" { } "echo ok > $out"
