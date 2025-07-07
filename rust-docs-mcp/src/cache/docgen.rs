//! Documentation generation for Rust crates
//!
//! This module handles running `cargo rustdoc` to generate JSON documentation
//! for both regular crates and workspace members.

use crate::cache::storage::CacheStorage;
use crate::cache::workspace::WorkspaceHandler;
use anyhow::{Context, Result, bail};
use std::path::{Path, PathBuf};
use std::process::Command;

/// The pinned nightly toolchain version compatible with rustdoc-types 0.53.0
const REQUIRED_TOOLCHAIN: &str = "nightly-2025-06-23";

/// Service for generating documentation from Rust crates
#[derive(Debug, Clone)]
pub struct DocGenerator {
    storage: CacheStorage,
}

impl DocGenerator {
    /// Create a new documentation generator
    pub fn new(storage: CacheStorage) -> Self {
        Self { storage }
    }

    /// Validate that the required toolchain is available
    async fn validate_toolchain(&self) -> Result<()> {
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

    /// Generate JSON documentation for a crate
    pub async fn generate_docs(&self, name: &str, version: &str) -> Result<PathBuf> {
        // Validate toolchain before generating docs
        self.validate_toolchain().await?;

        let source_path = self.storage.source_path(name, version);
        let docs_path = self.storage.docs_path(name, version);

        if !source_path.exists() {
            bail!(
                "Source not found for {}-{}. Download it first.",
                name,
                version
            );
        }

        tracing::info!("Generating documentation for {}-{}", name, version);

        // Run cargo rustdoc with JSON output using pinned nightly toolchain
        let output = Command::new("cargo")
            .args([
                &format!("+{REQUIRED_TOOLCHAIN}"),
                "rustdoc",
                "--all-features",
                "--",
                "--output-format",
                "json",
                "-Z",
                "unstable-options",
            ])
            .current_dir(&source_path)
            .output()
            .context("Failed to run cargo rustdoc")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            bail!("Failed to generate documentation: {}", stderr);
        }

        // Find the generated JSON file in target/doc
        let doc_dir = source_path.join("target").join("doc");
        let json_file = self.find_json_doc(&doc_dir, name)?;

        // Copy the JSON file to our cache location
        std::fs::copy(&json_file, &docs_path).context("Failed to copy documentation to cache")?;

        // Generate and save dependency information
        self.generate_dependencies(name, version).await?;

        // Update metadata to reflect that docs are now generated
        self.storage.save_metadata(name, version)?;

        tracing::info!(
            "Successfully generated documentation for {}-{}",
            name,
            version
        );
        Ok(docs_path)
    }

    /// Generate JSON documentation for a workspace member
    pub async fn generate_workspace_member_docs(
        &self,
        name: &str,
        version: &str,
        member_path: &str,
    ) -> Result<PathBuf> {
        // Validate toolchain before generating docs
        self.validate_toolchain().await?;

        let source_path = self.storage.source_path(name, version);
        let member_full_path = source_path.join(member_path);

        if !source_path.exists() {
            bail!(
                "Source not found for {}-{}. Download it first.",
                name,
                version
            );
        }

        if !member_full_path.exists() {
            bail!(
                "Workspace member not found at path: {}",
                member_full_path.display()
            );
        }

        // Get the actual package name from the member's Cargo.toml
        let member_cargo_toml = member_full_path.join("Cargo.toml");
        let package_name = WorkspaceHandler::get_package_name(&member_cargo_toml)?;

        // Extract the member name from the path (last directory)
        let member_name = WorkspaceHandler::extract_member_name(member_path);
        let docs_path = self.storage.member_docs_path(name, version, member_name);

        tracing::info!(
            "Generating documentation for workspace member {} (package: {}) in {}-{}",
            member_path,
            package_name,
            name,
            version
        );

        // Run cargo rustdoc with JSON output for the specific package using pinned nightly toolchain
        let output = Command::new("cargo")
            .args([
                &format!("+{REQUIRED_TOOLCHAIN}"),
                "rustdoc",
                "-p",
                &package_name,
                "--all-features",
                "--",
                "--output-format",
                "json",
                "-Z",
                "unstable-options",
            ])
            .current_dir(&source_path)
            .output()
            .context("Failed to run cargo rustdoc")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            bail!("Failed to generate documentation: {}", stderr);
        }

        // Find the generated JSON file in target/doc
        let doc_dir = source_path.join("target").join("doc");
        let json_file = self.find_json_doc(&doc_dir, &package_name)?;

        // Ensure the member directory exists in cache
        if let Some(parent) = docs_path.parent() {
            self.storage.ensure_dir(parent)?;
        } else {
            bail!(
                "Invalid docs path: no parent directory for {}",
                docs_path.display()
            );
        }

        // Copy the JSON file to our cache location
        std::fs::copy(&json_file, &docs_path)
            .context("Failed to copy workspace member documentation to cache")?;

        // Generate and save dependency information for the member
        self.generate_workspace_member_dependencies(name, version, member_path)
            .await?;

        tracing::info!(
            "Successfully generated documentation for workspace member {} in {}-{}",
            member_path,
            name,
            version
        );
        Ok(docs_path)
    }

    /// Find the JSON documentation file for a crate in the target/doc directory
    fn find_json_doc(&self, doc_dir: &Path, crate_name: &str) -> Result<PathBuf> {
        // The JSON file is typically named after the crate, with hyphens replaced by underscores
        let json_name = crate_name.replace('-', "_");
        let json_file = doc_dir.join(format!("{json_name}.json"));

        if json_file.exists() {
            return Ok(json_file);
        }

        // If not found, try to find any .json file in the directory
        let entries = std::fs::read_dir(doc_dir)
            .with_context(|| format!("Failed to read doc directory: {}", doc_dir.display()))?;

        for entry in entries {
            let entry = entry?;
            let path = entry.path();
            if path.extension().and_then(|s| s.to_str()) == Some("json") {
                return Ok(path);
            }
        }

        bail!(
            "No JSON documentation file found for crate '{}' in {}",
            crate_name,
            doc_dir.display()
        );
    }

    /// Generate and save dependency information for a crate
    async fn generate_dependencies(&self, name: &str, version: &str) -> Result<()> {
        let source_path = self.storage.source_path(name, version);
        let deps_path = self.storage.dependencies_path(name, version);

        tracing::info!("Generating dependency information for {}-{}", name, version);

        // Run cargo metadata to get dependency information
        let output = Command::new("cargo")
            .args(["metadata", "--format-version", "1"])
            .current_dir(&source_path)
            .output()
            .context("Failed to run cargo metadata")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            bail!("Failed to generate dependency metadata: {}", stderr);
        }

        // Save the raw metadata output
        tokio::fs::write(&deps_path, &output.stdout)
            .await
            .context("Failed to write dependencies to cache")?;

        Ok(())
    }

    /// Generate and save dependency information for a workspace member
    async fn generate_workspace_member_dependencies(
        &self,
        name: &str,
        version: &str,
        member_path: &str,
    ) -> Result<()> {
        let source_path = self.storage.source_path(name, version);
        let member_name = WorkspaceHandler::extract_member_name(member_path);
        let deps_path = self
            .storage
            .member_dependencies_path(name, version, member_name);

        tracing::info!(
            "Generating dependency information for workspace member {} in {}-{}",
            member_path,
            name,
            version
        );

        // Path to the member's Cargo.toml
        let member_cargo_toml = source_path.join(member_path).join("Cargo.toml");

        // Run cargo metadata with --manifest-path for the specific member
        let output = Command::new("cargo")
            .args([
                "metadata",
                "--format-version",
                "1",
                "--manifest-path",
                &member_cargo_toml.to_string_lossy(),
            ])
            .output()
            .context("Failed to run cargo metadata")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            bail!("Failed to generate dependency metadata: {}", stderr);
        }

        // Ensure the member directory exists
        if let Some(parent) = deps_path.parent() {
            self.storage.ensure_dir(parent)?;
        } else {
            bail!(
                "Invalid deps path: no parent directory for {}",
                deps_path.display()
            );
        }

        // Save the raw metadata output
        tokio::fs::write(&deps_path, &output.stdout)
            .await
            .context("Failed to write dependencies to cache")?;

        Ok(())
    }

    /// Load dependency information from cache
    pub async fn load_dependencies(&self, name: &str, version: &str) -> Result<serde_json::Value> {
        let deps_path = self.storage.dependencies_path(name, version);

        if !deps_path.exists() {
            bail!("Dependencies not found for {}-{}", name, version);
        }

        let json_string = tokio::fs::read_to_string(&deps_path)
            .await
            .context("Failed to read dependencies file")?;

        let deps: serde_json::Value =
            serde_json::from_str(&json_string).context("Failed to parse dependencies JSON")?;

        Ok(deps)
    }

    /// Load documentation from cache
    pub async fn load_docs(&self, name: &str, version: &str) -> Result<serde_json::Value> {
        let docs_path = self.storage.docs_path(name, version);

        if !docs_path.exists() {
            bail!("Documentation not found for {}-{}", name, version);
        }

        let json_string = tokio::fs::read_to_string(&docs_path)
            .await
            .context("Failed to read documentation file")?;

        let docs: serde_json::Value =
            serde_json::from_str(&json_string).context("Failed to parse documentation JSON")?;

        Ok(docs)
    }

    /// Load workspace member documentation from cache
    pub async fn load_member_docs(
        &self,
        name: &str,
        version: &str,
        member_name: &str,
    ) -> Result<serde_json::Value> {
        let docs_path = self.storage.member_docs_path(name, version, member_name);

        if !docs_path.exists() {
            bail!(
                "Documentation not found for workspace member {} in {}-{}",
                member_name,
                name,
                version
            );
        }

        let json_string = tokio::fs::read_to_string(&docs_path)
            .await
            .context("Failed to read member documentation file")?;

        let docs: serde_json::Value = serde_json::from_str(&json_string)
            .context("Failed to parse member documentation JSON")?;

        Ok(docs)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn test_docgen_creation() {
        let temp_dir = TempDir::new().unwrap();
        let storage = CacheStorage::new(Some(temp_dir.path().to_path_buf())).unwrap();
        let docgen = DocGenerator::new(storage);

        // Just verify it was created successfully
        assert!(format!("{docgen:?}").contains("DocGenerator"));
    }

    #[test]
    fn test_find_json_doc_not_found() {
        let temp_dir = TempDir::new().unwrap();
        let storage = CacheStorage::new(Some(temp_dir.path().to_path_buf())).unwrap();
        let docgen = DocGenerator::new(storage);

        let doc_dir = temp_dir.path().join("doc");
        fs::create_dir_all(&doc_dir).unwrap();

        let result = docgen.find_json_doc(&doc_dir, "nonexistent");
        assert!(result.is_err());
    }

    #[test]
    fn test_find_json_doc_found() {
        let temp_dir = TempDir::new().unwrap();
        let storage = CacheStorage::new(Some(temp_dir.path().to_path_buf())).unwrap();
        let docgen = DocGenerator::new(storage);

        let doc_dir = temp_dir.path().join("doc");
        fs::create_dir_all(&doc_dir).unwrap();

        // Create a JSON file
        let json_file = doc_dir.join("test_crate.json");
        fs::write(&json_file, "{}").unwrap();

        let result = docgen.find_json_doc(&doc_dir, "test_crate").unwrap();
        assert_eq!(result, json_file);
    }

    #[test]
    fn test_find_json_doc_with_underscore_conversion() {
        let temp_dir = TempDir::new().unwrap();
        let storage = CacheStorage::new(Some(temp_dir.path().to_path_buf())).unwrap();
        let docgen = DocGenerator::new(storage);

        let doc_dir = temp_dir.path().join("doc");
        fs::create_dir_all(&doc_dir).unwrap();

        // Create a JSON file with underscores (converted from hyphens)
        let json_file = doc_dir.join("test_crate.json");
        fs::write(&json_file, "{}").unwrap();

        let result = docgen.find_json_doc(&doc_dir, "test-crate").unwrap();
        assert_eq!(result, json_file);
    }
}
