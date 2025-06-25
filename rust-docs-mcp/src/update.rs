//! Update functionality for rust-docs-mcp
//!
//! This module provides functionality to update rust-docs-mcp to the latest version
//! from GitHub, similar to the install.sh script but built into the application.

use anyhow::{Context, Result};
use std::path::PathBuf;
use std::process::Command;

/// Update rust-docs-mcp to the latest version from GitHub
pub async fn update_executable(
    target_dir: Option<PathBuf>,
    repo_url: Option<String>,
    branch: Option<String>,
) -> Result<()> {
    // Configuration
    let repo_url = repo_url.unwrap_or_else(|| {
        // Use the repository URL from Cargo.toml metadata
        env!("CARGO_PKG_REPOSITORY").to_string()
    });
    let branch = branch.unwrap_or_else(|| "main".to_string());

    // Determine target directory
    let target_dir = match target_dir {
        Some(dir) => dir,
        None => {
            let home = dirs::home_dir()
                .ok_or_else(|| anyhow::anyhow!("Could not determine home directory"))?;
            home.join(".local").join("bin")
        }
    };

    println!("🦀 rust-docs-mcp Updater");
    println!("=========================");

    // Check for required tools
    check_command_exists("git")?;
    check_command_exists("cargo")?;

    // Check if nightly toolchain is available
    check_nightly_toolchain()?;

    // Create temporary directory
    let temp_dir = tempfile::TempDir::new().context("Failed to create temporary directory")?;
    let temp_path = temp_dir.path();

    println!("📦 Cloning rust-docs-mcp repository...");

    // Clone repository
    let clone_output = Command::new("git")
        .args(["clone", "--depth", "1", "--branch", &branch, &repo_url])
        .arg(temp_path.join("rust-docs-mcp"))
        .output()
        .context("Failed to run git clone")?;

    if !clone_output.status.success() {
        let stderr = String::from_utf8_lossy(&clone_output.stderr);
        anyhow::bail!("Failed to clone repository: {}", stderr);
    }

    // Build the project
    println!("🔨 Building rust-docs-mcp in release mode (this may take a few minutes)...");

    let build_output = Command::new("cargo")
        .args(["build", "--release", "-p", "rust-docs-mcp"])
        .current_dir(temp_path.join("rust-docs-mcp"))
        .output()
        .context("Failed to run cargo build")?;

    if !build_output.status.success() {
        let stderr = String::from_utf8_lossy(&build_output.stderr);
        anyhow::bail!("Failed to build rust-docs-mcp: {}", stderr);
    }

    // Install using the built binary's install command
    println!("📋 Installing rust-docs-mcp to {}...", target_dir.display());

    let built_binary = temp_path.join("rust-docs-mcp/target/release/rust-docs-mcp");
    let install_output = Command::new(&built_binary)
        .args([
            "install",
            "--target-dir",
            &target_dir.to_string_lossy(),
            "--force",
        ])
        .output()
        .context("Failed to run install command")?;

    if !install_output.status.success() {
        let stderr = String::from_utf8_lossy(&install_output.stderr);
        anyhow::bail!("Failed to install rust-docs-mcp: {}", stderr);
    }

    // Handle macOS code signing
    handle_macos_signing(&target_dir)?;

    println!("✅ rust-docs-mcp updated successfully!");

    // Check if target directory is in PATH
    check_path_and_advise(&target_dir)?;

    println!("\n📖 Usage:");
    println!("  rust-docs-mcp                # Start MCP server");
    println!("  rust-docs-mcp install        # Install/update to PATH");
    println!("  rust-docs-mcp update         # Update to latest version");
    println!("  rust-docs-mcp --help         # Show help");

    Ok(())
}

/// Check if a command exists in PATH
fn check_command_exists(command: &str) -> Result<()> {
    let output = Command::new("which")
        .arg(command)
        .output()
        .context("Failed to check command existence")?;

    if !output.status.success() {
        anyhow::bail!("{} is required but not installed", command);
    }

    Ok(())
}

/// Check if nightly toolchain is available and install if needed
fn check_nightly_toolchain() -> Result<()> {
    let output = Command::new("rustup")
        .args(["toolchain", "list"])
        .output()
        .context("Failed to check rustup toolchains")?;

    if !output.status.success() {
        anyhow::bail!("Failed to check available toolchains");
    }

    let toolchains = String::from_utf8_lossy(&output.stdout);
    if !toolchains.contains("nightly") {
        println!("🔧 Installing Rust nightly toolchain...");

        let install_output = Command::new("rustup")
            .args(["toolchain", "install", "nightly"])
            .output()
            .context("Failed to install nightly toolchain")?;

        if !install_output.status.success() {
            let stderr = String::from_utf8_lossy(&install_output.stderr);
            anyhow::bail!("Failed to install Rust nightly toolchain: {}", stderr);
        }

        println!("✅ Rust nightly toolchain installed");
    }

    Ok(())
}

/// Handle macOS-specific binary signing
#[cfg(target_os = "macos")]
fn handle_macos_signing(target_dir: &PathBuf) -> Result<()> {
    let binary_path = target_dir.join("rust-docs-mcp");

    println!("🔐 Signing binary for macOS...");

    // Remove quarantine attributes
    let _ = Command::new("xattr")
        .args(["-cr", &binary_path.to_string_lossy()])
        .output();

    // Ad-hoc sign the binary
    let sign_output = Command::new("codesign")
        .args([
            "--force",
            "--deep",
            "-s",
            "-",
            &binary_path.to_string_lossy(),
        ])
        .output()
        .context("Failed to run codesign")?;

    if sign_output.status.success() {
        println!("✅ Binary signed successfully");
    } else {
        println!("⚠️  Could not sign binary - you may need to run:");
        println!("   codesign --force --deep -s - {}", binary_path.display());
    }

    Ok(())
}

/// No-op for non-macOS systems
#[cfg(not(target_os = "macos"))]
fn handle_macos_signing(_target_dir: &PathBuf) -> Result<()> {
    Ok(())
}

/// Check if target directory is in PATH and provide advice
fn check_path_and_advise(target_dir: &PathBuf) -> Result<()> {
    use std::env;

    if let Ok(path_var) = env::var("PATH") {
        let paths: Vec<&str> = path_var.split(':').collect();
        let target_dir_str = target_dir.to_string_lossy();

        if !paths.iter().any(|&p| p == target_dir_str) {
            println!("\n⚠️  {} is not in your PATH.", target_dir.display());
            println!("Add this line to your shell configuration file (.bashrc, .zshrc, etc.):");
            println!("export PATH=\"{}:$PATH\"", target_dir.display());
            println!("\nThen reload your shell or run:");
            println!("source ~/.bashrc  # or ~/.zshrc");
        } else {
            println!("\n✅ You can now run 'rust-docs-mcp' from anywhere!");
        }
    }

    Ok(())
}
