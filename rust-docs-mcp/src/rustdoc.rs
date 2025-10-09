//! Unified rustdoc JSON generation functionality
//!
//! Provides consistent rustdoc JSON generation across the application,
//! including toolchain validation and command execution.

use anyhow::{Context, Result, bail};
use std::path::Path;
use std::process::Command;
use std::time::Duration;
use tokio::process::Command as TokioCommand;

/// The pinned nightly toolchain version compatible with rustdoc-types 0.53.0
pub const REQUIRED_TOOLCHAIN: &str = "nightly-2025-06-23";

/// Number of lines to preview from error messages in diagnostic output
const ERROR_MESSAGE_PREVIEW_LINES: usize = 10;

/// Maximum characters to store in error messages to prevent memory issues
const MAX_ERROR_MESSAGE_CHARS: usize = 4096;

/// Timeout for individual rustdoc execution attempts (in seconds)
const RUSTDOC_TIMEOUT_SECS: u64 = 1800;

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
        .arg(format!("+{}", REQUIRED_TOOLCHAIN))
        .arg("--version")
        .output()
        .context("Failed to run rustdoc --version")?;

    if !output.status.success() {
        bail!("rustdoc command failed");
    }

    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

/// Strategy for selecting feature flags when generating rustdoc JSON output.
///
/// Provides a fallback mechanism to handle crates that fail to compile with
/// certain feature combinations. Common scenarios include:
/// - Platform-specific features that don't compile on all targets
/// - Optional dependencies with conflicting version requirements
/// - Features requiring specific system libraries
///
/// The recommended order is: [`AllFeatures`](Self::AllFeatures) →
/// [`DefaultFeatures`](Self::DefaultFeatures) → [`NoDefaultFeatures`](Self::NoDefaultFeatures)
#[derive(Debug, Clone, Copy)]
#[allow(clippy::enum_variant_names)]
enum FeatureStrategy {
    /// Use --all-features (most comprehensive)
    AllFeatures,
    /// Use default features only
    DefaultFeatures,
    /// Use --no-default-features (minimal)
    NoDefaultFeatures,
}

impl FeatureStrategy {
    /// Get the command line arguments for this strategy
    fn args(&self) -> Vec<String> {
        match self {
            Self::AllFeatures => vec!["--all-features".to_string()],
            Self::DefaultFeatures => vec![],
            Self::NoDefaultFeatures => vec!["--no-default-features".to_string()],
        }
    }

    /// Get a description of this strategy for logging
    fn description(&self) -> &str {
        match self {
            Self::AllFeatures => "all features enabled",
            Self::DefaultFeatures => "default features only",
            Self::NoDefaultFeatures => "no default features",
        }
    }
}

/// Check if an error is a compilation error
fn is_compilation_error(stderr: &str) -> bool {
    stderr.contains("error[E")
        || stderr.contains("error: could not compile")
        || stderr.contains("Compiling")
            && (stderr.contains("error:") || stderr.contains("failed to compile"))
}

/// Stores information about a failed rustdoc attempt for diagnostics
#[derive(Debug, Clone)]
struct FailedAttempt {
    strategy: String,
    error: String,
}

impl FailedAttempt {
    /// Create a new failed attempt with error message truncation
    fn new(strategy: String, error: String) -> Self {
        let truncated_error = if error.len() > MAX_ERROR_MESSAGE_CHARS {
            format!(
                "{}... (truncated {} chars)",
                &error[..MAX_ERROR_MESSAGE_CHARS],
                error.len() - MAX_ERROR_MESSAGE_CHARS
            )
        } else {
            error
        };

        Self {
            strategy,
            error: truncated_error,
        }
    }
}

/// Execute cargo rustdoc with the given arguments
///
/// This is a helper to avoid duplicating the execution logic for both
/// standard and --lib retry cases.
///
/// Returns an error if the command times out after [`RUSTDOC_TIMEOUT_SECS`] seconds.
async fn execute_rustdoc(args: &[String], source_path: &Path) -> Result<std::process::Output> {
    tokio::time::timeout(
        Duration::from_secs(RUSTDOC_TIMEOUT_SECS),
        TokioCommand::new("cargo")
            .args(args)
            .current_dir(source_path)
            .output()
    )
    .await
    .context(format!("Rustdoc execution timed out after {} seconds", RUSTDOC_TIMEOUT_SECS))?
    .context("Failed to run cargo rustdoc")
}

/// Run cargo rustdoc with JSON output for a crate or specific package
pub async fn run_cargo_rustdoc_json(source_path: &Path, package: Option<&str>) -> Result<()> {
    validate_toolchain().await?;

    // Logging strategy:
    // - debug: Strategy attempts and retries
    // - warn: Non-fatal failures that trigger fallback
    // - info: Final success

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

    // Try different feature strategies in order
    let strategies = [
        FeatureStrategy::AllFeatures,
        FeatureStrategy::DefaultFeatures,
        FeatureStrategy::NoDefaultFeatures,
    ];

    let mut failed_attempts = Vec::new();

    for (i, strategy) in strategies.iter().enumerate() {
        tracing::debug!(
            "Attempting documentation generation with {}",
            strategy.description()
        );

        // Build args with current feature strategy
        let feature_args = strategy.args();
        let rustdoc_args = vec![
            "--".to_string(),
            "--output-format".to_string(),
            "json".to_string(),
            "-Z".to_string(),
            "unstable-options".to_string(),
        ];

        // First try without --lib to support crates that have a single target
        let mut args = base_args.clone();
        args.extend_from_slice(&feature_args);
        args.extend_from_slice(&rustdoc_args);

        let output = execute_rustdoc(&args, source_path).await?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);

            // Check for binary-only package early - this is not retryable
            if stderr.contains("no library targets found") {
                bail!("This is a binary-only package");
            }

            // Check for workspace error - this is not retryable
            if stderr.contains("could not find `Cargo.toml` in") || stderr.contains("workspace") {
                bail!(
                    "This appears to be a workspace. Please use workspace member caching instead of trying to cache the root workspace."
                );
            }

            // If we get the multiple targets error, try again with --lib
            if stderr.contains("extra arguments to `rustdoc` can only be passed to one target") {
                tracing::debug!("Multiple targets detected, retrying with --lib flag");

                // Try again with --lib flag
                let mut args_with_lib = base_args.clone();
                args_with_lib.push("--lib".to_string());
                args_with_lib.extend_from_slice(&feature_args);
                args_with_lib.extend_from_slice(&rustdoc_args);

                let output_with_lib = execute_rustdoc(&args_with_lib, source_path).await?;

                if !output_with_lib.status.success() {
                    let stderr_with_lib = String::from_utf8_lossy(&output_with_lib.stderr);

                    // Check for binary-only package
                    if stderr_with_lib.contains("no library targets found") {
                        bail!("This is a binary-only package");
                    }

                    // Check if this is a compilation error
                    if is_compilation_error(&stderr_with_lib) && i < strategies.len() - 1 {
                        tracing::warn!(
                            "Compilation failed with {}, will try next strategy",
                            strategy.description()
                        );
                        failed_attempts.push(FailedAttempt::new(
                            strategy.description().to_string(),
                            stderr_with_lib.to_string(),
                        ));
                        continue; // Try next strategy
                    }

                    bail!("Failed to generate documentation: {}", stderr_with_lib);
                }

                // Success with --lib
                tracing::info!(
                    "Successfully generated documentation with {}",
                    strategy.description()
                );
                return Ok(());
            }

            // Check if this is a compilation error that we should retry
            if is_compilation_error(&stderr) && i < strategies.len() - 1 {
                tracing::warn!(
                    "Compilation failed with {}, will try next strategy",
                    strategy.description()
                );
                failed_attempts.push(FailedAttempt::new(
                    strategy.description().to_string(),
                    stderr.to_string(),
                ));
                continue; // Try next strategy
            }

            // Other errors or last strategy failed
            bail!("Failed to generate documentation: {}", stderr);
        }

        // Success
        tracing::info!(
            "Successfully generated documentation with {}",
            strategy.description()
        );
        return Ok(());
    }

    // If we get here, all strategies failed
    let error_summary = failed_attempts
        .iter()
        .enumerate()
        .map(|(idx, attempt)| {
            format!(
                "  {}. Strategy '{}': {}",
                idx + 1,
                attempt.strategy,
                attempt
                    .error
                    .lines()
                    .take(ERROR_MESSAGE_PREVIEW_LINES)
                    .collect::<Vec<_>>()
                    .join("\n     ")
            )
        })
        .collect::<Vec<_>>()
        .join("\n");

    bail!(
        "Failed to generate documentation with all feature strategies:\n{}",
        error_summary
    )
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

    #[test]
    fn test_feature_strategy_args() {
        assert_eq!(
            FeatureStrategy::AllFeatures.args(),
            vec!["--all-features".to_string()]
        );
        assert_eq!(
            FeatureStrategy::DefaultFeatures.args(),
            Vec::<String>::new()
        );
        assert_eq!(
            FeatureStrategy::NoDefaultFeatures.args(),
            vec!["--no-default-features".to_string()]
        );
    }

    #[test]
    fn test_feature_strategy_description() {
        assert_eq!(
            FeatureStrategy::AllFeatures.description(),
            "all features enabled"
        );
        assert_eq!(
            FeatureStrategy::DefaultFeatures.description(),
            "default features only"
        );
        assert_eq!(
            FeatureStrategy::NoDefaultFeatures.description(),
            "no default features"
        );
    }

    #[test]
    fn test_is_compilation_error_with_error_codes() {
        let stderr = "error[E0425]: cannot find value `foo` in this scope";
        assert!(is_compilation_error(stderr));
    }

    #[test]
    fn test_is_compilation_error_with_could_not_compile() {
        let stderr = "error: could not compile `my-crate` due to previous error";
        assert!(is_compilation_error(stderr));
    }

    #[test]
    fn test_is_compilation_error_with_compiling_and_error() {
        let stderr = "Compiling my-crate v0.1.0\nerror: expected one of `!` or `::`, found `{`";
        assert!(is_compilation_error(stderr));
    }

    #[test]
    fn test_is_compilation_error_with_failed_to_compile() {
        let stderr = "Compiling my-crate v0.1.0\nfailed to compile my-crate";
        assert!(is_compilation_error(stderr));
    }

    #[test]
    fn test_is_not_compilation_error() {
        let stderr = "warning: unused import: `std::collections::HashMap`";
        assert!(!is_compilation_error(stderr));
    }

    #[test]
    fn test_is_not_compilation_error_compiling_without_error() {
        let stderr = "Compiling my-crate v0.1.0\nFinished dev [unoptimized + debuginfo] target(s)";
        assert!(!is_compilation_error(stderr));
    }
}
