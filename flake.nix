{
  nixConfig = {
    extra-trusted-substituters = [ "https://nix-community.cachix.org" ];
    extra-trusted-public-keys = [ "nix-community.cachix.org-1:mB9FSh9qf2dCimDSUo8Zy7bkq5CX+/rkCWyvRCYg3Fs=" ];
  };

  inputs = {
    nixpkgs.url = "github:nixos/nixpkgs/nixos-25.05"; # Latest stable release

    fenix = {
      url = "github:nix-community/fenix";
      inputs.nixpkgs.follows = "nixpkgs";
    };

    crane = {
      url = "github:ipetkov/crane";
    };

    advisory-db = {
      url = "github:rustsec/advisory-db";
      flake = false;
    };

    flake-parts.url = "github:hercules-ci/flake-parts";
  };

  outputs = inputs@{ self, fenix, crane, flake-parts, advisory-db, ... }:
    flake-parts.lib.mkFlake { inherit self inputs; } ({ withSystem, ... }: {
      systems = [
        "x86_64-linux"
        "x86_64-darwin"
        "aarch64-linux"
        "aarch64-darwin"
      ];

      perSystem = { lib, config, self', inputs', pkgs, system, ... }:
        let
          rustToolchain = fenix.packages.${system}.stable.toolchain;
          craneLib = (crane.mkLib pkgs).overrideToolchain rustToolchain;

          # File filters for including additional files in the build
          includeFileFilter = path: type:
            let
              # Define file extensions to include
              extensions = [ "md" "json" "proto" ];
              # Check if path matches any of the extensions
              matchesExtension = builtins.any (ext: 
              builtins.match (".*\\." + ext + "$") path != null
              ) extensions;
            in
            matchesExtension || (craneLib.filterCargoSources path type);

          version = (builtins.fromTOML (builtins.readFile ./Cargo.toml)).workspace.package.version;

          commonArgs = {
            src = lib.cleanSourceWith {
              src = ./.;
              filter = includeFileFilter;
              name = "source";
            };
            pname = "rs1090";
            version = version;

            nativeBuildInputs = with pkgs; [
              pkg-config
              openssl
              python3
              bzip2
              # soapysdr
              protobuf
            ] ++ lib.optionals pkgs.stdenv.isLinux [
              lld
              rustPlatform.bindgenHook # issue with stdbool.h on nix flake check
            ];

            buildInputs = with pkgs; [
              # soapysdr
            ] ++ lib.optionals pkgs.stdenv.isDarwin [
              libiconv
            ];

            # Minimal environment
            LIBCLANG_PATH = "${pkgs.libclang.lib}/lib";

            RUSTFLAGS = lib.optionalString pkgs.stdenv.isLinux
              "-C linker=${pkgs.gcc}/bin/gcc -C link-arg=--verbose";
          };

          cargoArtifacts = craneLib.buildDepsOnly commonArgs;
        in
        {
          devShells.default = pkgs.mkShell {
            inputsFrom = builtins.attrValues self.checks;
            buildInputs = with pkgs; [ rustToolchain pkg-config openssl ] ++
              commonArgs.buildInputs;
            nativeBuildInputs = commonArgs.nativeBuildInputs;

          };

          packages = {
            default = self'.packages.jet1090;

            jet1090 = craneLib.buildPackage (commonArgs // {
              pname = "jet1090";
              cargoExtraFlags = "--features rtlsdr,ssh,sero -p jet1090";
              meta.mainProgram = "jet1090";
              inherit cargoArtifacts;

              # disable the default check phase (which includes doctests)
              # On macOS, we need to run checks to ensure compatibility
              # On Linux, we disable checks due to linking issues at the
              #   doctest stage. (TODO work on a fix for this)
              doCheck = pkgs.stdenv.isDarwin;
            });

          };

          checks =
            {
              fmt = craneLib.cargoFmt (commonArgs);
              # audit = craneLib.cargoAudit (commonArgs // { inherit advisory-db; });
              rustdoc = craneLib.cargoDoc (commonArgs // { inherit cargoArtifacts; });

              clippy-check = craneLib.cargoClippy (commonArgs // {
                inherit cargoArtifacts;
                cargoClippyExtraArgs = "--all-features -- --deny warnings";
              });

              test-check = craneLib.cargoNextest (commonArgs // {
                inherit cargoArtifacts;
                partitions = 1;
                partitionType = "count";
              });
            }
            # build packages as part of the checks
            // (lib.mapAttrs' (key: value: lib.nameValuePair (key + "-package") value) self'.packages);

          formatter = pkgs.nixpkgs-fmt;
        };
    });
}
