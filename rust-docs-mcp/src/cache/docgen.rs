//! Documentation generation for Rust crates
//!
//! This module handles running `cargo rustdoc` to generate JSON documentation
//! for both regular crates and workspace members.

use crate::cache::constants::*;
use crate::cache::storage::CacheStorage;
use crate::cache::workspace::WorkspaceHandler;
use crate::rustdoc;
use crate::search::indexer::SearchIndexer;
use anyhow::{Context, Result, bail};
use std::path::{Path, PathBuf};
use std::process::Command;

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
        rustdoc::validate_toolchain().await
    }

    /// Generate JSON documentation for a crate
    pub async fn generate_docs(&self, name: &str, version: &str) -> Result<PathBuf> {
        // Validate toolchain before generating docs
        self.validate_toolchain().await?;

        let source_path = self.storage.source_path(name, version)?;
        let docs_path = self.storage.docs_path(name, version, None)?;

        if !source_path.exists() {
            bail!(
                "Source not found for {}-{}. Download it first.",
                name,
                version
            );
        }

        tracing::info!("Generating documentation for {}-{}", name, version);

        // Run cargo rustdoc with JSON output using unified function
        rustdoc::run_cargo_rustdoc_json(&source_path, None).await?;

        // Find the generated JSON file in target/doc
        let doc_dir = source_path.join(TARGET_DIR).join(DOC_DIR);
        let json_file = self.find_json_doc(&doc_dir, name)?;

        // Copy the JSON file to our cache location
        std::fs::copy(&json_file, &docs_path).context("Failed to copy documentation to cache")?;

        // Generate and save dependency information
        self.generate_dependencies(name, version).await?;

        // Update metadata to reflect that docs are now generated
        self.storage.save_metadata(name, version)?;

        // Create search index for the crate
        self.create_search_index(name, version, None)
            .await
            .context("Failed to create search index")?;

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

        let source_path = self.storage.source_path(name, version)?;
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
        let member_cargo_toml = member_full_path.join(CARGO_TOML);
        let package_name = WorkspaceHandler::get_package_name(&member_cargo_toml)?;

        // Use the full member path directly
        let docs_path = self.storage.docs_path(name, version, Some(member_path))?;

        tracing::info!(
            "Generating documentation for workspace member {} (package: {}) in {}-{}",
            member_path,
            package_name,
            name,
            version
        );

        // Run cargo rustdoc with JSON output for the specific package using unified function
        rustdoc::run_cargo_rustdoc_json(&source_path, Some(&package_name)).await?;

        // Find the generated JSON file in target/doc
        let doc_dir = source_path.join(TARGET_DIR).join(DOC_DIR);
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

        // Create search index for the workspace member
        self.create_search_index(name, version, Some(member_path))
            .await
            .context("Failed to create search index for workspace member")?;

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
        let source_path = self.storage.source_path(name, version)?;
        let deps_path = self.storage.dependencies_path(name, version, None)?;

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
        let source_path = self.storage.source_path(name, version)?;
        let deps_path = self
            .storage
            .member_path(name, version, member_path)?
            .join(DEPENDENCIES_FILE);

        tracing::info!(
            "Generating dependency information for workspace member {} in {}-{}",
            member_path,
            name,
            version
        );

        // Path to the member's Cargo.toml
        let member_cargo_toml = source_path.join(member_path).join(CARGO_TOML);

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
        let deps_path = self.storage.dependencies_path(name, version, None)?;

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

    /// Load documentation from cache for a crate or workspace member
    pub async fn load_docs(
        &self,
        name: &str,
        version: &str,
        member_name: Option<&str>,
    ) -> Result<serde_json::Value> {
        let docs_path = self.storage.docs_path(name, version, member_name)?;

        if !docs_path.exists() {
            if let Some(member) = member_name {
                bail!(
                    "Documentation not found for workspace member {} in {}-{}",
                    member,
                    name,
                    version
                );
            } else {
                bail!("Documentation not found for {}-{}", name, version);
            }
        }

        let json_string = tokio::fs::read_to_string(&docs_path)
            .await
            .context("Failed to read documentation file")?;

        let docs: serde_json::Value =
            serde_json::from_str(&json_string).context("Failed to parse documentation JSON")?;

        Ok(docs)
    }

    /// Create search index for a crate or workspace member
    pub async fn create_search_index(
        &self,
        name: &str,
        version: &str,
        member_name: Option<&str>,
    ) -> Result<()> {
        let log_prefix = if let Some(member) = member_name {
            format!("workspace member {member} in")
        } else {
            String::new()
        };

        tracing::info!(
            "Creating search index for {}{}-{}",
            log_prefix,
            name,
            version
        );

        // Load the generated documentation
        let docs_path = self.storage.docs_path(name, version, member_name)?;

        let docs_json = tokio::fs::read_to_string(&docs_path)
            .await
            .context("Failed to read documentation for indexing")?;

        let crate_data: rustdoc_types::Crate = serde_json::from_str(&docs_json)
            .context("Failed to parse documentation JSON for indexing")?;

        // Create the search indexer for this crate or workspace member
        let mut indexer = SearchIndexer::new_for_crate(name, version, &self.storage, member_name)?;

        // Add all crate items to the index
        indexer.add_crate_items(name, version, &crate_data)?;

        tracing::info!(
            "Successfully created search index for {}{}-{}",
            log_prefix,
            name,
            version
        );
        Ok(())
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

        let doc_dir = temp_dir.path().join(DOC_DIR);
        fs::create_dir_all(&doc_dir).unwrap();

        let result = docgen.find_json_doc(&doc_dir, "nonexistent");
        assert!(result.is_err());
    }

    #[test]
    fn test_find_json_doc_found() {
        let temp_dir = TempDir::new().unwrap();
        let storage = CacheStorage::new(Some(temp_dir.path().to_path_buf())).unwrap();
        let docgen = DocGenerator::new(storage);

        let doc_dir = temp_dir.path().join(DOC_DIR);
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

        let doc_dir = temp_dir.path().join(DOC_DIR);
        fs::create_dir_all(&doc_dir).unwrap();

        // Create a JSON file with underscores (converted from hyphens)
        let json_file = doc_dir.join("test_crate.json");
        fs::write(&json_file, "{}").unwrap();

        let result = docgen.find_json_doc(&doc_dir, "test-crate").unwrap();
        assert_eq!(result, json_file);
    }
}
