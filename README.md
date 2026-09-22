# Home-Server

NixOS configuration for a self-hosted family server. Everything here is
declarative: the machines are rebuilt from this repository, and secrets are
committed encrypted rather than kept out of tree.

## Layout

    flake.nix          entry point; `nixosConfigurations.<box>` from fleet/boxes.json
    fleet/             what the fleet is: the boxes, the members
    nix/               what a box is
      modules/         NixOS modules, grouped by concern (box, gate, forge, games, ...)
      roles/           what a box runs, by role; a box lists its roles in fleet/
      hosts/           per-machine hardware
      tests/           VM tests: boxes booted and driven
    box/               Rust that runs on boxes: the agent, the gate, the games manager
    client/            Rust a person runs: the dd cli, its libraries, the browser wasm
    data/              the games catalogue and covers
    scripts/           deploy, install-box, restore-box
    secrets/           sops-encrypted values (safe to publish)

## Deploying

    nixos-rebuild switch --flake github:DBRobot/Home-Server#node1 --refresh

`--refresh` matters: nix caches flake tarballs for an hour and will otherwise
silently redeploy the previous revision.

## License

Copyright (C) 2026 David Bascom

This program is free software: you can redistribute it and/or modify it under
the terms of the GNU Affero General Public License as published by the Free
Software Foundation, either version 3 of the License, or (at your option) any
later version.

This program is distributed in the hope that it will be useful, but WITHOUT ANY
WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A
PARTICULAR PURPOSE. See the GNU Affero General Public License for more details.

You should have received a copy of the GNU Affero General Public License along
with this program. If not, see <https://www.gnu.org/licenses/>.
