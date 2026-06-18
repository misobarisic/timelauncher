{ pkgs ? import <nixpkgs> {} }:

let
  # Import naersk from its GitHub release
  naersk = pkgs.callPackage (fetchTarball {
    url = "https://github.com/nix-community/naersk/archive/master.tar.gz";
    sha256 = "0l6nkhwn21x2cabzjnv1x5pgizj2pyri87480gm71d3k07b57xmd";
  }) {};
in
naersk.buildPackage {
  pname = "timelauncher";
  version = "0.1.0";

  # Points to the root of your Rust project (where Cargo.toml lives)
  src = ./.;

  # Build-time dependencies (if needed)
  nativeBuildInputs = with pkgs; [
    pkg-config
  ];

  buildInputs = with pkgs; [
  ];

  meta = with pkgs.lib; {
    description = "A launcher application for managing time";
    homepage = "https://github.com/yourusername/timelauncher";
    license = licenses.mit;
    maintainers = [];
  };
}
