{
  description = "rust-docs-mcp - Rust documentation MCP server";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";
    flake-parts.url = "github:hercules-ci/flake-parts";

    crane.url = "github:ipetkov/crane";

    fenix = {
      url = "github:nix-community/fenix";
      inputs.nixpkgs.follows = "nixpkgs";
      inputs.rust-analyzer-src.follows = "";
    };
  };

  outputs = inputs @ {
    self,
    flake-parts,
    ...
  }:
    flake-parts.lib.mkFlake {inherit inputs;} {
      systems = ["x86_64-linux" "aarch64-linux" "x86_64-darwin" "aarch64-darwin"];

      perSystem = {
        system,
        config,
        inputs',
        pkgs,
        ...
      }: let
        # Use rust-toolchain.toml for the exact nightly version
        rustToolchain = inputs'.fenix.packages.fromToolchainFile {
          file = ./rust-toolchain.toml;
          sha256 = "sha256-UAoZcxg3iWtS+2n8TFNfANFt/GmkuOMDf7QAE0fRxeA=";
        };

        # Nightly toolchain for runtime (used by the binary to fetch docs)
        rustNightly = rustToolchain;

        craneLib = inputs.crane.mkLib pkgs;

        # Override crane to use nightly toolchain
        cranelibNightly = craneLib.overrideToolchain rustNightly;

        # Get package info from the actual package Cargo.toml
        crateInfo = craneLib.crateNameFromCargoToml {
          cargoToml = ./rust-docs-mcp/Cargo.toml;
        };

        # Shared build args for all crane builds
        commonArgs = {
          inherit (crateInfo) pname version;
          src = craneLib.cleanCargoSource (craneLib.path ./.);
          buildInputs = with pkgs;
            [
              openssl
              pkg-config
            ]
            ++ pkgs.lib.optionals pkgs.stdenv.isDarwin [
              darwin.apple_sdk.frameworks.Security
              darwin.apple_sdk.frameworks.SystemConfiguration
            ];

          nativeBuildInputs = with pkgs; [
            pkg-config
          ];
        };

        # Build dependencies only
        cargoArtifacts = cranelibNightly.buildDepsOnly commonArgs;

        # Build the actual crate
        rust-docs-mcp-unwrapped = cranelibNightly.buildPackage (commonArgs
          // {
            inherit cargoArtifacts;
            doCheck = false; # Tests require network access
          });

        # Wrap the binary to include nightly rust in PATH at runtime
        rust-docs-mcp =
          pkgs.runCommand "rust-docs-mcp" {
            nativeBuildInputs = [pkgs.makeWrapper];
            meta = rust-docs-mcp-unwrapped.meta or {};
          } ''
            mkdir -p $out/bin
            makeWrapper ${rust-docs-mcp-unwrapped}/bin/rust-docs-mcp $out/bin/rust-docs-mcp \
              --prefix PATH : ${rustNightly}/bin
          '';
      in {
        packages = {
          inherit rust-docs-mcp rust-docs-mcp-unwrapped;
          default = rust-docs-mcp;
        };

        devShells.default = cranelibNightly.devShell {
          checks = self.checks.${system};
          packages = with pkgs; [
            rustNightly
            rust-analyzer
            cargo-watch
            cargo-expand
          ];
        };

        # Optional: Add checks
        checks = {
          rust-docs-mcp = rust-docs-mcp-unwrapped;

          rust-docs-mcp-clippy = cranelibNightly.cargoClippy (commonArgs
            // {
              inherit cargoArtifacts;
              cargoClippyExtraArgs = "--all-targets -- --deny warnings";
            });

          rust-docs-mcp-fmt = cranelibNightly.cargoFmt {
            inherit (commonArgs) src;
          };

          rust-docs-mcp-nextest = cranelibNightly.cargoNextest (commonArgs
            // {
              inherit cargoArtifacts;
              partitions = 1;
              partitionType = "count";
            });
        };
      };
    };
}
