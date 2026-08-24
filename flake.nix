{
  description = "Home server";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-26.05";
  };

  outputs = { self, nixpkgs, ... }: {
    nixosConfigurations.node1 = nixpkgs.lib.nixosSystem {
      modules = [ ./hosts/node1/configuration.nix ];
    };
  };
}
