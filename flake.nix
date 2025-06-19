{
  description = "rust-docs-mcp - Rust documentation MCP server";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";
    flake-parts.url = "github:hercules-ci/flake-parts";
    
    crane = {
      url = "github:ipetkov/crane";
      inputs.nixpkgs.follows = "nixpkgs";
    };
    
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
        toolchain = inputs'.fenix.packages.latest.toolchain;
        rustNightly = inputs'.fenix.packages.complete.withComponents [
          "cargo"
          "clippy"
          "rust-src"
          "rustc"
          "rustfmt"
        ];
        
        craneLib = inputs.crane.mkLib pkgs;
        
        # Override crane to use nightly toolchain
        cranelibNightly = craneLib.overrideToolchain rustNightly;
        
        # Shared build args for all crane builds
        commonArgs = {
          src = craneLib.cleanCargoSource (craneLib.path ./.);
          buildInputs = with pkgs; [
            openssl
            pkg-config
          ] ++ pkgs.lib.optionals pkgs.stdenv.isDarwin [
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
        rust-docs-mcp = cranelibNightly.buildPackage (commonArgs // {
          inherit cargoArtifacts;
        });
      in {
        packages = {
          inherit rust-docs-mcp;
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
          inherit rust-docs-mcp;
          
          rust-docs-mcp-clippy = cranelibNightly.cargoClippy (commonArgs // {
            inherit cargoArtifacts;
            cargoClippyExtraArgs = "--all-targets -- --deny warnings";
          });
          
          rust-docs-mcp-fmt = cranelibNightly.cargoFmt {
            inherit (commonArgs) src;
          };
          
          rust-docs-mcp-nextest = cranelibNightly.cargoNextest (commonArgs // {
            inherit cargoArtifacts;
            partitions = 1;
            partitionType = "count";
          });
        };
      };
    };
}