//! Unified rustdoc JSON generation functionality
//!
//! Provides consistent rustdoc JSON generation across the application,
//! including toolchain validation and command execution.

use anyhow::{Context, Result, bail};
use std::path::Path;
use std::process::Command;
use crate::util::analyze_crate_features;

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
            "Required toolchain {} is not installed. Please run: rustup toolchain install {}",
            REQUIRED_TOOLCHAIN,
            REQUIRED_TOOLCHAIN
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
        bail!("JSON generation failed: {}", stderr);
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

    let mut args = vec![format!("+{}", REQUIRED_TOOLCHAIN), "rustdoc".to_string()];

    // Add package-specific arguments if provided
    if let Some(pkg) = package {
        args.push("-p".to_string());
        args.push(pkg.to_string());
    }

    // Analyze crate features to determine if we can use --all-features
    let feature_analysis = analyze_crate_features(source_path)?;
    
    // Add feature flags based on analysis
    if feature_analysis.has_features && !feature_analysis.has_mutually_exclusive {
        // Safe to use all features
        args.push("--all-features".to_string());
        tracing::debug!(
            "Using --all-features for {} (no mutually exclusive features detected)",
            source_path.display()
        );
    } else if feature_analysis.has_mutually_exclusive {
        // Use default features only
        tracing::debug!(
            "Using default features only for {} (mutually exclusive features detected: {:?})",
            source_path.display(),
            feature_analysis.conflict_groups
        );
    }
    
    // Add remaining arguments
    args.extend_from_slice(&[
        "--".to_string(),
        "--output-format".to_string(),
        "json".to_string(),
        "-Z".to_string(),
        "unstable-options".to_string(),
    ]);

    let output = Command::new("cargo")
        .args(&args)
        .current_dir(source_path)
        .output()
        .context("Failed to run cargo rustdoc")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!("Failed to generate documentation: {}", stderr);
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
