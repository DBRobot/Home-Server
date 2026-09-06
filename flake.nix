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
    {
      nixosConfigurations.node1 = nixpkgs.lib.nixosSystem {
        modules = [
          ./hosts/node1/configuration.nix
          sops-nix.nixosModules.sops
        ];
      };
    };
}
