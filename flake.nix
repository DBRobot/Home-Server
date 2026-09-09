{
  description = "Home server";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-26.05";
    sops-nix.url = "github:Mic92/sops-nix";
    sops-nix.inputs.nixpkgs.follows = "nixpkgs";
  };

  outputs =
    {
      self,
      nixpkgs,
      sops-nix,
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
            ];
            nativeBuildInputs = [ pkgs.pkg-config ];
            # keyring talks to the secret service over dbus at runtime, not build
            # time, so nothing extra is needed here.
          };

          # Built separately from dd rather than as another binary in the same
          # derivation: this one runs on a server and has no business pulling in
          # the keyring/dbus stack that the cli needs.
          signup = pkgs.rustPlatform.buildRustPackage {
            pname = "signup";
            version = "0.1.0";
            src = ./client;
            inherit cargoLock;
            cargoBuildFlags = [
              "-p"
              "signup"
            ];
            nativeBuildInputs = [ pkgs.pkg-config ];
          };
        };

      nixosConfigurations.node1 = nixpkgs.lib.nixosSystem {
        # modules/signup.nix runs a binary built from this same flake, so it
        # needs a way to name it. specialArgs rather than an overlay because
        # there is exactly one such package and an overlay would rebuild the
        # world's pkgs to deliver it.
        specialArgs = { inherit self; };
        modules = [
          ./hosts/node1/configuration.nix
          sops-nix.nixosModules.sops
        ];
      };
    };
}
