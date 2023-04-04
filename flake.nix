{
  inputs = {
    crane = {
      url = "github:ipetkov/crane";
      inputs = {
        flake-utils.follows = "flake-utils";
        nixpkgs.follows = "nixpkgs";
      };
    };
    flake-utils.url = "github:numtide/flake-utils";
    nixpkgs.url = "nixpkgs/nixos-unstable";
  };

  outputs = { self, crane, flake-utils, nixpkgs }:
    flake-utils.lib.eachDefaultSystem (system: {
      packages.default =
        let
          pkgs = (import nixpkgs) {
            inherit system;
          };

          craneLib = crane.lib.${system};
        in

        craneLib.buildPackage {
          doCheck = false;
          src = ./.;

          buildInputs = with pkgs; [
            clang_15
            cargo-outdated
            cargo-deny
            cargo-watch
            openssl
            mold
            sqlite
            flyctl
          ];

          nativeBuildInputs = with pkgs; [
            pkg-config
          ];

        };
    });
}