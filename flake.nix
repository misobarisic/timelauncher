{
  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/26.05";
    crane.url = "github:ipetkov/crane";
    flake-utils.url = "github:numtide/flake-utils";
  };

  outputs = {
    self,
    nixpkgs,
    crane,
    flake-utils,
    ...
  }: let
    # 1. Define the core build function that can take ANY pkgs
    mkTimelauncher = {pkgs}: let
      craneLib = crane.mkLib pkgs;
      commonArgs = {
        src = craneLib.cleanCargoSource ./.;
        strictDeps = true;
        buildInputs = [];
      };
    in
      craneLib.buildPackage (
        commonArgs
        // {
          cargoArtifacts = craneLib.buildDepsOnly commonArgs;
        }
      );
  in
    # 2. Wire up the standard flake outputs using the fallback default pkgs
    flake-utils.lib.eachDefaultSystem (
      system: let
        # Default fallback pkgs if evaluated standalone
        defaultPkgs = nixpkgs.legacyPackages.${system};

        # Build scrollsaw using the standalone fallback
        timelauncherDefault = mkTimelauncher {pkgs = defaultPkgs;};
      in {
        checks = {timelauncher = timelauncherDefault;};

        # standalone default package output (uses pinned flake nixpkgs)
        packages.default = timelauncherDefault;

        # Expose the builder function so other flakes can pass their global pkgs!
        lib.mkPackage = mkTimelauncher;

        apps.default = flake-utils.lib.mkApp {drv = timelauncherDefault;};

        devShells.default = (crane.mkLib defaultPkgs).devShell {
          checks = self.checks.${system};
          packages = [];
        };
      }
    );
}
