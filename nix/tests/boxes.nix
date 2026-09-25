# Evaluation only: the box list is well-formed. Every role a box names is a
# file, every box has core, every box builds, and the wiring the flake
# derives from the list (peers, grafana's datasources) says what the list
# says.
{
  pkgs,
  self,
  lib,
  ...
}:
let
  boxes = builtins.fromJSON (builtins.readFile ../../fleet/boxes.json);
  cfgs = self.nixosConfigurations;
  ok = builtins.all (x: x) (
    lib.flatten (
      lib.mapAttrsToList (
        name: box:
        [
          (builtins.elem "core" box.roles)
          (builtins.all (r: builtins.pathExists (../roles + "/${r}.nix")) box.roles)
          (builtins.hasAttr name cfgs)
          (cfgs.${name}.config.networking.hostName == name)
          # peers: every other box, none of itself
          (
            builtins.length cfgs.${name}.config.dd.verify.peers
            == builtins.length (builtins.attrNames boxes) - 1
          )
          (builtins.all (p: !lib.hasInfix box.tailnet p) cfgs.${name}.config.dd.verify.peers)
          # the keyboard way in: a hash nixos will actually write. With
          # mutable users it writes one only for a user it is creating, so
          # the two go together or the console password is decoration
          (cfgs.${name}.config.users.users.admin.hashedPasswordFile != null)
          (cfgs.${name}.config.users.mutableUsers == false)
        ]
        ++ lib.optionals (builtins.elem "storage" box.roles) [
          # garage: this box's own address is public, its peers never include itself
          (cfgs.${name}.config.dd.garage.publicAddr == "${box.tailnet}:3901")
          (builtins.all (p: !lib.hasInfix box.tailnet p) cfgs.${name}.config.dd.garage.peers)
          (cfgs.${name}.config.dd.garage.zone == box.regionId)
        ]
        ++ lib.optional (builtins.elem "observe" box.roles) (
          builtins.attrNames cfgs.${name}.config.dd.grafana.boxes == builtins.attrNames boxes
        )
      ) boxes
    )
  );
in
assert ok;
pkgs.runCommand "box-list" { } "echo ok > $out"
