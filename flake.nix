{
  description = "Home server";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-26.05";
    sops-nix.url = "github:Mic92/sops-nix";
    sops-nix.inputs.nixpkgs.follows = "nixpkgs";
    disko.url = "github:nix-community/disko/latest";
    disko.inputs.nixpkgs.follows = "nixpkgs";
  };

  outputs =
    {
      self,
      nixpkgs,
      sops-nix,
      disko,
      ...
    }:
    let
      # devShells are per-system; nixosConfigurations are not. Both machines
      # here are x86_64-linux, so one system is enough for now.
      system = "x86_64-linux";
      pkgs = nixpkgs.legacyPackages.${system};
    in
    {
      # `nix develop` drops you into a shell with the rust toolchain on PATH.
      # Nothing is installed globally and any machine cloning this repo gets
      # exactly these versions.
      devShells.${system}.default = pkgs.mkShell {
        packages = [
          pkgs.cargo
          pkgs.rustc
          pkgs.rust-analyzer # editor: completion, jump to definition
          pkgs.clippy # linter that teaches you the language
          pkgs.rustfmt
          pkgs.pkg-config # crates with C dependencies need this to find them
          pkgs.sops # `dd secret run -- ...` runs this
          pkgs.age
        ];
        RUST_BACKTRACE = "1";
      };

      # `nix build .#dd` / `nix run .#dd -- status`. Every dependency is
      # fetched by hash from Cargo.lock, so the binary is as reproducible as
      # the nixos closure. ente-accounts is a git dep and carries no checksum
      # in the lockfile, so its hash has to be stated.
      packages.${system} =
        let
          cargoLock = {
            lockFile = ./client/Cargo.lock;
            outputHashes = {
              "ente-accounts-0.0.0" = "sha256-3oQcxIAQ6H2IUXU20T/+ZTsOD1oyrFsM29Pynq8nttw=";
            };
          };
        in
        {
          dd = pkgs.rustPlatform.buildRustPackage {
            pname = "dd";
            version = "0.1.0";
            src = ./client;
            inherit cargoLock;
            cargoBuildFlags = [
              "-p"
              "dd"
              "-p"
              "git-remote-dd" # `git remote add origin dd::...`; dd repo calls it too
            ];
            # the tests of these two, not the workspace's: the helper's test
            # drives the dd binary, which only this derivation builds
            cargoTestFlags = [
              "-p"
              "dd"
              "-p"
              "git-remote-dd"
            ];
            nativeBuildInputs = [ pkgs.pkg-config ];
            # the encrypted-remote and signed-commit tests drive real git and ssh-keygen
            nativeCheckInputs = [
              pkgs.git
              pkgs.openssh
            ];
            # keyring talks to the secret service over dbus at runtime, not build
            # time, so nothing extra is needed here.
          };

          # The verifier behind nginx's auth_request. Built separately from dd
          # rather than as another binary in the same derivation: this one
          # runs on a server and has no business pulling in the keyring/dbus
          # stack that the cli needs.
          verify = pkgs.rustPlatform.buildRustPackage {
            pname = "verify";
            version = "0.1.0";
            src = ./client;
            inherit cargoLock;
            cargoBuildFlags = [
              "-p"
              "verify"
            ];
            cargoTestFlags = [
              "-p"
              "verify"
            ];
            nativeBuildInputs = [ pkgs.pkg-config ];
          };

        };

      # Boxes booted as vms and driven through the failure cases, so the
      # modules the real hosts import are proven before a host sees them.
      # `nix build .#checks.x86_64-linux.directory`; ci runs them when the
      # modules or the verifier change. Not part of a plain `nix flake check`
      # run's build set on a laptop without kvm: `--no-build` there.
      checks.${system} =
        let
          args = {
            inherit pkgs self;
            lib = nixpkgs.lib;
          };
          vm = path: pkgs.testers.runNixOSTest (import path args);
        in
        {
          directory = vm ./tests/directory.nix;
          metrics = vm ./tests/metrics.nix;
          backup = vm ./tests/backup.nix;
          placement = import ./tests/placement.nix args;
          boxes = import ./tests/boxes.nix args;
        };

      # One box per entry in fleet/boxes.json: its hardware file plus its
      # roles. What a box runs is data, not a hand-written import list, and
      # the per-box wiring (who its peers are, which prometheus grafana reads)
      # is derived from the same list.
      nixosConfigurations =
        let
          boxes = builtins.fromJSON (builtins.readFile ./fleet/boxes.json);
          lib = nixpkgs.lib;
          directoryOf =
            name: box:
            if box.public then
              "https://files.distributed-datacenter.duckdns.org/_dd/directory"
            else
              "http://${box.tailnet}:4181/_dd/directory";
          # a role that declares secrets imports roles/_sops.nix; the sops
          # module has to be present for it. Roles are files; look inside.
          needsSops =
            roles: builtins.any (r: lib.hasInfix "_sops.nix" (builtins.readFile ./roles/${r}.nix)) roles;
          storage = lib.filterAttrs (_: b: builtins.elem "storage" b.roles) boxes;
          # the garage cluster: every storage box, each told about the others
          # whose ids are known (a box's id exists once it has started once)
          garageOf =
            name: box:
            {
              dd.garage = {
                zone = box.regionId;
                inherit (box.garage) capacity dataDir;
                publicAddr = "${box.tailnet}:3901";
                peers = lib.mapAttrsToList (_: b: "${b.garage.id}@${b.tailnet}:3901") (
                  lib.filterAttrs (n: b: n != name && b.garage ? id) storage
                );
              };
            };
          # a box without garage backs up to a storage box over the tailnet
          backupEndpoint =
            name: box:
            lib.optionalAttrs (!(builtins.elem "storage" box.roles)) {
              dd.backup.endpoint = "http://${(builtins.head (builtins.attrValues storage)).tailnet}:3900";
            };
          mkBox =
            name: box:
            lib.nixosSystem {
              specialArgs = { inherit self; };
              modules = [
                ./hosts/${name}/hardware.nix
                {
                  networking.hostName = name;
                  dd.box.site = box.siteId;
                  dd.box.region = box.regionId;
                  dd.verify.peers = lib.mapAttrsToList directoryOf (lib.filterAttrs (n: _: n != name) boxes);
                }
              ]
              ++ map (r: ./roles/${r}.nix) box.roles
              ++ lib.optional (builtins.elem "storage" box.roles) (garageOf name box)
              ++ [ (backupEndpoint name box) ]
              ++ lib.optional (needsSops box.roles) sops-nix.nixosModules.sops
              ++ lib.optional (builtins.pathExists ./hosts/${name}/disko.nix) disko.nixosModules.disko
              ++ lib.optional (builtins.elem "observe" box.roles) {
                dd.grafana.boxes = lib.mapAttrs (
                  n: b: if n == name then "http://127.0.0.1:9090" else "http://${b.tailnet}:9090"
                ) boxes;
              };
            };
        in
        lib.mapAttrs mkBox boxes;
    };
}
