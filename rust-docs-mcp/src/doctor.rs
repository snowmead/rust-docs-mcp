use anyhow::{anyhow, Result};
use std::process::Command;
use std::path::Path;
use std::fs;
use reqwest;
use dirs;

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

pub async fn run_diagnostics(cache_dir: Option<std::path::PathBuf>) -> Result<Vec<DiagnosticResult>> {
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
            DiagnosticResult::new(
                "Rust toolchain".to_string(),
                true,
                version,
                true,
            )
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
    match Command::new("rustup").args(&["toolchain", "list"]).output() {
        Ok(output) if output.status.success() => {
            let toolchains = String::from_utf8_lossy(&output.stdout);
            if toolchains.contains("nightly") {
                // Try to get nightly version
                match Command::new("rustc").args(&["+nightly", "--version"]).output() {
                    Ok(nightly_output) if nightly_output.status.success() => {
                        let version = String::from_utf8_lossy(&nightly_output.stdout).trim().to_string();
                        DiagnosticResult::new(
                            "Nightly toolchain".to_string(),
                            true,
                            version,
                            true,
                        )
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
    match Command::new("rustdoc").arg("--version").output() {
        Ok(output) if output.status.success() => {
            let version = String::from_utf8_lossy(&output.stdout).trim().to_string();
            
            // Try to create a simple test for JSON generation
            let temp_dir = match tempfile::tempdir() {
                Ok(dir) => dir,
                Err(_) => return DiagnosticResult::new(
                    "Rustdoc JSON".to_string(),
                    false,
                    "Failed to create temporary directory for testing".to_string(),
                    false,
                ),
            };
            
            let test_file = temp_dir.path().join("lib.rs");
            if let Err(_) = fs::write(&test_file, "//! Test crate\npub fn test() {}") {
                return DiagnosticResult::new(
                    "Rustdoc JSON".to_string(),
                    false,
                    "Failed to create test file".to_string(),
                    false,
                );
            }
            
            // Try to generate JSON documentation
            match Command::new("rustdoc")
                .args(&[
                    "+nightly",
                    "--output-format", "json",
                    "--crate-name", "test",
                    test_file.to_str().unwrap(),
                ])
                .output() {
                Ok(json_output) if json_output.status.success() => {
                    DiagnosticResult::new(
                        "Rustdoc JSON".to_string(),
                        true,
                        format!("{} with JSON support", version),
                        false,
                    )
                }
                _ => DiagnosticResult::new(
                    "Rustdoc JSON".to_string(),
                    false,
                    "JSON generation failed - ensure nightly toolchain is installed".to_string(),
                    false,
                ),
            }
        }
        Ok(_) => DiagnosticResult::new(
            "Rustdoc JSON".to_string(),
            false,
            "rustdoc command failed".to_string(),
            false,
        ),
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
            DiagnosticResult::new(
                "Git".to_string(),
                true,
                version,
                true,
            )
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
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .unwrap();
    
    // Test crates.io API
    match client.get("https://crates.io/api/v1/crates/serde").send().await {
        Ok(response) if response.status().is_success() => {
            // Also test GitHub connectivity
            match client.get("https://api.github.com").send().await {
                Ok(gh_response) if gh_response.status().is_success() => {
                    DiagnosticResult::new(
                        "Network".to_string(),
                        true,
                        "crates.io and GitHub reachable".to_string(),
                        false,
                    )
                }
                _ => DiagnosticResult::new(
                    "Network".to_string(),
                    false,
                    "crates.io reachable but GitHub unreachable".to_string(),
                    false,
                ),
            }
        }
        _ => DiagnosticResult::new(
            "Network".to_string(),
            false,
            "Unable to reach crates.io - check network connection".to_string(),
            false,
        ),
    }
}

async fn check_cache_directory(cache_dir: Option<std::path::PathBuf>) -> DiagnosticResult {
    let cache_path = match cache_dir {
        Some(dir) => dir,
        None => {
            match dirs::home_dir() {
                Some(home) => home.join(".rust-docs-mcp").join("cache"),
                None => return DiagnosticResult::new(
                    "Cache directory".to_string(),
                    false,
                    "Unable to determine home directory".to_string(),
                    false,
                ),
            }
        }
    };
    
    // Check if directory exists or can be created
    if !cache_path.exists() {
        match fs::create_dir_all(&cache_path) {
            Ok(_) => {},
            Err(e) => return DiagnosticResult::new(
                "Cache directory".to_string(),
                false,
                format!("Cannot create cache directory: {}", e),
                false,
            ),
        }
    }
    
    // Test write permissions
    let test_file = cache_path.join(".test_write");
    match fs::write(&test_file, "test") {
        Ok(_) => {
            let _ = fs::remove_file(&test_file);
            
            // Check available disk space
            match fs::metadata(&cache_path) {
                Ok(_) => {
                    // We can't easily get disk space in a cross-platform way without additional dependencies
                    // For now, just confirm it's writable
                    DiagnosticResult::new(
                        "Cache directory".to_string(),
                        true,
                        format!("{} (writable)", cache_path.display()),
                        false,
                    )
                }
                Err(_) => DiagnosticResult::new(
                    "Cache directory".to_string(),
                    true,
                    format!("{} (writable)", cache_path.display()),
                    false,
                ),
            }
        }
        Err(e) => DiagnosticResult::new(
            "Cache directory".to_string(),
            false,
            format!("Directory not writable: {}", e),
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

pub fn print_results(results: &[DiagnosticResult]) {
    println!("🔍 rust-docs-mcp doctor\n");
    
    let mut failed_count = 0;
    let mut critical_failed = false;
    
    for result in results {
        let icon = if result.success { "✅" } else { "❌" };
        println!("{} {}: {}", icon, result.name, result.message);
        
        if !result.success {
            failed_count += 1;
            if result.critical {
                critical_failed = true;
            }
        }
    }
    
    if failed_count > 0 {
        println!("\n[ERROR] Doctor found {} issue{}.", failed_count, if failed_count == 1 { "" } else { "s" });
        
        // Print specific error messages and suggestions
        for result in results {
            if !result.success {
                match result.name.as_str() {
                    "Rust toolchain" => {
                        println!("\nRust toolchain is required. Please install Rust from https://rustup.rs/");
                    }
                    "Nightly toolchain" => {
                        println!("\nNightly toolchain is required for rustdoc JSON generation. Install with:");
                        println!("  rustup toolchain install nightly");
                    }
                    "Git" => {
                        println!("\nGit is required for repository operations. Please install Git from https://git-scm.com/");
                    }
                    "Rustdoc JSON" => {
                        println!("\nRustdoc JSON generation failed. Ensure nightly toolchain is properly installed:");
                        println!("  rustup toolchain install nightly");
                    }
                    "Network" => {
                        println!("\nNetwork connectivity issues detected. Check your internet connection.");
                    }
                    "Cache directory" => {
                        println!("\nCache directory issues detected. Check file permissions and disk space.");
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