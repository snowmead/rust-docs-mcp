//! Workspace handling utilities for Rust crates
//!
//! This module provides functionality for detecting and managing Rust workspace crates,
//! including member detection and metadata extraction.

use anyhow::{Context, Result, anyhow};
use std::fs;
use std::path::Path;
use toml::Value;

/// Workspace-related utilities
pub struct WorkspaceHandler;

impl WorkspaceHandler {
    /// Check if a Cargo.toml represents a virtual manifest (workspace without [package])
    pub fn is_workspace(cargo_toml_path: &Path) -> Result<bool> {
        let content = fs::read_to_string(cargo_toml_path).with_context(|| {
            format!("Failed to read Cargo.toml at {}", cargo_toml_path.display())
        })?;

        let parsed: Value = toml::from_str(&content).with_context(|| {
            format!(
                "Failed to parse Cargo.toml at {}",
                cargo_toml_path.display()
            )
        })?;

        // A virtual manifest has [workspace] but no [package]
        let has_workspace = parsed.get("workspace").is_some();
        let has_package = parsed.get("package").is_some();

        Ok(has_workspace && !has_package)
    }

    /// Get workspace members from a workspace Cargo.toml
    pub fn get_workspace_members(cargo_toml_path: &Path) -> Result<Vec<String>> {
        let content = fs::read_to_string(cargo_toml_path).with_context(|| {
            format!("Failed to read Cargo.toml at {}", cargo_toml_path.display())
        })?;

        let parsed: Value = toml::from_str(&content).with_context(|| {
            format!(
                "Failed to parse Cargo.toml at {}",
                cargo_toml_path.display()
            )
        })?;

        let workspace = parsed
            .get("workspace")
            .ok_or_else(|| anyhow!("No [workspace] section found in Cargo.toml"))?;

        let members = workspace
            .get("members")
            .and_then(|m| m.as_array())
            .ok_or_else(|| anyhow!("No members array found in [workspace] section"))?;

        let mut member_list = Vec::new();
        for member in members {
            if let Some(member_str) = member.as_str() {
                // Expand glob patterns
                if member_str.contains('*') {
                    // For now, we'll skip glob patterns and handle them later if needed
                    // In the real implementation, we'd expand these patterns
                    if member_str == "examples/*" {
                        // Skip examples for now as requested
                        continue;
                    }
                } else {
                    member_list.push(member_str.to_string());
                }
            }
        }

        Ok(member_list)
    }

    /// Get the package name from a Cargo.toml file
    pub fn get_package_name(cargo_toml_path: &Path) -> Result<String> {
        let content = fs::read_to_string(cargo_toml_path).with_context(|| {
            format!("Failed to read Cargo.toml at {}", cargo_toml_path.display())
        })?;

        let parsed: Value = toml::from_str(&content).with_context(|| {
            format!(
                "Failed to parse Cargo.toml at {}",
                cargo_toml_path.display()
            )
        })?;

        let package = parsed
            .get("package")
            .ok_or_else(|| anyhow!("No [package] section found in Cargo.toml"))?;

        let name = package
            .get("name")
            .and_then(|n| n.as_str())
            .ok_or_else(|| anyhow!("No 'name' field found in [package] section"))?;

        Ok(name.to_string())
    }

    /// Get the package version from a Cargo.toml file
    pub fn get_package_version(cargo_toml_path: &Path) -> Result<String> {
        let content = fs::read_to_string(cargo_toml_path).with_context(|| {
            format!("Failed to read Cargo.toml at {}", cargo_toml_path.display())
        })?;

        let parsed: Value = toml::from_str(&content).with_context(|| {
            format!(
                "Failed to parse Cargo.toml at {}",
                cargo_toml_path.display()
            )
        })?;

        let package = parsed
            .get("package")
            .ok_or_else(|| anyhow!("No [package] section found in Cargo.toml"))?;

        let version = package
            .get("version")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow!("No 'version' field found in [package] section"))?;

        Ok(version.to_string())
    }

    /// Extract member name from a member path
    pub fn extract_member_name(member_path: &str) -> &str {
        member_path.split('/').next_back().unwrap_or(member_path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_extract_member_name() {
        assert_eq!(WorkspaceHandler::extract_member_name("crates/rmcp"), "rmcp");
        assert_eq!(WorkspaceHandler::extract_member_name("rmcp"), "rmcp");
        assert_eq!(
            WorkspaceHandler::extract_member_name("path/to/deep/crate"),
            "crate"
        );
    }

    #[test]
    fn test_get_package_version() -> Result<()> {
        let temp_dir = TempDir::new()?;

        // Test regular crate with version
        let cargo_toml = temp_dir.path().join("Cargo.toml");
        fs::write(
            &cargo_toml,
            r#"
[package]
name = "test-crate"
version = "1.2.3"
"#,
        )?;

        let version = WorkspaceHandler::get_package_version(&cargo_toml)?;
        assert_eq!(version, "1.2.3");

        // Test crate without version field
        let no_version_toml = temp_dir.path().join("no_version.toml");
        fs::write(
            &no_version_toml,
            r#"
[package]
name = "test-crate"
"#,
        )?;

        let result = WorkspaceHandler::get_package_version(&no_version_toml);
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("No 'version' field")
        );

        Ok(())
    }

    #[test]
    fn test_workspace_detection() -> Result<()> {
        let temp_dir = TempDir::new()?;

        // Test virtual manifest (workspace without package)
        let workspace_toml = temp_dir.path().join("workspace.toml");
        fs::write(
            &workspace_toml,
            r#"
[workspace]
members = ["crate-a", "crate-b"]
"#,
        )?;
        assert!(WorkspaceHandler::is_workspace(&workspace_toml)?);

        // Test regular crate (has package)
        let crate_toml = temp_dir.path().join("crate.toml");
        fs::write(
            &crate_toml,
            r#"
[package]
name = "my-crate"
version = "0.1.0"
"#,
        )?;
        assert!(!WorkspaceHandler::is_workspace(&crate_toml)?);

        // Test workspace with package (not a virtual manifest)
        let mixed_toml = temp_dir.path().join("mixed.toml");
        fs::write(
            &mixed_toml,
            r#"
[package]
name = "my-crate"
version = "0.1.0"

[workspace]
members = ["sub-crate"]
"#,
        )?;
        assert!(!WorkspaceHandler::is_workspace(&mixed_toml)?);

        Ok(())
    }
}
