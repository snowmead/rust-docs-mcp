//! Unified rustdoc JSON generation functionality
//!
//! Provides consistent rustdoc JSON generation across the application,
//! including toolchain validation and command execution.

use anyhow::{Context, Result, bail};
use serde::Deserialize;
use std::env;
use std::path::Path;
use std::process::Command;
use std::sync::OnceLock;
use std::time::Duration;
use tokio::process::Command as TokioCommand;

/// Preferred nightly toolchain. Keep this new enough to compile modern crates
/// and keep `rustdoc-types` in sync with its rustdoc JSON format.
pub const PREFERRED_TOOLCHAIN: &str = "nightly-2026-05-22";

/// Fallback nightly alias to try when the preferred dated toolchain is unavailable.
pub const FALLBACK_TOOLCHAIN: &str = "nightly";

/// Environment variable allowing users to explicitly select a rustdoc toolchain.
pub const TOOLCHAIN_ENV_VAR: &str = "RUST_DOCS_MCP_TOOLCHAIN";

/// Number of lines to preview from error messages in diagnostic output
const ERROR_MESSAGE_PREVIEW_LINES: usize = 10;

/// Maximum characters to store in error messages to prevent memory issues
const MAX_ERROR_MESSAGE_CHARS: usize = 4096;

/// Timeout for individual rustdoc execution attempts (in seconds)
const RUSTDOC_TIMEOUT_SECS: u64 = 1800;

static SELECTED_TOOLCHAIN: OnceLock<std::result::Result<String, String>> = OnceLock::new();

#[derive(Debug)]
enum ToolchainProbe {
    Compatible,
    Missing,
    Incompatible(String),
}

#[derive(Debug, Deserialize)]
struct ProbeFormatVersion {
    format_version: u32,
}

/// Resolve the rustdoc toolchain to use for JSON generation.
pub fn resolve_toolchain() -> Result<String> {
    match SELECTED_TOOLCHAIN.get_or_init(|| select_toolchain().map_err(|err| err.to_string())) {
        Ok(toolchain) => Ok(toolchain.clone()),
        Err(message) => bail!("{message}"),
    }
}

fn select_toolchain() -> Result<String> {
    if let Some(toolchain) = env::var(TOOLCHAIN_ENV_VAR)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
    {
        return match probe_toolchain(&toolchain)? {
            ToolchainProbe::Compatible => Ok(toolchain),
            ToolchainProbe::Missing => bail!(
                "Configured toolchain {toolchain} from {TOOLCHAIN_ENV_VAR} is not installed. \
Please run: rustup toolchain install {toolchain}"
            ),
            ToolchainProbe::Incompatible(reason) => bail!(
                "Configured toolchain {toolchain} from {TOOLCHAIN_ENV_VAR} is not compatible \
with rustdoc JSON format version {}: {reason}",
                rustdoc_types::FORMAT_VERSION
            ),
        };
    }

    let preferred = probe_toolchain(PREFERRED_TOOLCHAIN)?;
    if matches!(preferred, ToolchainProbe::Compatible) {
        return Ok(PREFERRED_TOOLCHAIN.to_string());
    }

    let fallback = probe_toolchain(FALLBACK_TOOLCHAIN)?;
    if matches!(fallback, ToolchainProbe::Compatible) {
        tracing::warn!(
            "Preferred rustdoc toolchain {} is unavailable or incompatible; falling back to {}",
            PREFERRED_TOOLCHAIN,
            FALLBACK_TOOLCHAIN
        );
        return Ok(FALLBACK_TOOLCHAIN.to_string());
    }

    let preferred_reason = match preferred {
        ToolchainProbe::Missing => format!(
            "{PREFERRED_TOOLCHAIN} is not installed. Install it with: rustup toolchain install \
{PREFERRED_TOOLCHAIN}"
        ),
        ToolchainProbe::Incompatible(reason) => format!(
            "{PREFERRED_TOOLCHAIN} is installed but incompatible with rustdoc JSON format \
version {}: {reason}",
            rustdoc_types::FORMAT_VERSION
        ),
        ToolchainProbe::Compatible => unreachable!(),
    };

    let fallback_reason = match fallback {
        ToolchainProbe::Missing => {
            format!(
                "{FALLBACK_TOOLCHAIN} is not installed. Install it with: rustup toolchain install {FALLBACK_TOOLCHAIN}"
            )
        }
        ToolchainProbe::Incompatible(reason) => format!(
            "{FALLBACK_TOOLCHAIN} is installed but incompatible with rustdoc JSON format \
version {}: {reason}",
            rustdoc_types::FORMAT_VERSION
        ),
        ToolchainProbe::Compatible => unreachable!(),
    };

    bail!(
        "No compatible nightly rustdoc toolchain was found.\n\
Preferred: {preferred_reason}\n\
Fallback: {fallback_reason}\n\
You can also set {TOOLCHAIN_ENV_VAR} to a specific compatible nightly."
    )
}

fn probe_toolchain(toolchain: &str) -> Result<ToolchainProbe> {
    let version_output = Command::new("rustdoc")
        .arg(format!("+{toolchain}"))
        .arg("--version")
        .output()
        .with_context(|| format!("Failed to run rustdoc --version for toolchain {toolchain}"))?;

    if !version_output.status.success() {
        let stderr = String::from_utf8_lossy(&version_output.stderr)
            .trim()
            .to_string();
        if is_missing_toolchain_error(&stderr, toolchain) {
            return Ok(ToolchainProbe::Missing);
        }

        return Ok(ToolchainProbe::Incompatible(format!(
            "rustdoc --version failed: {stderr}"
        )));
    }

    match validate_rustdoc_json_format(toolchain) {
        Ok(()) => Ok(ToolchainProbe::Compatible),
        Err(err) => Ok(ToolchainProbe::Incompatible(err.to_string())),
    }
}

fn is_missing_toolchain_error(stderr: &str, toolchain: &str) -> bool {
    if !(stderr.contains("is not installed") || stderr.contains("toolchain not installed")) {
        return false;
    }

    // Extract toolchain name from rustup's error format: 'toolchain-name'
    // Then check it matches our query, allowing for an architecture suffix
    // (e.g., "nightly" -> "nightly-aarch64-apple-darwin") but not a date suffix
    // (e.g., "nightly" should NOT match "nightly-2026-05-22-aarch64-apple-darwin")
    stderr.split('\'').nth(1).is_some_and(|name| {
        name == toolchain
            || name
                .strip_prefix(toolchain)
                .and_then(|rest| rest.strip_prefix('-'))
                .is_some_and(|rest| !rest.starts_with(|c: char| c.is_ascii_digit()))
    })
}

fn validate_rustdoc_json_format(toolchain: &str) -> Result<()> {
    let json = generate_probe_json(toolchain)?;
    let format: ProbeFormatVersion =
        serde_json::from_str(&json).context("Failed to read rustdoc JSON format version")?;

    if format.format_version != rustdoc_types::FORMAT_VERSION {
        bail!(
            "expected rustdoc JSON format version {}, got {}",
            rustdoc_types::FORMAT_VERSION,
            format.format_version
        );
    }

    let _: rustdoc_types::Crate =
        serde_json::from_str(&json).context("Generated rustdoc JSON could not be parsed")?;

    Ok(())
}

fn generate_probe_json(toolchain: &str) -> Result<String> {
    let temp_dir =
        tempfile::tempdir().context("Failed to create temporary directory for toolchain probe")?;
    let test_file = temp_dir.path().join("lib.rs");
    let output_dir = temp_dir.path().join("out");
    std::fs::write(&test_file, "//! Toolchain probe\npub fn probe() {}")
        .context("Failed to create probe source file")?;
    std::fs::create_dir(&output_dir).context("Failed to create probe output directory")?;

    let output = Command::new("rustdoc")
        .arg(format!("+{toolchain}"))
        .args([
            "-Z",
            "unstable-options",
            "--output-format",
            "json",
            "--crate-name",
            "toolchain_probe",
            "-o",
        ])
        .arg(&output_dir)
        .arg(&test_file)
        .output()
        .with_context(|| format!("Failed to run rustdoc probe for toolchain {toolchain}"))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        bail!("rustdoc JSON probe failed: {stderr}");
    }

    std::fs::read_to_string(output_dir.join("toolchain_probe.json"))
        .context("Failed to read rustdoc JSON probe output")
}

/// Check if the required nightly toolchain is available
pub async fn validate_toolchain() -> Result<()> {
    let toolchain = resolve_toolchain()?;
    tracing::debug!("Validated toolchain {} is available", toolchain);
    Ok(())
}

/// Test rustdoc JSON functionality with a simple test file
pub async fn test_rustdoc_json() -> Result<()> {
    let toolchain = resolve_toolchain()?;
    tracing::debug!("Testing rustdoc JSON generation with {}", toolchain);
    validate_rustdoc_json_format(&toolchain)?;

    tracing::debug!("Successfully tested rustdoc JSON generation");
    Ok(())
}

/// Get rustdoc version information
pub async fn get_rustdoc_version() -> Result<String> {
    let toolchain = resolve_toolchain()?;
    get_rustdoc_version_for_toolchain(&toolchain)
}

/// Get rustdoc version information for a specific toolchain.
pub fn get_rustdoc_version_for_toolchain(toolchain: &str) -> Result<String> {
    let output = Command::new("rustdoc")
        .arg(format!("+{toolchain}"))
        .arg("--version")
        .output()
        .with_context(|| format!("Failed to run rustdoc --version for toolchain {toolchain}"))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        bail!("rustdoc command failed for toolchain {toolchain}: {stderr}");
    }

    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

fn check_msrv_error(stderr: &str, toolchain: &str) -> Result<()> {
    if stderr.contains("requires rustc")
        || stderr.contains("rustc is not supported")
        || stderr.contains("is not supported by the following package")
        || stderr.contains("error[E0658]")
    {
        bail!(
            "Documentation build with {toolchain} failed because the crate requires compiler features unavailable in this toolchain. Set {TOOLCHAIN_ENV_VAR} to a newer installed nightly compatible with rustdoc JSON format {}. Run rust-docs-mcp doctor to inspect compatibility. No compiler was changed automatically.\n{stderr}",
            rustdoc_types::FORMAT_VERSION
        );
    }
    Ok(())
}

/// Check if an error is a compilation error
fn is_compilation_error(stderr: &str) -> bool {
    stderr.contains("error[E")
        || stderr.contains("error: could not compile")
        || stderr.contains("error: could not document")
        || stderr.contains("requires rustc")
        || stderr.contains("is not supported by the following package")
        || stderr.contains("error: aborting due to")
        || (stderr.contains("error:") && stderr.contains("failed to compile"))
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
            // Safely truncate at a UTF-8 character boundary
            let truncate_at = error
                .char_indices()
                .take_while(|(idx, _)| *idx < MAX_ERROR_MESSAGE_CHARS)
                .last()
                .map(|(idx, ch)| idx + ch.len_utf8())
                .unwrap_or(0);
            format!(
                "{}... (truncated {} chars)",
                &error[..truncate_at],
                error.len() - truncate_at
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
async fn execute_rustdoc(
    args: &[String],
    source_path: &Path,
    target_dir: Option<&Path>,
) -> Result<std::process::Output> {
    let mut command = TokioCommand::new("cargo");
    command
        .args(args)
        .current_dir(source_path)
        .kill_on_drop(true);

    // Set custom target directory if provided to avoid conflicts when building
    // multiple workspace members concurrently
    if let Some(dir) = target_dir {
        command.env("CARGO_TARGET_DIR", dir);
    }

    tokio::time::timeout(Duration::from_secs(RUSTDOC_TIMEOUT_SECS), command.output())
        .await
        .context(format!(
            "Rustdoc execution timed out after {RUSTDOC_TIMEOUT_SECS} seconds"
        ))?
        .context("Failed to run cargo rustdoc")
}

/// Run cargo rustdoc with JSON output for a crate or specific package
///
/// # Parameters
/// - `source_path`: The root directory containing Cargo.toml
/// - `package`: Optional package name for workspace members
/// - `target_dir`: Optional custom target directory to avoid conflicts when building
///   multiple workspace members concurrently. When building multiple workspace members
///   in parallel, each must use a unique target directory to prevent cargo from
///   conflicting with itself. See [`DocGenerator::generate_workspace_member_docs`](crate::cache::docgen::DocGenerator::generate_workspace_member_docs)
///   for the implementation pattern.
pub async fn run_cargo_rustdoc_json(
    source_path: &Path,
    package: Option<&str>,
    target_dir: Option<&Path>,
    features: Option<Vec<String>>,
) -> Result<()> {
    let options = crate::cache::features::FeatureOptions::new(features, None, None)?;
    run_cargo_rustdoc_json_with_options(source_path, package, target_dir, &options)
        .await
        .map(|_| ())
}

pub async fn run_cargo_rustdoc_json_with_options(
    source_path: &Path,
    package: Option<&str>,
    target_dir: Option<&Path>,
    options: &crate::cache::features::FeatureOptions,
) -> Result<crate::cache::features::FeatureOptions> {
    let toolchain = resolve_toolchain()?;

    // Logging strategy:
    // - debug: Strategy attempts and retries
    // - warn: Non-fatal failures that trigger fallback
    // - info: Final success

    let log_msg = match (package, target_dir) {
        (Some(pkg), Some(target)) => format!(
            "Running cargo rustdoc with JSON output for package {} in {} (target: {})",
            pkg,
            source_path.display(),
            target.display()
        ),
        (Some(pkg), None) => format!(
            "Running cargo rustdoc with JSON output for package {} in {}",
            pkg,
            source_path.display()
        ),
        (None, Some(target)) => format!(
            "Running cargo rustdoc with JSON output in {} (target: {})",
            source_path.display(),
            target.display()
        ),
        (None, None) => format!(
            "Running cargo rustdoc with JSON output in {}",
            source_path.display()
        ),
    };
    tracing::debug!("{}", log_msg);

    let mut base_args = vec![format!("+{}", toolchain), "rustdoc".to_string()];

    // Add package-specific arguments if provided
    if let Some(pkg) = package {
        base_args.push("-p".to_string());
        base_args.push(pkg.to_string());
    }

    let strategies = options.strategies();

    let mut failed_attempts = Vec::new();

    for (i, strategy) in strategies.iter().enumerate() {
        tracing::debug!("Attempting documentation generation with {strategy}");

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

        let output = execute_rustdoc(&args, source_path, target_dir).await?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);

            // Check for binary-only package early - this is not retryable
            if stderr.contains("no library targets found") {
                bail!("This is a binary-only package");
            }

            // Check for workspace error - this is not retryable
            // Only bail if we get a specific workspace root error, not just any error mentioning workspaces
            if stderr.contains("could not find `Cargo.toml` in")
                || stderr.contains("current package believes it's in a workspace")
                || (stderr.contains("workspace") && stderr.contains("manifest not found"))
            {
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

                let output_with_lib =
                    execute_rustdoc(&args_with_lib, source_path, target_dir).await?;

                if !output_with_lib.status.success() {
                    let stderr_with_lib = String::from_utf8_lossy(&output_with_lib.stderr);

                    // Check for binary-only package
                    if stderr_with_lib.contains("no library targets found") {
                        bail!("This is a binary-only package");
                    }

                    // Check if this is a compilation error
                    if is_compilation_error(&stderr_with_lib) && i < strategies.len() - 1 {
                        tracing::warn!(
                            "Compilation failed with {strategy}, will try next strategy"
                        );
                        failed_attempts.push(FailedAttempt::new(
                            strategy.to_string(),
                            stderr_with_lib.to_string(),
                        ));
                        continue; // Try next strategy
                    }

                    check_msrv_error(&stderr_with_lib, &toolchain)?;
                    bail!("Failed to generate documentation with {strategy}: {stderr_with_lib}");
                }

                // Success with --lib
                tracing::info!("Successfully generated documentation with {strategy}");
                return Ok(strategy.clone());
            }

            // Check if this is a compilation error that we should retry
            if is_compilation_error(&stderr) && i < strategies.len() - 1 {
                tracing::warn!("Compilation failed with {strategy}, will try next strategy");
                failed_attempts.push(FailedAttempt::new(strategy.to_string(), stderr.to_string()));
                continue; // Try next strategy
            }

            // Other errors or last strategy failed
            check_msrv_error(&stderr, &toolchain)?;
            bail!("Failed to generate documentation with {strategy}: {stderr}");
        }

        // Success
        tracing::info!("Successfully generated documentation with {strategy}");
        return Ok(strategy.clone());
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

    bail!("Failed to generate documentation with all feature strategies:\n{error_summary}")
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
    fn test_is_missing_toolchain_error() {
        let stderr = "error: toolchain 'nightly-2099-01-01-aarch64-apple-darwin' is not installed";
        assert!(is_missing_toolchain_error(stderr, "nightly-2099-01-01"));
        assert!(!is_missing_toolchain_error(stderr, "nightly"));
    }

    #[test]
    fn compiler_version_errors_explain_the_override() {
        let message = check_msrv_error("error: example@1 requires rustc 1.99", PREFERRED_TOOLCHAIN)
            .unwrap_err()
            .to_string();
        assert!(message.contains(TOOLCHAIN_ENV_VAR));
        assert!(message.contains(PREFERRED_TOOLCHAIN));
        assert!(message.contains("JSON format"));
        assert!(check_msrv_error("error[E0658]: unstable syntax", PREFERRED_TOOLCHAIN).is_err());
        assert!(check_msrv_error("error: unknown feature", PREFERRED_TOOLCHAIN).is_ok());
        assert!(is_compilation_error("error: could not document `example`"));
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
    fn test_is_compilation_error_with_aborting_due_to() {
        let stderr = "error: aborting due to 3 previous errors";
        assert!(is_compilation_error(stderr));
    }

    #[test]
    fn test_is_compilation_error_with_failed_to_compile() {
        let stderr = "error: failed to compile `my-crate` due to previous error";
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
