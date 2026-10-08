{
  description = "RefactorLens: paste code, get a modern version, and learn why every change was made";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    flake-utils.url = "github:numtide/flake-utils";
  };

  outputs = { self, nixpkgs, flake-utils }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        pkgs = nixpkgs.legacyPackages.${system};
        manifest = (pkgs.lib.importTOML ./Cargo.toml).package;
      in
      {
        packages.default = pkgs.rustPlatform.buildRustPackage {
          pname = manifest.name;
          version = manifest.version;
          src = pkgs.lib.cleanSource ./.;
          cargoLock.lockFile = ./Cargo.lock;
          # The TLS library (aws-lc) builds C code, so it needs cmake, perl and
          # (on some systems) pkg-config at build time.
          nativeBuildInputs = [ pkgs.cmake pkgs.perl pkgs.pkg-config ];
          meta = {
            description = manifest.description;
            license = pkgs.lib.licenses.mit;
            mainProgram = "refactorlens";
          };
        };

        apps.default = flake-utils.lib.mkApp { drv = self.packages.${system}.default; };

        devShells.default = pkgs.mkShell {
          packages = with pkgs; [
            # Rust toolchain.
            cargo
            rustc
            clippy
            rustfmt
            rust-analyzer
            # Needed to build aws-lc (the rustls backend).
            cmake
            perl
            pkg-config
            # Used by CI to syntax-check ui/app.js.
            nodejs
          ];

          shellHook = ''
            echo "RefactorLens dev shell"
            echo "  cargo   $(cargo --version)"
            echo "  rustc   $(rustc --version)"
            echo "  rust-analyzer $(rust-analyzer --version 2>/dev/null || rust-analyzer --help | head -n1)"
            echo "  node    $(node --version)"
            echo ""
            echo "Run:    cargo run --release   # serves http://127.0.0.1:7878"
            echo "Test:   cargo test"
            echo "Lint:   cargo clippy -- -D warnings"
          '';
        };
      });
}
