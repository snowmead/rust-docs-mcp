use anyhow::Result;
use rust_docs_mcp::rustdoc;
use serde::Serialize;
use std::fs;
use std::process::Command;

#[derive(Serialize)]
pub struct DiagnosticResult {
    pub name: String,
    pub success: bool,
    pub message: String,
    pub critical: bool,
}

impl DiagnosticResult {
    pub fn new(name: String, success: bool, message: String, critical: bool) -> Self {
        Self {
            name,
            success,
            message,
            critical,
        }
    }
}

pub async fn run_diagnostics(
    cache_dir: Option<std::path::PathBuf>,
) -> Result<Vec<DiagnosticResult>> {
    let mut results = Vec::new();

    // Check Rust toolchain
    results.push(check_rust_toolchain().await);

    // Check nightly toolchain
    results.push(check_nightly_toolchain().await);

    // Check rustdoc JSON capability
    results.push(check_rustdoc_json().await);

    // Check Git installation
    results.push(check_git_installation().await);

    // Check network connectivity
    results.push(check_network_connectivity().await);

    // Check cache directory
    results.push(check_cache_directory(cache_dir).await);

    // Check optional dependencies
    results.push(check_optional_dependencies().await);

    Ok(results)
}

async fn check_rust_toolchain() -> DiagnosticResult {
    match Command::new("rustc").arg("--version").output() {
        Ok(output) if output.status.success() => {
            let version = String::from_utf8_lossy(&output.stdout).trim().to_string();
            DiagnosticResult::new("Rust toolchain".to_string(), true, version, true)
        }
        Ok(_) => DiagnosticResult::new(
            "Rust toolchain".to_string(),
            false,
            "rustc command failed".to_string(),
            true,
        ),
        Err(_) => DiagnosticResult::new(
            "Rust toolchain".to_string(),
            false,
            "rustc not found in PATH".to_string(),
            true,
        ),
    }
}

async fn check_nightly_toolchain() -> DiagnosticResult {
    match Command::new("rustup").args(["toolchain", "list"]).output() {
        Ok(output) if output.status.success() => {
            let toolchains = String::from_utf8_lossy(&output.stdout);
            if toolchains.contains("nightly") {
                // Try to get nightly version
                match Command::new("rustc")
                    .args(["+nightly", "--version"])
                    .output()
                {
                    Ok(nightly_output) if nightly_output.status.success() => {
                        let version = String::from_utf8_lossy(&nightly_output.stdout)
                            .trim()
                            .to_string();
                        DiagnosticResult::new("Nightly toolchain".to_string(), true, version, true)
                    }
                    _ => DiagnosticResult::new(
                        "Nightly toolchain".to_string(),
                        false,
                        "nightly toolchain installed but not functional".to_string(),
                        true,
                    ),
                }
            } else {
                DiagnosticResult::new(
                    "Nightly toolchain".to_string(),
                    false,
                    "nightly toolchain not installed".to_string(),
                    true,
                )
            }
        }
        Ok(_) => DiagnosticResult::new(
            "Nightly toolchain".to_string(),
            false,
            "rustup command failed".to_string(),
            true,
        ),
        Err(_) => DiagnosticResult::new(
            "Nightly toolchain".to_string(),
            false,
            "rustup not found in PATH".to_string(),
            true,
        ),
    }
}

async fn check_rustdoc_json() -> DiagnosticResult {
    // First check if rustdoc is available
    match rustdoc::get_rustdoc_version().await {
        Ok(version) => {
            // Try to test JSON generation using the unified function
            match rustdoc::test_rustdoc_json().await {
                Ok(_) => DiagnosticResult::new(
                    "Rustdoc JSON".to_string(),
                    true,
                    format!(
                        "{} with JSON support (toolchain: {})",
                        version,
                        rustdoc::REQUIRED_TOOLCHAIN
                    ),
                    false,
                ),
                Err(e) => {
                    tracing::debug!("Rustdoc JSON test failed: {}", e);
                    DiagnosticResult::new(
                        "Rustdoc JSON".to_string(),
                        false,
                        format!("JSON generation failed: {e}"),
                        false,
                    )
                }
            }
        }
        Err(_) => DiagnosticResult::new(
            "Rustdoc JSON".to_string(),
            false,
            "rustdoc not found in PATH".to_string(),
            false,
        ),
    }
}

async fn check_git_installation() -> DiagnosticResult {
    match Command::new("git").arg("--version").output() {
        Ok(output) if output.status.success() => {
            let version = String::from_utf8_lossy(&output.stdout).trim().to_string();
            DiagnosticResult::new("Git".to_string(), true, version, true)
        }
        Ok(_) => DiagnosticResult::new(
            "Git".to_string(),
            false,
            "git command failed".to_string(),
            true,
        ),
        Err(_) => DiagnosticResult::new(
            "Git".to_string(),
            false,
            "git not found in PATH".to_string(),
            true,
        ),
    }
}

async fn check_network_connectivity() -> DiagnosticResult {
    let client = match reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .user_agent("rust-docs-mcp-doctor/1.0")
        .build()
    {
        Ok(client) => client,
        Err(e) => {
            return DiagnosticResult::new(
                "Network".to_string(),
                false,
                format!("Failed to create HTTP client: {e}"),
                false,
            );
        }
    };

    // Test crates.io API
    tracing::debug!("Testing crates.io connectivity...");
    match client
        .get("https://crates.io/api/v1/crates/serde")
        .send()
        .await
    {
        Ok(response) => {
            let status = response.status();
            tracing::debug!("crates.io response status: {}", status);

            if status.is_success() {
                // Try to read a small portion of the response to ensure it's valid
                match response.text().await {
                    Ok(body) => {
                        tracing::debug!("crates.io response body length: {}", body.len());

                        // Also test GitHub connectivity
                        tracing::debug!("Testing GitHub connectivity...");
                        match client.get("https://api.github.com").send().await {
                            Ok(gh_response) => {
                                let gh_status = gh_response.status();
                                tracing::debug!("GitHub response status: {}", gh_status);

                                if gh_status.is_success() {
                                    DiagnosticResult::new(
                                        "Network".to_string(),
                                        true,
                                        format!(
                                            "crates.io ({status}) and GitHub ({gh_status}) reachable"
                                        ),
                                        false,
                                    )
                                } else {
                                    DiagnosticResult::new(
                                        "Network".to_string(),
                                        false,
                                        format!(
                                            "crates.io reachable ({status}) but GitHub unreachable ({gh_status})"
                                        ),
                                        false,
                                    )
                                }
                            }
                            Err(e) => {
                                eprintln!("DEBUG: GitHub request error: {e}");
                                DiagnosticResult::new(
                                    "Network".to_string(),
                                    false,
                                    format!("crates.io reachable ({status}) but GitHub error: {e}"),
                                    false,
                                )
                            }
                        }
                    }
                    Err(e) => {
                        eprintln!("DEBUG: Failed to read crates.io response body: {e}");
                        DiagnosticResult::new(
                            "Network".to_string(),
                            false,
                            format!(
                                "crates.io responded ({status}) but failed to read response: {e}"
                            ),
                            false,
                        )
                    }
                }
            } else {
                DiagnosticResult::new(
                    "Network".to_string(),
                    false,
                    format!("crates.io returned error status: {status}"),
                    false,
                )
            }
        }
        Err(e) => {
            eprintln!("DEBUG: crates.io request error: {e}");
            DiagnosticResult::new(
                "Network".to_string(),
                false,
                format!("Unable to reach crates.io: {e}"),
                false,
            )
        }
    }
}

async fn check_cache_directory(cache_dir: Option<std::path::PathBuf>) -> DiagnosticResult {
    let cache_path = match cache_dir {
        Some(dir) => dir,
        None => match dirs::home_dir() {
            Some(home) => home.join(".rust-docs-mcp").join("cache"),
            None => {
                return DiagnosticResult::new(
                    "Cache directory".to_string(),
                    false,
                    "Unable to determine home directory".to_string(),
                    false,
                );
            }
        },
    };

    // Check if directory exists or can be created
    if !cache_path.exists() {
        match fs::create_dir_all(&cache_path) {
            Ok(_) => {}
            Err(e) => {
                return DiagnosticResult::new(
                    "Cache directory".to_string(),
                    false,
                    format!("Cannot create cache directory: {e}"),
                    false,
                );
            }
        }
    }

    // Test write permissions
    let test_file = cache_path.join(".test_write");
    match fs::write(&test_file, "test") {
        Ok(_) => {
            let _ = fs::remove_file(&test_file);

            // Check available disk space
            match fs4::available_space(&cache_path) {
                Ok(available_bytes) => {
                    let available_formatted = format_bytes(available_bytes);
                    // Warn if less than 1GB available
                    if available_bytes < 1_073_741_824 {
                        DiagnosticResult::new(
                            "Cache directory".to_string(),
                            false,
                            format!(
                                "{} (writable, but only {} available - at least 1GB recommended)",
                                cache_path.display(),
                                available_formatted
                            ),
                            false,
                        )
                    } else {
                        DiagnosticResult::new(
                            "Cache directory".to_string(),
                            true,
                            format!(
                                "{} (writable, {} available)",
                                cache_path.display(),
                                available_formatted
                            ),
                            false,
                        )
                    }
                }
                Err(e) => {
                    // If disk space check fails, just report that it's writable
                    tracing::debug!("Failed to check disk space: {}", e);
                    DiagnosticResult::new(
                        "Cache directory".to_string(),
                        true,
                        format!("{} (writable)", cache_path.display()),
                        false,
                    )
                }
            }
        }
        Err(e) => DiagnosticResult::new(
            "Cache directory".to_string(),
            false,
            format!("Directory not writable: {e}"),
            false,
        ),
    }
}

async fn check_optional_dependencies() -> DiagnosticResult {
    let mut messages = Vec::new();

    // Check for codesign on macOS
    #[cfg(target_os = "macos")]
    {
        match Command::new("codesign").arg("--version").output() {
            Ok(output) if output.status.success() => {
                messages.push("codesign available".to_string());
            }
            _ => {
                messages.push("codesign not available (optional for binary signing)".to_string());
            }
        }
    }

    // Check for GITHUB_TOKEN
    match std::env::var("GITHUB_TOKEN") {
        Ok(_) => {
            messages.push("GITHUB_TOKEN set (enables authenticated GitHub access)".to_string());
        }
        Err(_) => {
            messages.push(
                "GITHUB_TOKEN not set (optional: enables private repos and higher rate limits)"
                    .to_string(),
            );
        }
    }

    // If no optional dependencies to check, return success
    if messages.is_empty() {
        messages.push("No optional dependencies to check".to_string());
    }

    DiagnosticResult::new(
        "Optional dependencies".to_string(),
        true,
        messages.join(", "),
        false,
    )
}

fn format_bytes(bytes: u64) -> String {
    const UNITS: &[&str] = &["B", "KB", "MB", "GB", "TB"];
    let mut size = bytes as f64;
    let mut unit_index = 0;

    while size >= 1024.0 && unit_index < UNITS.len() - 1 {
        size /= 1024.0;
        unit_index += 1;
    }

    if unit_index == 0 {
        format!("{} {}", size as u64, UNITS[unit_index])
    } else {
        format!("{:.2} {}", size, UNITS[unit_index])
    }
}

pub fn print_results(results: &[DiagnosticResult]) {
    println!("🔍 rust-docs-mcp doctor\n");

    let mut failed_count = 0;
    for result in results {
        let icon = if result.success { "✅" } else { "❌" };
        println!("{} {}: {}", icon, result.name, result.message);

        if !result.success {
            failed_count += 1;
        }
    }

    if failed_count > 0 {
        println!(
            "\n[ERROR] Doctor found {} issue{}.",
            failed_count,
            if failed_count == 1 { "" } else { "s" }
        );

        // Print specific error messages and suggestions
        for result in results {
            if !result.success {
                match result.name.as_str() {
                    "Rust toolchain" => {
                        println!(
                            "\nRust toolchain is required. Please install Rust from https://rustup.rs/"
                        );
                    }
                    "Nightly toolchain" => {
                        println!(
                            "\nNightly toolchain is required for rustdoc JSON generation. Install with:"
                        );
                        println!("  rustup toolchain install nightly");
                    }
                    "Git" => {
                        println!(
                            "\nGit is required for repository operations. Please install Git from https://git-scm.com/"
                        );
                    }
                    "Rustdoc JSON" => {
                        println!(
                            "\nRustdoc JSON generation failed. Ensure nightly toolchain is properly installed:"
                        );
                        println!("  rustup toolchain install nightly");
                    }
                    "Network" => {
                        println!(
                            "\nNetwork connectivity issues detected. Check your internet connection."
                        );
                    }
                    "Cache directory" => {
                        println!(
                            "\nCache directory issues detected. Check file permissions and disk space."
                        );
                        if result.message.contains("available")
                            && result.message.contains("recommended")
                        {
                            println!(
                                "Consider freeing up disk space. At least 1GB is recommended for caching documentation."
                            );
                        }
                    }
                    _ => {}
                }
            }
        }

        println!("\nPlease fix the above errors before using rust-docs-mcp.");
    } else {
        println!("\n✅ All checks passed! rust-docs-mcp is ready to use.");
    }
}

pub fn exit_code(results: &[DiagnosticResult]) -> i32 {
    let mut has_failures = false;
    let mut has_critical_failures = false;

    for result in results {
        if !result.success {
            has_failures = true;
            if result.critical {
                has_critical_failures = true;
            }
        }
    }

    if has_critical_failures {
        2 // Critical system dependency missing
    } else if has_failures {
        1 // One or more checks failed
    } else {
        0 // All checks passed
    }
}

pub fn print_results_json(results: &[DiagnosticResult]) -> Result<()> {
    let json_output = serde_json::json!({
        "results": results,
        "summary": {
            "total_checks": results.len(),
            "passed": results.iter().filter(|r| r.success).count(),
            "failed": results.iter().filter(|r| !r.success).count(),
            "critical_failures": results.iter().filter(|r| !r.success && r.critical).count(),
        },
        "exit_code": exit_code(results),
    });

    println!("{}", serde_json::to_string_pretty(&json_output)?);
    Ok(())
}

/// Run diagnostics and print results with status message
/// This is a convenience function used after install/update operations
pub async fn run_and_print_diagnostics() -> Result<()> {
    println!("\n🔍 Running system diagnostics...\n");
    let results = run_diagnostics(None).await?;
    print_results(&results);

    let exit_code = exit_code(&results);
    if exit_code != 0 {
        println!("\n⚠️  Some diagnostic checks failed. Please address the issues above.");
        println!("You can run 'rust-docs-mcp doctor' anytime to check system status.");
    } else {
        println!("\n✅ All system checks passed!");
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_diagnostic_result_creation() {
        let result =
            DiagnosticResult::new("Test".to_string(), true, "Test passed".to_string(), false);
        assert_eq!(result.name, "Test");
        assert!(result.success);
        assert_eq!(result.message, "Test passed");
        assert!(!result.critical);
    }

    #[test]
    fn test_exit_code_all_success() {
        let results = vec![
            DiagnosticResult::new("Test 1".to_string(), true, "Success".to_string(), false),
            DiagnosticResult::new("Test 2".to_string(), true, "Success".to_string(), true),
        ];
        assert_eq!(exit_code(&results), 0);
    }

    #[test]
    fn test_exit_code_non_critical_failure() {
        let results = vec![
            DiagnosticResult::new("Test 1".to_string(), true, "Success".to_string(), false),
            DiagnosticResult::new("Test 2".to_string(), false, "Failed".to_string(), false),
        ];
        assert_eq!(exit_code(&results), 1);
    }

    #[test]
    fn test_exit_code_critical_failure() {
        let results = vec![
            DiagnosticResult::new("Test 1".to_string(), false, "Failed".to_string(), true),
            DiagnosticResult::new("Test 2".to_string(), false, "Failed".to_string(), false),
        ];
        assert_eq!(exit_code(&results), 2);
    }

    #[tokio::test]
    async fn test_check_rust_toolchain() {
        // This test will pass if rustc is installed
        let result = check_rust_toolchain().await;
        assert_eq!(result.name, "Rust toolchain");
        // We can't guarantee the success state in all environments
        // but we can verify it returns a valid DiagnosticResult
        assert!(result.critical);
    }

    #[tokio::test]
    async fn test_check_git_installation() {
        // This test will pass if git is installed
        let result = check_git_installation().await;
        assert_eq!(result.name, "Git");
        assert!(result.critical);
    }

    #[tokio::test]
    async fn test_cache_directory_with_none() {
        let result = check_cache_directory(None).await;
        assert_eq!(result.name, "Cache directory");
        // The success depends on whether the directory can be created
        // but we can verify it returns a valid DiagnosticResult
        assert!(!result.critical);
    }

    #[tokio::test]
    async fn test_cache_directory_with_temp_dir() {
        let temp_dir = tempfile::tempdir().unwrap();
        let result = check_cache_directory(Some(temp_dir.path().to_path_buf())).await;
        assert_eq!(result.name, "Cache directory");
        assert!(result.success);
        assert!(!result.critical);
    }

    #[tokio::test]
    async fn test_optional_dependencies() {
        let result = check_optional_dependencies().await;
        assert_eq!(result.name, "Optional dependencies");
        // Optional dependencies should always return success
        assert!(result.success);
        assert!(!result.critical);
    }

    #[test]
    fn test_print_results_output() {
        // This is a simple test to ensure print_results doesn't panic
        let results = vec![
            DiagnosticResult::new("Test 1".to_string(), true, "Success".to_string(), false),
            DiagnosticResult::new("Test 2".to_string(), false, "Failed".to_string(), true),
        ];
        // This will print to stdout, but we're mainly testing it doesn't panic
        print_results(&results);
    }

    #[test]
    fn test_format_bytes() {
        assert_eq!(format_bytes(0), "0 B");
        assert_eq!(format_bytes(1023), "1023 B");
        assert_eq!(format_bytes(1024), "1.00 KB");
        assert_eq!(format_bytes(1536), "1.50 KB");
        assert_eq!(format_bytes(1048576), "1.00 MB");
        assert_eq!(format_bytes(1073741824), "1.00 GB");
        assert_eq!(format_bytes(1099511627776), "1.00 TB");
        assert_eq!(format_bytes(2147483648), "2.00 GB");
    }
}
