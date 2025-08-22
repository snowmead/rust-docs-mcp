//! Crate downloading and source management
//!
//! This module handles downloading crates from various sources including
//! crates.io, GitHub repositories, and local filesystem paths.

use crate::cache::constants::*;
use crate::cache::source::{GitReference, SourceDetector, SourceType};
use crate::cache::storage::CacheStorage;
use crate::cache::tools::{
    CacheCrateFromCratesIOParams, CacheCrateFromGitHubParams, CacheCrateFromLocalParams,
};
use crate::cache::utils::copy_directory_contents;
use anyhow::{Context, Result, bail};
use flate2::read::GzDecoder;
use futures::StreamExt;
use git2::{Cred, FetchOptions, RemoteCallbacks};
use std::env;
use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};
use tar::Archive;

/// Constants for download operations
const LOCK_TIMEOUT_SECS: u64 = 60;
const LOCK_POLL_INTERVAL_MS: u64 = 100;

/// RAII guard for cleaning up lock files
struct LockGuard {
    path: PathBuf,
}

impl Drop for LockGuard {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

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
        let client = Self::build_http_client();
        Self { storage, client }
    }

    /// Build the HTTP client with proper configuration
    fn build_http_client() -> reqwest::Client {
        let user_agent = Self::format_user_agent();

        tracing::info!("Creating HTTP client with User-Agent: {}", user_agent);

        reqwest::Client::builder()
            .user_agent(user_agent)
            .redirect(reqwest::redirect::Policy::limited(10))
            .build()
            .expect("Failed to create HTTP client") // HTTP client creation should not fail with proper configuration
    }

    /// Format the user-agent string for API compliance
    fn format_user_agent() -> String {
        format!(
            "{}/{} ({})",
            env!("CARGO_PKG_NAME"),
            env!("CARGO_PKG_VERSION"),
            env!("CARGO_PKG_REPOSITORY")
        )
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
        // Check if already cached
        if self.storage.is_cached(name, version) {
            tracing::info!("Crate {}-{} already cached", name, version);
            return self.storage.source_path(name, version);
        }

        // Create a lock file to prevent concurrent downloads
        let crate_path = self.storage.crate_path(name, version)?;
        let lock_path = crate_path.with_extension("lock");

        // Check if another process is already downloading
        if lock_path.exists() {
            tracing::info!(
                "Another process is downloading {}-{}, waiting...",
                name,
                version
            );
            // Wait for the other process to finish (simple polling)
            let start = std::time::Instant::now();
            while lock_path.exists()
                && start.elapsed() < std::time::Duration::from_secs(LOCK_TIMEOUT_SECS)
            {
                tokio::time::sleep(std::time::Duration::from_millis(LOCK_POLL_INTERVAL_MS)).await;
            }

            // Check if it was successfully cached by the other process
            if self.storage.is_cached(name, version) {
                tracing::info!("Crate {}-{} was cached by another process", name, version);
                return self.storage.source_path(name, version);
            }
        }

        // Create lock file
        if let Some(parent) = lock_path.parent() {
            self.storage.ensure_dir(parent)?;
        }
        std::fs::write(&lock_path, "downloading").context("Failed to create lock file")?;

        // Ensure lock file is removed on exit
        let _lock_guard = LockGuard {
            path: lock_path.clone(),
        };

        tracing::info!(
            "Starting fresh download of {}-{} from crates.io",
            name,
            version
        );

        let url = format!("https://crates.io/api/v1/crates/{name}/{version}/download");
        tracing::debug!("Download URL: {}", url);

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

        // Save to a temporary file first - make path unique to avoid concurrent conflicts
        let temp_file_path = std::env::temp_dir().join(format!(
            "{name}-{version}-{}-{}.tar.gz",
            std::process::id(),
            uuid::Uuid::new_v4().simple()
        ));
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

                // Validate that the path doesn't escape the destination directory
                // Check for path traversal attempts
                let has_parent_refs = relative_path
                    .components()
                    .any(|c| matches!(c, std::path::Component::ParentDir));

                if has_parent_refs {
                    tracing::warn!(
                        "Skipping entry with parent directory reference: {}",
                        path.display()
                    );
                    continue;
                }

                let dest_path = source_path.join(&relative_path);

                // Additional validation: ensure the destination is within source_path
                let canonical_source = source_path
                    .canonicalize()
                    .unwrap_or_else(|_| source_path.clone());

                if let Ok(canonical_dest) = dest_path.canonicalize() {
                    if !canonical_dest.starts_with(&canonical_source) {
                        tracing::warn!(
                            "Skipping entry that would escape destination: {}",
                            path.display()
                        );
                        continue;
                    }
                } else if let Some(parent) = dest_path.parent() {
                    // For files that don't exist yet, check the parent
                    if matches!(parent.canonicalize(), Ok(canonical_parent) if !canonical_parent.starts_with(&canonical_source)) {
                        tracing::warn!(
                            "Skipping entry with parent outside destination: {}",
                            path.display()
                        );
                        continue;
                    }
                }

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
        // Check if already cached
        if self.storage.is_cached(name, version) {
            tracing::info!("Crate {}-{} already cached", name, version);
            return self.storage.source_path(name, version);
        }

        // Create a lock file to prevent concurrent downloads
        let crate_path = self.storage.crate_path(name, version)?;
        let lock_path = crate_path.with_extension("lock");

        // Check if another process is already downloading
        if lock_path.exists() {
            tracing::info!(
                "Another process is downloading {}-{}, waiting...",
                name,
                version
            );
            // Wait for the other process to finish (simple polling)
            let start = std::time::Instant::now();
            while lock_path.exists()
                && start.elapsed() < std::time::Duration::from_secs(LOCK_TIMEOUT_SECS)
            {
                tokio::time::sleep(std::time::Duration::from_millis(LOCK_POLL_INTERVAL_MS)).await;
            }

            // Check if it was successfully cached by the other process
            if self.storage.is_cached(name, version) {
                tracing::info!("Crate {}-{} was cached by another process", name, version);
                return self.storage.source_path(name, version);
            }
        }

        // Create lock file
        if let Some(parent) = lock_path.parent() {
            self.storage.ensure_dir(parent)?;
        }
        std::fs::write(&lock_path, "downloading").context("Failed to create lock file")?;

        // Ensure lock file is removed on exit
        let _lock_guard = LockGuard {
            path: lock_path.clone(),
        };

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

        // Set up GitHub authentication if token is available
        let github_token = env::var("GITHUB_TOKEN").ok();

        // Configure git authentication callbacks
        let mut fetch_options = FetchOptions::new();
        let mut callbacks = RemoteCallbacks::new();

        if let Some(token) = &github_token {
            tracing::debug!("Using GITHUB_TOKEN for authentication");
            callbacks.credentials(move |_url, username_from_url, _allowed_types| {
                Cred::userpass_plaintext(username_from_url.unwrap_or("git"), token)
            });
        } else {
            tracing::debug!("No GITHUB_TOKEN found, using unauthenticated access");
        }

        fetch_options.remote_callbacks(callbacks);

        // Clone the repository with authentication
        let mut builder = git2::build::RepoBuilder::new();
        builder.fetch_options(fetch_options);

        let repo = builder
            .clone(repo_url, &temp_dir)
            .with_context(|| {
                let mut msg = format!("Failed to clone repository: {repo_url}");
                if github_token.is_none() && repo_url.contains("github.com") {
                    msg.push_str("\nNote: Set GITHUB_TOKEN environment variable for private repositories and higher rate limits");
                }
                msg
            })?;

        // Checkout the specific branch or tag (version contains the branch/tag name)
        // The version parameter here is actually the branch or tag name
        if version != "main" && version != "master" {
            // Validate git reference name to prevent potential issues
            if !Self::is_valid_git_ref(version) {
                bail!("Invalid git reference name: {}", version);
            }

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
        let cargo_toml = repo_source_path.join(CARGO_TOML);
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
        self.storage.save_metadata_with_source(
            name,
            version,
            "github",
            Some(&source_info),
            None,
        )?;

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

        let cargo_toml = source_path_input.join(CARGO_TOML);
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
            .save_metadata_with_source(name, version, "local", Some(local_path), None)?;

        tracing::info!("Successfully copied {}-{} from local path", name, version);
        Ok(source_path)
    }

    /// Validate git reference name to prevent potential issues
    fn is_valid_git_ref(ref_name: &str) -> bool {
        // Git references must not:
        // - Be empty
        // - Contain ".." (directory traversal)
        // - Start or end with dots or slashes
        // - Contain control characters or spaces
        // - Contain characters that could be problematic in shell contexts

        if ref_name.is_empty() || ref_name.contains("..") {
            return false;
        }

        if ref_name.starts_with('.')
            || ref_name.ends_with('.')
            || ref_name.starts_with('/')
            || ref_name.ends_with('/')
        {
            return false;
        }

        // Allow alphanumeric, dots, slashes, hyphens, underscores
        // Common for tags like "v1.0.0" or branches like "feature/new-thing"
        ref_name.chars().all(|c| {
            c.is_alphanumeric() || c == '-' || c == '_' || c == '.' || c == '/' || c == '+' // Allow for version tags like "1.0.0+20240621"
        })
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

    #[tokio::test]
    async fn test_user_agent_set() {
        // Initialize logging for the test
        let _ = tracing_subscriber::fmt()
            .with_env_filter("rust_docs_mcp=debug")
            .try_init();

        // Create a temporary directory for testing
        let temp_dir = TempDir::new().unwrap();
        let storage = CacheStorage::new(Some(temp_dir.path().to_path_buf())).unwrap();

        // Create downloader
        let downloader = CrateDownloader::new(storage);

        // Test that download doesn't fail with 403
        // Note: This is an integration test that requires internet access
        match downloader.download_crate("serde", "1.0.0").await {
            Ok(path) => {
                assert!(path.exists());
                println!("Successfully downloaded crate to: {path:?}");
            }
            Err(e) => {
                // If it fails, it should not be a 403 error
                let error_msg = format!("{e}");
                assert!(!error_msg.contains("403"), "Got 403 error: {error_msg}");
            }
        }
    }

    #[tokio::test]
    async fn test_problematic_crate_download() {
        // Initialize logging for the test
        let _ = tracing_subscriber::fmt()
            .with_env_filter("rust_docs_mcp=debug")
            .try_init();

        // Test downloading the specific crate that was failing
        let temp_dir = TempDir::new().unwrap();
        let storage = CacheStorage::new(Some(temp_dir.path().to_path_buf())).unwrap();
        let downloader = CrateDownloader::new(storage);

        match downloader
            .download_crate("google-sheets4", "6.0.0+20240621")
            .await
        {
            Ok(path) => {
                assert!(path.exists());
                println!(
                    "Successfully downloaded google-sheets4-6.0.0+20240621 to: {path:?}"
                );
            }
            Err(e) => {
                panic!("Failed to download google-sheets4: {e}");
            }
        }
    }
}
