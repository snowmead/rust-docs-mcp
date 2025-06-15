use crate::cache::storage::CacheStorage;
use anyhow::{Context, Result, bail};
use flate2::read::GzDecoder;
use futures::StreamExt;
use std::fs::File;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;
use tar::Archive;
use git2::Repository;
use std::fs;

/// Different source types for crates
#[derive(Debug, Clone, PartialEq)]
enum SourceType {
    CratesIo,
    GitHub { url: String, repo_path: Option<String> },
    Local { path: String },
}

/// Service for managing crate caching and documentation generation
#[derive(Debug, Clone)]
pub struct CrateCache {
    storage: CacheStorage,
    client: reqwest::Client,
}

impl CrateCache {
    /// Create a new crate cache instance
    pub fn new(cache_dir: Option<PathBuf>) -> Result<Self> {
        Ok(Self {
            storage: CacheStorage::new(cache_dir)?,
            client: reqwest::Client::new(),
        })
    }

    /// Detect the source type from a source string
    fn detect_source_type(&self, source: Option<&str>) -> SourceType {
        match source {
            None => SourceType::CratesIo,
            Some(s) => {
                if s.starts_with("http://") || s.starts_with("https://") {
                    self.parse_github_url(s)
                } else if s.starts_with('/') || s.starts_with("~/") || s.starts_with("../") || s.starts_with("./") {
                    SourceType::Local { path: s.to_string() }
                } else {
                    // If it contains path separators, treat as local path
                    if s.contains('/') || s.contains('\\') {
                        SourceType::Local { path: s.to_string() }
                    } else {
                        SourceType::CratesIo
                    }
                }
            }
        }
    }

    /// Parse a GitHub URL and extract repository information
    fn parse_github_url(&self, url: &str) -> SourceType {
        // Handle GitHub URLs like:
        // https://github.com/user/repo
        // https://github.com/user/repo/tree/branch/path/to/crate
        
        if let Some(github_part) = url.strip_prefix("https://github.com/") {
            let parts: Vec<&str> = github_part.split('/').collect();
            if parts.len() >= 2 {
                let base_url = format!("https://github.com/{}/{}", parts[0], parts[1]);
                
                // Check if there's a path specification (tree/branch/path)
                if parts.len() > 4 && parts[2] == "tree" {
                    // Skip "tree" and branch name, take the rest as path
                    let repo_path = parts[4..].join("/");
                    SourceType::GitHub { 
                        url: base_url, 
                        repo_path: Some(repo_path) 
                    }
                } else {
                    SourceType::GitHub { 
                        url: base_url, 
                        repo_path: None 
                    }
                }
            } else {
                // Invalid GitHub URL, treat as local path
                SourceType::Local { path: url.to_string() }
            }
        } else if url.starts_with("http://github.com/") {
            // Convert http to https and recurse
            let https_url = url.replace("http://", "https://");
            self.parse_github_url(&https_url)
        } else {
            // Not a GitHub URL, treat as local path
            SourceType::Local { path: url.to_string() }
        }
    }

    /// Ensure a crate's documentation is available, downloading and generating if necessary
    pub async fn ensure_crate_docs(
        &self,
        name: &str,
        version: &str,
        source: Option<&str>,
    ) -> Result<rustdoc_types::Crate> {
        // Check if docs already exist
        if self.storage.has_docs(name, version) {
            return self.load_docs(name, version).await;
        }

        // Check if crate is downloaded but docs not generated
        if !self.storage.is_cached(name, version) {
            self.download_or_copy_crate(name, version, source).await?;
        }

        // Generate documentation
        self.generate_docs(name, version).await?;

        // Load and return the generated docs
        self.load_docs(name, version).await
    }

    /// Download or copy a crate based on source type
    async fn download_or_copy_crate(&self, name: &str, version: &str, source: Option<&str>) -> Result<PathBuf> {
        let source_type = self.detect_source_type(source);
        
        match source_type {
            SourceType::CratesIo => {
                self.download_crate(name, version).await
            }
            SourceType::GitHub { url, repo_path } => {
                self.download_from_github(name, version, &url, repo_path.as_deref()).await
            }
            SourceType::Local { path } => {
                self.copy_from_local(name, version, &path).await
            }
        }
    }

    /// Download a crate from crates.io
    pub async fn download_crate(&self, name: &str, version: &str) -> Result<PathBuf> {
        let url = format!(
            "https://crates.io/api/v1/crates/{}/{}/download",
            name, version
        );

        tracing::info!("Downloading crate {}-{} from {}", name, version, url);

        // Create response - don't use Accept: application/json as it returns JSON instead of redirect
        let response = self
            .client
            .get(&url)
            .header(
                "User-Agent",
                format!("{}/{}", env!("CARGO_PKG_NAME"), env!("CARGO_PKG_VERSION")),
            )
            .send()
            .await
            .context("Failed to download crate")?;

        if !response.status().is_success() {
            bail!(
                "Failed to download crate: HTTP {} - {}",
                response.status(),
                response
                    .status()
                    .canonical_reason()
                    .unwrap_or("Unknown error")
            );
        }

        // Create temporary file for download
        let temp_dir = std::env::temp_dir();
        let temp_file_path = temp_dir.join(format!("{}-{}.tar.gz", name, version));
        let mut temp_file =
            File::create(&temp_file_path).context("Failed to create temporary file")?;

        // Stream download to file
        let mut stream = response.bytes_stream();
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.context("Failed to read download chunk")?;
            temp_file
                .write_all(&chunk)
                .context("Failed to write to temporary file")?;
        }

        // Extract the crate
        let source_path = self.storage.source_path(name, version);
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
    async fn download_from_github(&self, name: &str, version: &str, repo_url: &str, repo_path: Option<&str>) -> Result<PathBuf> {
        tracing::info!("Downloading crate {}-{} from GitHub: {}", name, version, repo_url);

        let temp_dir = std::env::temp_dir().join(format!("rust-docs-mcp-git-{}-{}", name, version));
        
        // Clean up any existing temp directory
        if temp_dir.exists() {
            fs::remove_dir_all(&temp_dir).context("Failed to clean temp directory")?;
        }

        // Clone the repository
        let _repo = Repository::clone(repo_url, &temp_dir)
            .with_context(|| format!("Failed to clone repository: {}", repo_url))?;

        // Determine source path within the repository
        let repo_source_path = if let Some(path) = repo_path {
            temp_dir.join(path)
        } else {
            temp_dir.clone()
        };

        // Verify Cargo.toml exists
        let cargo_toml = repo_source_path.join("Cargo.toml");
        if !cargo_toml.exists() {
            bail!("No Cargo.toml found at path: {}", repo_source_path.display());
        }

        // Copy to cache location
        let source_path = self.storage.source_path(name, version);
        self.storage.ensure_dir(&source_path)?;

        self.copy_directory_contents(&repo_source_path, &source_path)
            .context("Failed to copy repository contents")?;

        // Clean up temp directory
        fs::remove_dir_all(&temp_dir).ok();

        // Save metadata with source information
        let source_info = match repo_path {
            Some(path) => format!("{}#{}", repo_url, path),
            None => repo_url.to_string(),
        };
        self.storage.save_metadata_with_source(name, version, "github", Some(&source_info))?;

        tracing::info!("Successfully downloaded and extracted {}-{} from GitHub", name, version);
        Ok(source_path)
    }

    /// Copy a crate from local file system
    async fn copy_from_local(&self, name: &str, version: &str, local_path: &str) -> Result<PathBuf> {
        tracing::info!("Copying crate {}-{} from local path: {}", name, version, local_path);

        // Expand tilde and other shell expansions
        let expanded_path = shellexpand::full(local_path)
            .with_context(|| format!("Failed to expand path: {}", local_path))?;
        let source_path_input = Path::new(expanded_path.as_ref());

        // Verify the path exists and contains Cargo.toml
        if !source_path_input.exists() {
            bail!("Local path does not exist: {}", source_path_input.display());
        }

        let cargo_toml = source_path_input.join("Cargo.toml");
        if !cargo_toml.exists() {
            bail!("No Cargo.toml found at path: {}", source_path_input.display());
        }

        // Copy to cache location
        let source_path = self.storage.source_path(name, version);
        self.storage.ensure_dir(&source_path)?;

        self.copy_directory_contents(source_path_input, &source_path)
            .context("Failed to copy local directory contents")?;

        // Save metadata with source information
        self.storage.save_metadata_with_source(name, version, "local", Some(local_path))?;

        tracing::info!("Successfully copied {}-{} from local path", name, version);
        Ok(source_path)
    }

    /// Recursively copy directory contents
    fn copy_directory_contents(&self, src: &Path, dest: &Path) -> Result<()> {
        if !dest.exists() {
            fs::create_dir_all(dest)?;
        }

        for entry in fs::read_dir(src)? {
            let entry = entry?;
            let path = entry.path();
            let name = entry.file_name();
            let dest_path = dest.join(&name);

            if path.is_dir() {
                // Skip .git directories and other version control
                if name == ".git" || name == ".svn" || name == ".hg" {
                    continue;
                }
                self.copy_directory_contents(&path, &dest_path)?;
            } else {
                fs::copy(&path, &dest_path)
                    .with_context(|| format!("Failed to copy file from {} to {}", path.display(), dest_path.display()))?;
            }
        }

        Ok(())
    }

    /// Generate JSON documentation for a crate
    pub async fn generate_docs(&self, name: &str, version: &str) -> Result<PathBuf> {
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

        // Run cargo rustdoc with JSON output
        let output = Command::new("cargo")
            .args(&[
                "+nightly",
                "rustdoc",
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

    /// Find the JSON documentation file in the doc directory
    fn find_json_doc(&self, doc_dir: &Path, crate_name: &str) -> Result<PathBuf> {
        // The JSON file is usually named after the crate with underscores
        let normalized_name = crate_name.replace("-", "_");
        let json_path = doc_dir.join(format!("{}.json", normalized_name));

        if json_path.exists() {
            return Ok(json_path);
        }

        // If not found, search for any JSON file
        for entry in std::fs::read_dir(doc_dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.extension().and_then(|s| s.to_str()) == Some("json") {
                return Ok(path);
            }
        }

        bail!("No JSON documentation file found in {:?}", doc_dir);
    }

    /// Load documentation from cache
    pub async fn load_docs(&self, name: &str, version: &str) -> Result<rustdoc_types::Crate> {
        let docs_path = self.storage.docs_path(name, version);

        if !docs_path.exists() {
            bail!("Documentation not found for {}-{}", name, version);
        }

        let json_string = tokio::fs::read_to_string(&docs_path)
            .await
            .context("Failed to read documentation file")?;

        let crate_docs: rustdoc_types::Crate =
            serde_json::from_str(&json_string).context("Failed to parse documentation JSON")?;

        Ok(crate_docs)
    }

    /// Get cached versions of a crate
    pub async fn get_cached_versions(&self, name: &str) -> Result<Vec<String>> {
        let cached = self.storage.list_cached_crates()?;
        let versions: Vec<String> = cached
            .into_iter()
            .filter(|meta| meta.name == name)
            .map(|meta| meta.version)
            .collect();

        Ok(versions)
    }

    /// Get all cached crates with their metadata
    pub async fn list_all_cached_crates(
        &self,
    ) -> Result<Vec<crate::cache::storage::CrateMetadata>> {
        self.storage.list_cached_crates()
    }

    /// Remove a cached crate version
    pub async fn remove_crate(&self, name: &str, version: &str) -> Result<()> {
        self.storage.remove_crate(name, version)
    }

    /// Get the source path for a crate
    pub fn get_source_path(&self, name: &str, version: &str) -> PathBuf {
        self.storage.source_path(name, version)
    }

    /// Generate and save dependency information for a crate
    async fn generate_dependencies(&self, name: &str, version: &str) -> Result<()> {
        let source_path = self.storage.source_path(name, version);
        let deps_path = self.storage.dependencies_path(name, version);

        tracing::info!("Generating dependency information for {}-{}", name, version);

        // Run cargo metadata to get dependency information
        let output = Command::new("cargo")
            .args(&["metadata", "--format-version", "1"])
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
}
