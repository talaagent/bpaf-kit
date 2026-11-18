{
  description = "bpaf-kit: batteries for Rust CLIs built on bpaf";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs =
    {
      self,
      nixpkgs,
      rust-overlay,
    }:
    let
      inherit (nixpkgs) lib;
      systems = [
        "x86_64-linux"
        "aarch64-linux"
        "x86_64-darwin"
        "aarch64-darwin"
      ];
      forAll =
        f:
        lib.genAttrs systems (
          system:
          f (
            import nixpkgs {
              inherit system;
              overlays = [ (import rust-overlay) ];
            }
          )
        );
      rustToolchain = pkgs: pkgs.rust-bin.fromRustupToolchainFile ./rust-toolchain.toml;
      rustPlatform =
        pkgs:
        pkgs.makeRustPlatform {
          cargo = rustToolchain pkgs;
          rustc = rustToolchain pkgs;
        };

      # `YYYY-MM-DD` from a flake's lastModifiedDate (`YYYYMMDDhhmmss`).
      flakeDate =
        src:
        let
          d = src.lastModifiedDate or null;
        in
        if d == null then
          null
        else
          "${builtins.substring 0 4 d}-${builtins.substring 4 2 d}-${builtins.substring 6 2 d}";
    in
    {
      lib = {
        # withVersionFile pkgs { package = ...; rev = ...; dirty = ...; date = ...; }
        withVersionFile = pkgs: pkgs.callPackage ./nix/with-version-file.nix { };
        # The rev/dirty/date arguments for withVersionFile from a flake's `self`.
        flakeVersionArgs = src: {
          rev = src.rev or (if src ? dirtyRev then lib.removeSuffix "-dirty" src.dirtyRev else null);
          dirty = src ? dirtyRev;
          date = flakeDate src;
        };
      };

      packages = forAll (pkgs: rec {
        # Built without git info, so it only rebuilds when sources change.
        demo-unwrapped = (rustPlatform pkgs).buildRustPackage {
          pname = "demo";
          version = (lib.importTOML ./Cargo.toml).workspace.package.version;
          src = lib.cleanSource ./.;
          cargoLock.lockFile = ./Cargo.lock;
          cargoBuildFlags = [
            "-p"
            "demo"
          ];
          doCheck = false;
          postInstall = ''
            mkdir -p $out/share/man/man1
            $out/bin/demo --man > $out/share/man/man1/demo.1
          '';
          meta.mainProgram = "demo";
        };
        demo = self.lib.withVersionFile pkgs (
          { package = demo-unwrapped; } // self.lib.flakeVersionArgs self
        );
        default = demo;
      });

      devShells = forAll (pkgs: {
        default = pkgs.mkShell {
          packages = [
            (rustToolchain pkgs)
            pkgs.cargo-nextest
            pkgs.cargo-deny
          ];
          RUST_SRC_PATH = "${rustToolchain pkgs}/lib/rustlib/src/rust/library";
        };
      });

      formatter = forAll (pkgs: pkgs.nixfmt);
    };
}
