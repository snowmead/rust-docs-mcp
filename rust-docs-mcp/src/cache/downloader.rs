//! Crate downloading and source management
//!
//! This module handles downloading crates from various sources including
//! crates.io, GitHub repositories, and local filesystem paths.

use crate::cache::source::{GitReference, SourceDetector, SourceType};
use crate::cache::storage::CacheStorage;
use crate::cache::tools::{
    CacheCrateFromCratesIOParams, CacheCrateFromGitHubParams, CacheCrateFromLocalParams,
};
use crate::cache::utils::copy_directory_contents;
use anyhow::{Context, Result, bail};
use flate2::read::GzDecoder;
use futures::StreamExt;
use git2::Repository;
use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};
use tar::Archive;

/// Unified crate source enum that reuses the parameter structs from tools
#[derive(Debug, Clone)]
pub enum CrateSource {
    CratesIO(CacheCrateFromCratesIOParams),
    GitHub(CacheCrateFromGitHubParams),
    LocalPath(CacheCrateFromLocalParams),
}

/// Service for downloading crates from various sources
#[derive(Debug, Clone)]
pub struct CrateDownloader {
    storage: CacheStorage,
    client: reqwest::Client,
}

impl CrateDownloader {
    /// Create a new crate downloader
    pub fn new(storage: CacheStorage) -> Self {
        Self {
            storage,
            client: reqwest::Client::new(),
        }
    }

    /// Download or copy a crate from the specified source
    pub async fn download_or_copy_crate(
        &self,
        name: &str,
        version: &str,
        source: Option<&str>,
    ) -> Result<PathBuf> {
        let source_type = SourceDetector::detect(source);

        match source_type {
            SourceType::CratesIo => self.download_crate(name, version).await,
            SourceType::GitHub {
                url,
                reference,
                repo_path,
            } => {
                let version_str = match reference {
                    GitReference::Branch(branch) => branch,
                    GitReference::Tag(tag) => tag,
                    GitReference::Default => "main".to_string(),
                };
                self.download_from_github(name, &version_str, &url, repo_path.as_deref())
                    .await
            }
            SourceType::Local { path } => self.copy_from_local(name, version, &path).await,
        }
    }

    /// Download a crate from crates.io
    async fn download_crate(&self, name: &str, version: &str) -> Result<PathBuf> {
        tracing::info!("Downloading crate {}-{} from crates.io", name, version);

        let url = format!("https://crates.io/api/v1/crates/{name}/{version}/download");

        let response = self
            .client
            .get(&url)
            .send()
            .await
            .with_context(|| format!("Failed to download {name}-{version}"))?;

        if !response.status().is_success() {
            bail!(
                "Failed to download {}-{}: HTTP {}",
                name,
                version,
                response.status()
            );
        }

        // Save to a temporary file first
        let temp_file_path = std::env::temp_dir().join(format!("{name}-{version}.tar.gz"));
        let mut temp_file = File::create(&temp_file_path)
            .with_context(|| format!("Failed to create temporary file for {name}-{version}"))?;

        let mut stream = response.bytes_stream();
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.context("Failed to read chunk from download stream")?;
            temp_file
                .write_all(&chunk)
                .context("Failed to write to temporary file")?;
        }

        // Extract the crate
        let source_path = self.storage.source_path(name, version)?;
        self.storage.ensure_dir(&source_path)?;

        let tar_gz = File::open(&temp_file_path).context("Failed to open downloaded file")?;
        let tar = GzDecoder::new(tar_gz);
        let mut archive = Archive::new(tar);

        // Extract with proper path handling
        for entry in archive.entries()? {
            let mut entry = entry?;
            let path = entry.path()?;

            // Skip the top-level directory (crate-version/)
            let components: Vec<_> = path.components().collect();
            if components.len() > 1 {
                let relative_path: PathBuf = components[1..].iter().collect();
                let dest_path = source_path.join(relative_path);

                if let Some(parent) = dest_path.parent() {
                    std::fs::create_dir_all(parent)?;
                }

                entry.unpack(&dest_path)?;
            }
        }

        // Clean up temp file
        std::fs::remove_file(&temp_file_path).ok();

        // Save metadata for the cached crate
        self.storage.save_metadata(name, version)?;

        tracing::info!("Successfully downloaded and extracted {}-{}", name, version);
        Ok(source_path)
    }

    /// Download a crate from GitHub repository
    async fn download_from_github(
        &self,
        name: &str,
        version: &str,
        repo_url: &str,
        repo_path: Option<&str>,
    ) -> Result<PathBuf> {
        tracing::info!(
            "Downloading crate {}-{} from GitHub: {}",
            name,
            version,
            repo_url
        );

        let temp_dir = std::env::temp_dir().join(format!("rust-docs-mcp-git-{name}-{version}"));

        // Clean up any existing temp directory
        if temp_dir.exists() {
            fs::remove_dir_all(&temp_dir).context("Failed to clean temp directory")?;
        }

        // Clone the repository
        let repo = Repository::clone(repo_url, &temp_dir)
            .with_context(|| format!("Failed to clone repository: {repo_url}"))?;

        // Checkout the specific branch or tag (version contains the branch/tag name)
        // The version parameter here is actually the branch or tag name
        if version != "main" && version != "master" {
            // Try to checkout as a branch first
            let refname = format!("refs/remotes/origin/{version}");
            if let Ok(reference) = repo.find_reference(&refname) {
                let oid = reference
                    .target()
                    .ok_or_else(|| anyhow::anyhow!("Reference has no target"))?;
                repo.set_head_detached(oid)
                    .with_context(|| format!("Failed to checkout branch: {version}"))?;
                repo.checkout_head(Some(git2::build::CheckoutBuilder::default().force()))
                    .with_context(|| format!("Failed to checkout branch: {version}"))?;
            } else {
                // Try as a tag
                let tag_ref = format!("refs/tags/{version}");
                if let Ok(reference) = repo.find_reference(&tag_ref) {
                    let oid = reference
                        .target()
                        .ok_or_else(|| anyhow::anyhow!("Reference has no target"))?;
                    repo.set_head_detached(oid)
                        .with_context(|| format!("Failed to checkout tag: {version}"))?;
                    repo.checkout_head(Some(git2::build::CheckoutBuilder::default().force()))
                        .with_context(|| format!("Failed to checkout tag: {version}"))?;
                } else {
                    bail!("Could not find branch or tag: {}", version);
                }
            }
        }

        // Determine source path within the repository
        let repo_source_path = if let Some(path) = repo_path {
            temp_dir.join(path)
        } else {
            temp_dir.clone()
        };

        // Verify Cargo.toml exists
        let cargo_toml = repo_source_path.join("Cargo.toml");
        if !cargo_toml.exists() {
            bail!(
                "No Cargo.toml found at path: {}",
                repo_source_path.display()
            );
        }

        // Copy to cache location
        let source_path = self.storage.source_path(name, version)?;
        self.storage.ensure_dir(&source_path)?;

        copy_directory_contents(&repo_source_path, &source_path)
            .context("Failed to copy repository contents")?;

        // Clean up temp directory
        fs::remove_dir_all(&temp_dir).ok();

        // Save metadata with source information
        let source_info = match repo_path {
            Some(path) => format!("{repo_url}#{path}"),
            None => repo_url.to_string(),
        };
        self.storage
            .save_metadata_with_source(name, version, "github", Some(&source_info))?;

        tracing::info!(
            "Successfully downloaded and extracted {}-{} from GitHub",
            name,
            version
        );
        Ok(source_path)
    }

    /// Copy a crate from local file system
    async fn copy_from_local(
        &self,
        name: &str,
        version: &str,
        local_path: &str,
    ) -> Result<PathBuf> {
        tracing::info!(
            "Copying crate {}-{} from local path: {}",
            name,
            version,
            local_path
        );

        // Expand tilde and other shell expansions
        let expanded_path = shellexpand::full(local_path)
            .with_context(|| format!("Failed to expand path: {local_path}"))?;
        let source_path_input = Path::new(expanded_path.as_ref());

        // Verify the path exists and contains Cargo.toml
        if !source_path_input.exists() {
            bail!("Local path does not exist: {}", source_path_input.display());
        }

        let cargo_toml = source_path_input.join("Cargo.toml");
        if !cargo_toml.exists() {
            bail!(
                "No Cargo.toml found at path: {}",
                source_path_input.display()
            );
        }

        // Copy to cache location
        let source_path = self.storage.source_path(name, version)?;
        self.storage.ensure_dir(&source_path)?;

        copy_directory_contents(source_path_input, &source_path)
            .context("Failed to copy local directory contents")?;

        // Save metadata with source information
        self.storage
            .save_metadata_with_source(name, version, "local", Some(local_path))?;

        tracing::info!("Successfully copied {}-{} from local path", name, version);
        Ok(source_path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_downloader_creation() {
        let temp_dir = TempDir::new().unwrap();
        let storage = CacheStorage::new(Some(temp_dir.path().to_path_buf())).unwrap();
        let downloader = CrateDownloader::new(storage);

        // Just verify it was created successfully
        assert!(format!("{downloader:?}").contains("CrateDownloader"));
    }
}
