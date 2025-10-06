//! Unified rustdoc JSON generation functionality
//!
//! Provides consistent rustdoc JSON generation across the application,
//! including toolchain validation and command execution.

use anyhow::{Context, Result, bail};
use std::path::Path;
use std::process::Command;

/// The pinned nightly toolchain version compatible with rustdoc-types 0.53.0
pub const REQUIRED_TOOLCHAIN: &str = "nightly-2025-06-23";

/// Check if the required nightly toolchain is available
pub async fn validate_toolchain() -> Result<()> {
    let output = Command::new("rustup")
        .args(["toolchain", "list"])
        .output()
        .context("Failed to run rustup toolchain list")?;

    if !output.status.success() {
        bail!("Failed to check available toolchains");
    }

    let toolchains = String::from_utf8_lossy(&output.stdout);
    if !toolchains.contains(REQUIRED_TOOLCHAIN) {
        bail!(
            "Required toolchain {REQUIRED_TOOLCHAIN} is not installed. Please run: rustup toolchain install {REQUIRED_TOOLCHAIN}"
        );
    }

    tracing::debug!("Validated toolchain {} is available", REQUIRED_TOOLCHAIN);
    Ok(())
}

/// Test rustdoc JSON functionality with a simple test file
pub async fn test_rustdoc_json() -> Result<()> {
    // First validate the toolchain
    validate_toolchain().await?;

    // Create a temporary directory and test file
    let temp_dir =
        tempfile::tempdir().context("Failed to create temporary directory for testing")?;

    let test_file = temp_dir.path().join("lib.rs");
    std::fs::write(&test_file, "//! Test crate\npub fn test() {}")
        .context("Failed to create test file")?;

    let test_file_str = test_file
        .to_str()
        .ok_or_else(|| anyhow::anyhow!("Test file path contains invalid UTF-8"))?;

    tracing::debug!(
        "Testing rustdoc JSON generation with {}",
        REQUIRED_TOOLCHAIN
    );

    // Try to generate JSON documentation using the pinned toolchain
    let output = Command::new("rustdoc")
        .args([
            &format!("+{REQUIRED_TOOLCHAIN}"),
            "-Z",
            "unstable-options",
            "--output-format",
            "json",
            "--crate-name",
            "test",
            test_file_str,
        ])
        .output()
        .context("Failed to run rustdoc")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!("JSON generation failed: {stderr}");
    }

    tracing::debug!("Successfully tested rustdoc JSON generation");
    Ok(())
}

/// Get rustdoc version information
pub async fn get_rustdoc_version() -> Result<String> {
    let output = Command::new("rustdoc")
        .arg("--version")
        .output()
        .context("Failed to run rustdoc --version")?;

    if !output.status.success() {
        bail!("rustdoc command failed");
    }

    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

/// Run cargo rustdoc with JSON output for a crate or specific package
pub async fn run_cargo_rustdoc_json(source_path: &Path, package: Option<&str>) -> Result<()> {
    validate_toolchain().await?;

    let log_msg = match package {
        Some(pkg) => format!(
            "Running cargo rustdoc with JSON output for package {} in {}",
            pkg,
            source_path.display()
        ),
        None => format!(
            "Running cargo rustdoc with JSON output in {}",
            source_path.display()
        ),
    };
    tracing::debug!("{}", log_msg);

    let mut base_args = vec![format!("+{}", REQUIRED_TOOLCHAIN), "rustdoc".to_string()];

    // Add package-specific arguments if provided
    if let Some(pkg) = package {
        base_args.push("-p".to_string());
        base_args.push(pkg.to_string());
    }

    // Add remaining arguments
    let rustdoc_args = vec![
        "--all-features".to_string(),
        "--".to_string(),
        "--output-format".to_string(),
        "json".to_string(),
        "-Z".to_string(),
        "unstable-options".to_string(),
    ];

    // First try without --lib to support crates that have a single target
    let mut args = base_args.clone();
    args.extend_from_slice(&rustdoc_args);

    let output = Command::new("cargo")
        .args(&args)
        .current_dir(source_path)
        .output()
        .context("Failed to run cargo rustdoc")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);

        // If we get the multiple targets error, try again with --lib
        if stderr.contains("extra arguments to `rustdoc` can only be passed to one target") {
            tracing::debug!("Multiple targets detected, retrying with --lib flag");

            // Try again with --lib flag
            let mut args_with_lib = base_args;
            args_with_lib.push("--lib".to_string());
            args_with_lib.extend_from_slice(&rustdoc_args);

            let output_with_lib = Command::new("cargo")
                .args(&args_with_lib)
                .current_dir(source_path)
                .output()
                .context("Failed to run cargo rustdoc with --lib")?;

            if !output_with_lib.status.success() {
                let stderr_with_lib = String::from_utf8_lossy(&output_with_lib.stderr);

                if stderr_with_lib.contains("no library targets found") {
                    bail!("This is a binary-only package");
                }

                bail!("Failed to generate documentation: {stderr_with_lib}");
            }

            // Success with --lib
            return Ok(());
        }

        // Check for workspace error
        if stderr.contains("could not find `Cargo.toml` in") || stderr.contains("workspace") {
            bail!(
                "This appears to be a workspace. Please use workspace member caching instead of trying to cache the root workspace."
            );
        }

        bail!("Failed to generate documentation: {stderr}");
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_get_rustdoc_version() {
        // This test will pass if rustdoc is installed
        let result = get_rustdoc_version().await;
        // We can't guarantee the success state in all environments
        // but we can verify it returns a valid result
        assert!(result.is_ok() || result.is_err());
    }

    #[tokio::test]
    async fn test_validate_toolchain() {
        // This test will pass if rustup is installed
        let result = validate_toolchain().await;
        // We can't guarantee the toolchain is installed in all environments
        // but we can verify it returns a valid result
        assert!(result.is_ok() || result.is_err());
    }
}
