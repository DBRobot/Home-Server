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

      nixosConfigurations.node1 = nixpkgs.lib.nixosSystem {
        modules = [
          ./hosts/node1/configuration.nix
          sops-nix.nixosModules.sops
        ];
      };
    };
}
