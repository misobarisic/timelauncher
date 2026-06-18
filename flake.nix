{
  description = "HakuNeko Desktop Flake";

  inputs = {
    nixpkgs.url = "github:nixos/nixpkgs/nixos-unstable";
    flake-utils.url = "github:numtide/flake-utils";
  };

  outputs = { self, nixpkgs, flake-utils }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        pkgs = import nixpkgs { inherit system; };
      in
      {
        # Drops the package straight into `nix build`
        packages.default = pkgs.callPackage ./default.nix { };

        # Creates a `nix develop` environment with the package's build dependencies
        devShells.default = pkgs.mkShell {
          inputsFrom = [ self.packages.${system}.default ];

          # Add extra debugging tools here if you want them during development
          nativeBuildInputs = with pkgs; [ ];
        };
      });
}
