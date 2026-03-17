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
use std::sync::Arc;
use tar::Archive;
use zeroize::Zeroizing;

/// Progress callback function type for reporting download/operation progress (0-100)
pub type ProgressCallback = Arc<dyn Fn(u8) + Send + Sync>;

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

    /// Resolve a potentially partial version (e.g. "9" or "9.3") to the latest
    /// matching exact version on crates.io. If the version already contains two
    /// dots (looks like full semver), it is returned as-is.
    pub async fn resolve_crates_io_version(
        &self,
        name: &str,
        version: &str,
    ) -> Result<String> {
        // If it already looks like a full semver version (has two dots), return as-is
        if version.matches('.').count() >= 2 {
            return Ok(version.to_string());
        }

        let url = format!("https://crates.io/api/v1/crates/{name}");
        tracing::debug!("Resolving version for {name} prefix={version} via {url}");

        let response = self
            .client
            .get(&url)
            .send()
            .await
            .with_context(|| format!("Failed to query crates.io for {name}"))?;

        if !response.status().is_success() {
            bail!(
                "Failed to look up crate {name} on crates.io: HTTP {}",
                response.status()
            );
        }

        let body: serde_json::Value = response
            .json()
            .await
            .context("Failed to parse crates.io response")?;

        let versions = body["versions"]
            .as_array()
            .context("Unexpected crates.io response format")?;

        // Find the latest non-yanked version that starts with the given prefix
        let prefix_dot = format!("{version}.");
        let matching_version = versions
            .iter()
            .filter_map(|v| {
                let num = v["num"].as_str()?;
                let yanked = v["yanked"].as_bool().unwrap_or(false);
                if yanked {
                    return None;
                }
                // Match if version equals the prefix exactly or starts with "prefix."
                if num == version || num.starts_with(&prefix_dot) {
                    Some(num.to_string())
                } else {
                    None
                }
            })
            .next(); // crates.io returns versions newest-first

        match matching_version {
            Some(resolved) => {
                tracing::info!("Resolved {name} version {version} → {resolved}");
                Ok(resolved)
            }
            None => bail!(
                "No matching version found for {name} with prefix '{version}'. \
                 Try specifying an exact version (e.g. '9.3.1')."
            ),
        }
    }

    /// Download or copy a crate from the specified source
    pub async fn download_or_copy_crate(
        &self,
        name: &str,
        version: &str,
        source: Option<&str>,
        progress_callback: Option<ProgressCallback>,
    ) -> Result<PathBuf> {
        let source_type = SourceDetector::detect(source);

        match source_type {
            SourceType::CratesIo => self.download_crate(name, version, progress_callback).await,
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
    async fn download_crate(
        &self,
        name: &str,
        version: &str,
        progress_callback: Option<ProgressCallback>,
    ) -> Result<PathBuf> {
        // Check if already cached
        if self.storage.is_cached(name, version) {
            tracing::info!("Crate {}-{} already cached", name, version);
            if let Some(callback) = progress_callback {
                callback(100); // Already cached = 100% complete
            }
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

        // Try to reuse an already-downloaded crate from Cargo's on-disk
        // registry cache. This is an optimization — any failure (e.g., a
        // permission-denied $CARGO_HOME, a mid-copy I/O error) should log a
        // warning and fall through to the regular HTTP download path rather
        // than aborting the entire operation.
        match self.try_copy_from_cargo_registry(name, version) {
            Ok(Some(source_path)) => {
                tracing::info!(
                    "Reused Cargo registry source for {}-{} at {}",
                    name,
                    version,
                    source_path.display()
                );
                if let Some(callback) = progress_callback {
                    callback(100);
                }
                return Ok(source_path);
            }
            Ok(None) => {}
            Err(e) => {
                tracing::warn!(
                    "Failed to reuse Cargo registry source for {}-{}: {:#}. Falling back to HTTP download.",
                    name,
                    version,
                    e
                );
            }
        }

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

        // Track download progress
        let total_bytes = response.content_length().unwrap_or(0);
        let mut downloaded_bytes = 0u64;

        let mut stream = response.bytes_stream();
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.context("Failed to read chunk from download stream")?;
            downloaded_bytes += chunk.len() as u64;

            temp_file
                .write_all(&chunk)
                .context("Failed to write to temporary file")?;

            // Report progress if callback provided and total size known
            if let Some(ref callback) = progress_callback
                && total_bytes > 0
            {
                let percent = ((downloaded_bytes * 100) / total_bytes).min(100) as u8;
                callback(percent);
            }
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
                    if matches!(parent.canonicalize(), Ok(canonical_parent) if !canonical_parent.starts_with(&canonical_source))
                    {
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
        let github_token = env::var("GITHUB_TOKEN").ok().map(Zeroizing::new);
        let has_token = github_token.is_some();

        // Configure git authentication callbacks
        let mut fetch_options = FetchOptions::new();
        let mut callbacks = RemoteCallbacks::new();

        if let Some(token) = github_token {
            tracing::debug!("Using GITHUB_TOKEN for authentication");
            callbacks.credentials(move |_url, username_from_url, _allowed_types| {
                Cred::userpass_plaintext(username_from_url.unwrap_or("git"), &token)
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
                if !has_token && repo_url.contains("github.com") {
                    msg.push_str("\nNote: Set GITHUB_TOKEN environment variable for private repositories and higher rate limits");
                }
                msg
            })?;

        // Checkout the specific branch or tag (version contains the branch/tag name)
        // The version parameter here is actually the branch or tag name
        if version != "main" && version != "master" {
            // Validate git reference name to prevent potential issues
            if !Self::is_valid_git_ref(version) {
                bail!("Invalid git reference name: {version}");
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
                    bail!("Could not find branch or tag: {version}");
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

    fn try_copy_from_cargo_registry(&self, name: &str, version: &str) -> Result<Option<PathBuf>> {
        let Some(cargo_source_path) = self.find_cargo_registry_source(name, version)? else {
            return Ok(None);
        };

        tracing::info!(
            "Found Cargo registry source for {}-{} at {}",
            name,
            version,
            cargo_source_path.display()
        );

        let source_path = self.storage.source_path(name, version)?;

        // Populate the cache from the registry source. If any step fails we
        // roll back the partially-written source directory so the caller can
        // safely retry via HTTP without mixing old and new files.
        let populate = (|| -> Result<()> {
            self.storage.ensure_dir(&source_path)?;
            copy_directory_contents(&cargo_source_path, &source_path)
                .context("Failed to copy Cargo registry source into cache")?;
            self.storage.save_metadata_with_source(
                name,
                version,
                "crates.io",
                Some(cargo_source_path.to_string_lossy().as_ref()),
                None,
            )?;
            Ok(())
        })();

        if let Err(e) = populate {
            if source_path.exists()
                && let Err(cleanup_err) = fs::remove_dir_all(&source_path)
            {
                tracing::warn!(
                    "Failed to clean up partial Cargo registry copy at {}: {}",
                    source_path.display(),
                    cleanup_err
                );
            }
            return Err(e);
        }

        Ok(Some(source_path))
    }

    /// Look up a crate in Cargo's on-disk registry source cache.
    ///
    /// Cargo lays out unpacked crate sources under
    /// `$CARGO_HOME/registry/src/<index>-<hash>/<name>-<version>/`, where
    /// `<index>-<hash>` is derived from the registry URL (e.g.
    /// `index.crates.io-6f17d22bba15001f`). Because users may have multiple
    /// registries configured, we iterate every `<index>-<hash>` subdirectory
    /// and return the first one that contains a `<name>-<version>/Cargo.toml`.
    /// The `Cargo.toml` probe is what validates a candidate — unrelated
    /// directories are simply skipped rather than treated as errors.
    fn find_cargo_registry_source(&self, name: &str, version: &str) -> Result<Option<PathBuf>> {
        let Some(cargo_home) = Self::cargo_home() else {
            tracing::debug!("Cargo home not found while checking local registry cache");
            return Ok(None);
        };

        let registry_src_root = cargo_home.join("registry").join("src");
        if !registry_src_root.is_dir() {
            tracing::debug!(
                "Cargo registry source directory does not exist: {}",
                registry_src_root.display()
            );
            return Ok(None);
        }

        let crate_dir_name = format!("{name}-{version}");
        for registry_entry in fs::read_dir(&registry_src_root).with_context(|| {
            format!(
                "Failed to read Cargo registry source directory: {}",
                registry_src_root.display()
            )
        })? {
            let registry_entry = registry_entry?;
            if !registry_entry.file_type()?.is_dir() {
                continue;
            }

            let candidate = registry_entry.path().join(&crate_dir_name);
            if candidate.join(CARGO_TOML).is_file() {
                return Ok(Some(candidate));
            }
        }

        Ok(None)
    }

    fn cargo_home() -> Option<PathBuf> {
        env::var_os("CARGO_HOME")
            .map(PathBuf::from)
            .or_else(|| dirs::home_dir().map(|home| home.join(".cargo")))
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
    use std::sync::{Mutex, OnceLock};
    use tempfile::TempDir;

    /// Serializes tests that mutate the process-global `CARGO_HOME` env var.
    /// Rust's test harness runs tests concurrently by default, so without this
    /// lock two tests could race on `set_var`/`remove_var` and observe each
    /// other's state — causing intermittent failures where one test sees the
    /// other's temp directory as its `CARGO_HOME`.
    fn cargo_home_lock() -> &'static Mutex<()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
    }

    fn set_cargo_home(path: &Path) {
        unsafe {
            env::set_var("CARGO_HOME", path);
        }
    }

    fn remove_cargo_home() {
        unsafe {
            env::remove_var("CARGO_HOME");
        }
    }

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
        match downloader.download_crate("serde", "1.0.0", None).await {
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
            .download_crate("google-sheets4", "6.0.0+20240621", None)
            .await
        {
            Ok(path) => {
                assert!(path.exists());
                println!("Successfully downloaded google-sheets4-6.0.0+20240621 to: {path:?}");
            }
            Err(e) => {
                panic!("Failed to download google-sheets4: {e}");
            }
        }
    }

    #[test]
    fn test_find_cargo_registry_source_uses_configured_cargo_home() {
        let _guard = cargo_home_lock().lock().unwrap();

        let cargo_home_dir = TempDir::new().unwrap();
        let storage_dir = TempDir::new().unwrap();
        // Build the path one component at a time so the separator is
        // platform-native on Windows (otherwise the embedded `/` ends up
        // preserved verbatim and breaks string-based path comparisons).
        let cached_source = cargo_home_dir
            .path()
            .join("registry")
            .join("src")
            .join("index.crates.io-test")
            .join("cached-crate-9.9.9");
        fs::create_dir_all(cached_source.join("src")).unwrap();
        fs::write(
            cached_source.join(CARGO_TOML),
            "[package]\nname = \"cached-crate\"\nversion = \"9.9.9\"\nedition = \"2021\"\n",
        )
        .unwrap();
        fs::write(
            cached_source.join("src/lib.rs"),
            "pub fn answer() -> u32 { 42 }\n",
        )
        .unwrap();

        set_cargo_home(cargo_home_dir.path());

        let storage = CacheStorage::new(Some(storage_dir.path().to_path_buf())).unwrap();
        let downloader = CrateDownloader::new(storage);
        let found = downloader
            .find_cargo_registry_source("cached-crate", "9.9.9")
            .unwrap();

        remove_cargo_home();

        assert_eq!(found, Some(cached_source));
    }

    // The guard is intentionally held across the `.await` below to serialize
    // `CARGO_HOME` mutation with the sync test above — no other async task
    // contends for this lock.
    #[allow(clippy::await_holding_lock)]
    #[tokio::test]
    async fn test_download_crate_reuses_cargo_registry_source() {
        let _guard = cargo_home_lock().lock().unwrap();

        let cargo_home_dir = TempDir::new().unwrap();
        let storage_dir = TempDir::new().unwrap();
        // Build the path one component at a time so the separator is
        // platform-native on Windows (otherwise the embedded `/` ends up
        // preserved verbatim and breaks string-based path comparisons).
        let cached_source = cargo_home_dir
            .path()
            .join("registry")
            .join("src")
            .join("index.crates.io-test")
            .join("cached-crate-9.9.9");
        fs::create_dir_all(cached_source.join("src")).unwrap();
        fs::write(
            cached_source.join(CARGO_TOML),
            "[package]\nname = \"cached-crate\"\nversion = \"9.9.9\"\nedition = \"2021\"\n",
        )
        .unwrap();
        fs::write(
            cached_source.join("src/lib.rs"),
            "pub fn answer() -> u32 { 42 }\n",
        )
        .unwrap();

        set_cargo_home(cargo_home_dir.path());

        let storage = CacheStorage::new(Some(storage_dir.path().to_path_buf())).unwrap();
        let downloader = CrateDownloader::new(storage.clone());

        let cached_path = downloader
            .download_crate("cached-crate", "9.9.9", None)
            .await
            .unwrap();

        remove_cargo_home();

        assert_eq!(
            cached_path,
            storage.source_path("cached-crate", "9.9.9").unwrap()
        );
        assert!(cached_path.join(CARGO_TOML).exists());
        assert!(cached_path.join("src/lib.rs").exists());

        let metadata = storage
            .load_metadata("cached-crate", "9.9.9", None)
            .unwrap();
        assert_eq!(metadata.source, "crates.io");
        assert_eq!(
            metadata.source_path,
            Some(cached_source.to_string_lossy().to_string())
        );
    }

    #[test]
    fn test_try_copy_from_cargo_registry_returns_none_when_crate_absent() {
        let _guard = cargo_home_lock().lock().unwrap();

        let cargo_home_dir = TempDir::new().unwrap();
        let storage_dir = TempDir::new().unwrap();

        // Registry index directory exists but contains no crate subdirectories.
        fs::create_dir_all(
            cargo_home_dir
                .path()
                .join("registry")
                .join("src")
                .join("index.crates.io-test"),
        )
        .unwrap();

        set_cargo_home(cargo_home_dir.path());

        let storage = CacheStorage::new(Some(storage_dir.path().to_path_buf())).unwrap();
        let downloader = CrateDownloader::new(storage.clone());

        let result = downloader
            .try_copy_from_cargo_registry("cached-crate", "9.9.9")
            .unwrap();

        remove_cargo_home();

        assert_eq!(result, None);
        assert!(
            !storage
                .source_path("cached-crate", "9.9.9")
                .unwrap()
                .exists()
        );
        assert!(
            !storage
                .metadata_path("cached-crate", "9.9.9", None)
                .unwrap()
                .exists()
        );
    }

    #[test]
    fn test_try_copy_from_cargo_registry_rolls_back_partial_copy() {
        let _guard = cargo_home_lock().lock().unwrap();

        let cargo_home_dir = TempDir::new().unwrap();
        let storage_dir = TempDir::new().unwrap();

        // Pre-populate a valid cargo registry source.
        let cached_source = cargo_home_dir
            .path()
            .join("registry")
            .join("src")
            .join("index.crates.io-test")
            .join("cached-crate-9.9.9");
        fs::create_dir_all(cached_source.join("src")).unwrap();
        fs::write(
            cached_source.join(CARGO_TOML),
            "[package]\nname = \"cached-crate\"\nversion = \"9.9.9\"\nedition = \"2021\"\n",
        )
        .unwrap();
        fs::write(
            cached_source.join("src").join("lib.rs"),
            "pub fn answer() -> u32 { 42 }\n",
        )
        .unwrap();

        set_cargo_home(cargo_home_dir.path());

        let storage = CacheStorage::new(Some(storage_dir.path().to_path_buf())).unwrap();

        // Force `copy_directory_contents` to fail mid-walk by planting an empty
        // directory at the destination path where `src/lib.rs` would be copied.
        // `fs::copy` opens the destination via `File::create`, which errors out
        // on a directory (EISDIR on Unix; access-denied on Windows). This
        // happens after `ensure_dir(&source_path)` succeeds and potentially
        // after `Cargo.toml` is already copied — so the rollback branch must
        // clean up genuinely partial state.
        let source_path = storage.source_path("cached-crate", "9.9.9").unwrap();
        let planted_lib_rs = source_path.join("src").join("lib.rs");
        fs::create_dir_all(&planted_lib_rs).unwrap();

        let downloader = CrateDownloader::new(storage.clone());

        let result = downloader.try_copy_from_cargo_registry("cached-crate", "9.9.9");

        remove_cargo_home();

        // We only assert the error shape, never the error text — the underlying
        // OS error code differs across platforms.
        assert!(result.is_err(), "expected Err from try_copy, got Ok");
        assert!(
            !source_path.exists(),
            "rollback should have removed the partially-copied source directory"
        );
        // Sanity check: the cargo registry source was not touched by the rollback.
        assert!(
            cached_source.join("src").join("lib.rs").is_file(),
            "rollback must not reach outside storage's source_path"
        );
    }
}
