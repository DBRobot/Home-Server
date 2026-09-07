# Home-Server

NixOS configuration for a self-hosted family server. Everything here is
declarative: the machines are rebuilt from this repository, and secrets are
committed encrypted rather than kept out of tree.

## Layout

    flake.nix          entry point; `nixosConfigurations.node1`
    hosts/             per-machine configuration
    modules/           services: ente, garage, harmonia, llama-cpp, zfs, secrets
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
