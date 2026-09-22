{
  description = "Home server";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-26.05";
    sops-nix.url = "github:Mic92/sops-nix";
    sops-nix.inputs.nixpkgs.follows = "nixpkgs";
    disko.url = "github:nix-community/disko/latest";
    disko.inputs.nixpkgs.follows = "nixpkgs";
    # rust builds in two layers: every dependency once, cached in the
    # bucket until Cargo.lock changes; our crates on top, a minute
    crane.url = "github:ipetkov/crane";
    # a toolchain with the wasm32 target, for the browser side
    rust-overlay.url = "github:oxalica/rust-overlay";
    rust-overlay.inputs.nixpkgs.follows = "nixpkgs";
  };

  outputs =
    {
      self,
      nixpkgs,
      sops-nix,
      disko,
      crane,
      rust-overlay,
      ...
    }:
    let
      # devShells are per-system; nixosConfigurations are not. Both machines
      # here are x86_64-linux, so one system is enough for now.
      system = "x86_64-linux";
      pkgs = nixpkgs.legacyPackages.${system};
      # every vm test, by name: nix/tests/<name>.nix + .py. Also what the ci
      # matrix runs and what main requires (nix/modules/forge/forgejo.nix)
      vmTests = [
        "directory"
        "metrics"
        "backup"
        "release"
        "thanos"
        "members-runner"
        "games"
        "forge"
      ];
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
      # the nixos closure. crane builds the dependencies as their own
      # derivation, so a change to our code costs our code's compile, not
      # the three hundred crates under it. The tests ran already in ci's
      # rust job on this same source; no second run in here.
      packages.${system} =
        let
          craneLib = crane.mkLib pkgs;
          # cargo sources, plus the pages' templates, stylesheets, scripts
          # and icons the crates include at build time
          # only the rust: the workspace files and the box/ and client/
          # trees, so a change under nix/ or data/ is not a rust rebuild
          src = pkgs.lib.cleanSourceWith {
            src = ./.;
            filter =
              path: type:
              let
                rel = pkgs.lib.removePrefix (toString ./. + "/") (toString path);
                inRust = builtins.match "(box|client)(/.*)?" rel != null;
                top = builtins.elem rel [
                  "Cargo.toml"
                  "Cargo.lock"
                ];
              in
              top
              || (
                inRust
                && (
                  type == "directory"
                  || craneLib.filterCargoSources path type
                  || builtins.match ".*/(templates|web)/.*" rel != null
                )
              );
          };
          # the same toolchain, plus the wasm32 target; nixpkgs' rustc
          # ships no std for it
          wasmToolchain = (pkgs.extend rust-overlay.overlays.default).rust-bin.stable.latest.minimal.override {
            targets = [ "wasm32-unknown-unknown" ];
          };
          craneWasm = craneLib.overrideToolchain wasmToolchain;
          wasmCommon = {
            inherit src;
            strictDeps = true;
            doCheck = false;
            CARGO_BUILD_TARGET = "wasm32-unknown-unknown";
            cargoExtraArgs = "-p dd-web";
          };
          wasmArtifacts = craneWasm.buildDepsOnly (
            wasmCommon
            // {
              pname = "dd-web-deps";
              version = "0.1.0";
            }
          );
          wasmBuild = craneWasm.buildPackage (
            wasmCommon
            // {
              pname = "dd-web";
              version = "0.1.0";
              cargoArtifacts = wasmArtifacts;
            }
          );
          common = {
            inherit src;
            strictDeps = true;
            nativeBuildInputs = [ pkgs.pkg-config ];
            doCheck = false;
          };
          cargoArtifacts = craneLib.buildDepsOnly (common // { pname = "dd-deps"; version = "0.1.0"; });
          crate = pname: cargoExtraArgs:
            craneLib.buildPackage (
              common
              // {
                inherit pname cargoArtifacts cargoExtraArgs;
                version = "0.1.0";
              }
            );
        in
        {
          # the cli, and `git remote add origin dd::...`, which dd repo calls too
          dd = crate "dd" "-p dd -p git-remote-dd";
          # The release agent, on every box. Server side like verify: the
          # release crate holds the file format the cli signs and this
          # binary checks, and nothing that needs a keyring.
          agent = crate "dd-agent" "-p release";
          # The verifier behind nginx's auth_request. Built separately from dd
          # rather than as another binary in the same derivation: this one
          # runs on a server and has no business pulling in the keyring/dbus
          # stack that the cli needs.
          verify = crate "verify" "-p verify";
          # the manager behind the Games tile (modules/games.nix)
          games = crate "dd-games" "-p games";
          # Our Rust in the browser: the ente account for a person whose key
          # is a passkey (crates/web). The verifier serves this directory.
          web = pkgs.runCommand "dd-web-dist" { nativeBuildInputs = [ pkgs.wasm-bindgen-cli ]; } ''
            mkdir -p $out
            wasm-bindgen --target web --no-typescript --out-dir $out ${wasmBuild}/lib/dd_web.wasm
          '';
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
          vm =
            name:
            (import ./nix/lib/vm-test.nix {
              inherit pkgs vmTests;
              lib = nixpkgs.lib;
              boxNames = builtins.attrNames (builtins.fromJSON (builtins.readFile ./fleet/boxes.json));
            })
              name
              (import ./nix/tests/${name}.nix args);
        in
        {
        }
        // nixpkgs.lib.genAttrs vmTests vm
        // {
          placement = import ./nix/tests/placement.nix args;
          ci = import ./nix/tests/ci.nix (args // { inherit vmTests; });
          boxes = import ./nix/tests/boxes.nix args;
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
            roles: builtins.any (r: lib.hasInfix "_sops.nix" (builtins.readFile ./nix/roles/${r}.nix)) roles;
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
          # closures come from the nix-cache bucket in the garage cluster:
          # through the box's own garage, or a storage box's over the tailnet
          cacheOf =
            name: box:
            let
              host =
                if builtins.elem "storage" box.roles then
                  "127.0.0.1"
                else
                  (builtins.head (builtins.attrValues storage)).tailnet;
            in
            {
              dd.agent.cache = "s3://nix-cache?endpoint=${host}:3900&scheme=http&region=us-east-1";
            };
          # a box without garage backs up to a storage box over the tailnet
          backupEndpoint =
            name: box:
            lib.optionalAttrs (!(builtins.elem "storage" box.roles)) {
              dd.backup.endpoint = "http://${(builtins.head (builtins.attrValues storage)).tailnet}:3900";
            };
          # the gateway's names resolve to its tailnet address on every box,
          # whatever the public name says: the door may be open to the
          # internet, the fleet still talks to itself over the tailnet
          gateway = lib.head (lib.attrNames (lib.filterAttrs (_: b: b.public) boxes));
          namesOf = _: _: {
            networking.hosts.${boxes.${gateway}.tailnet} = builtins.filter (n: n != "_") (
              builtins.attrNames self.nixosConfigurations.${gateway}.config.services.nginx.virtualHosts
            );
          };
          mkBox =
            name: box:
            lib.nixosSystem {
              specialArgs = {
                inherit self vmTests;
                boxNames = builtins.attrNames boxes;
                # a unit's script from a file beside its module (nix/lib/script.nix)
                ddScript = import ./nix/lib/script.nix { lib = nixpkgs.lib; };
              };
              modules = [
                ./nix/hosts/${name}/hardware.nix
                {
                  networking.hostName = name;
                  dd.box.tailnet = box.tailnet;
                  dd.box.site = box.siteId;
                  dd.box.region = box.regionId;
                  dd.verify.peers = lib.mapAttrsToList directoryOf (lib.filterAttrs (n: _: n != name) boxes);
                }
              ]
              ++ map (r: ./nix/roles/${r}.nix) box.roles
              ++ lib.optional (builtins.elem "storage" box.roles) (garageOf name box)
              ++ [
                (backupEndpoint name box)
                (cacheOf name box)
                (namesOf name box)
              ]
              ++ lib.optional (needsSops box.roles) sops-nix.nixosModules.sops
              ++ lib.optional (builtins.pathExists ./nix/hosts/${name}/disko.nix) disko.nixosModules.disko
              ++ lib.optional (builtins.elem "observe" box.roles) {
                dd.grafana.boxes = lib.mapAttrs (
                  n: b: if n == name then "http://127.0.0.1:9090" else "http://${b.tailnet}:9090"
                ) boxes;
                # every box's sidecar, for the fleet-wide query
                dd.thanos.sidecars = lib.mapAttrsToList (
                  n: b: if n == name then "127.0.0.1:10901" else "${b.tailnet}:10901"
                ) boxes;
              };
            };
        in
        lib.mapAttrs mkBox boxes;
    };
}
