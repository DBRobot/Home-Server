# Evaluation only, no vm: an untrusted box configured with a plaintext
# service must fail to build, naming the service; a trusted one must not.
{
  pkgs,
  self,
  lib,
  ...
}:
let
  eval =
    trusted:
    (lib.nixosSystem {
      modules = [
        ./box.nix
        ../modules/jellyfin.nix
        {
          dd.box.ownerTrusted = trusted;
          nixpkgs.hostPlatform = pkgs.stdenv.hostPlatform.system;
          boot.loader.grub.enable = false;
          fileSystems."/" = {
            device = "/dev/null";
            fsType = "ext4";
          };
        }
      ];
      specialArgs = { inherit self; };
    }).config.system.build.toplevel.drvPath;
  untrusted = builtins.tryEval (builtins.deepSeq (eval false) true);
  trustedOk = builtins.deepSeq (eval true) true;
in
assert !untrusted.success;
assert trustedOk;
pkgs.runCommand "placement-rule" { } "echo ok > $out"
