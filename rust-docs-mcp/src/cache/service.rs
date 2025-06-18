use crate::cache::storage::CacheStorage;
use crate::cache::tools::{CacheCrateFromCratesIOParams, CacheCrateFromGitHubParams, CacheCrateFromLocalParams};
use anyhow::{Context, Result, bail};
use flate2::read::GzDecoder;
use futures::StreamExt;
use git2::Repository;
use std::fs;
use std::fs::File;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;
use tar::Archive;
use toml::Value;

/// Different source types for crates
#[derive(Debug, Clone, PartialEq)]
enum SourceType {
    CratesIo,
    GitHub {
        url: String,
        repo_path: Option<String>,
    },
    Local {
        path: String,
    },
}

/// Unified crate source enum that reuses the parameter structs from tools
#[derive(Debug, Clone)]
pub enum CrateSource {
    CratesIO(CacheCrateFromCratesIOParams),
    GitHub(CacheCrateFromGitHubParams),
    LocalPath(CacheCrateFromLocalParams),
}

/// Service for managing crate caching and documentation generation
#[derive(Debug, Clone)]
pub struct CrateCache {
    pub(crate) storage: CacheStorage,
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
                } else if s.starts_with('/')
                    || s.starts_with("~/")
                    || s.starts_with("../")
                    || s.starts_with("./")
                {
                    SourceType::Local {
                        path: s.to_string(),
                    }
                } else {
                    // If it contains path separators, treat as local path
                    if s.contains('/') || s.contains('\\') {
                        SourceType::Local {
                            path: s.to_string(),
                        }
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
                        repo_path: Some(repo_path),
                    }
                } else {
                    SourceType::GitHub {
                        url: base_url,
                        repo_path: None,
                    }
                }
            } else {
                // Invalid GitHub URL, treat as local path
                SourceType::Local {
                    path: url.to_string(),
                }
            }
        } else if url.starts_with("http://github.com/") {
            // Convert http to https and recurse
            let https_url = url.replace("http://", "https://");
            self.parse_github_url(&https_url)
        } else {
            // Not a GitHub URL, treat as local path
            SourceType::Local {
                path: url.to_string(),
            }
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

    /// Ensure a workspace member's documentation is available
    pub async fn ensure_workspace_member_docs(
        &self,
        name: &str,
        version: &str,
        source: Option<&str>,
        member_path: &str,
    ) -> Result<rustdoc_types::Crate> {
        // Check if docs already exist for this member
        let member_name = member_path.split('/').last().unwrap_or(member_path);

        if self.storage.has_member_docs(name, version, member_name) {
            return self.load_member_docs(name, version, member_name).await;
        }

        // Check if crate is downloaded
        if !self.storage.is_cached(name, version) {
            self.download_or_copy_crate(name, version, source).await?;
        }

        // Generate documentation for the specific workspace member
        self.generate_workspace_member_docs(name, version, member_path)
            .await?;

        // Load and return the generated docs
        self.load_member_docs(name, version, member_name).await
    }

    /// Ensure documentation is available for a crate or workspace member
    pub async fn ensure_crate_or_member_docs(
        &self,
        name: &str,
        version: &str,
        member: Option<&str>,
    ) -> Result<rustdoc_types::Crate> {
        // If member is specified, use workspace member logic
        if let Some(member_path) = member {
            return self
                .ensure_workspace_member_docs(name, version, None, member_path)
                .await;
        }

        // Check if crate is already downloaded
        if self.storage.is_cached(name, version) {
            let source_path = self.storage.source_path(name, version);
            let cargo_toml_path = source_path.join("Cargo.toml");

            // Check if it's a workspace
            if cargo_toml_path.exists() && self.is_workspace(&cargo_toml_path)? {
                // It's a workspace without member specified
                let members = self.get_workspace_members(&cargo_toml_path)?;
                bail!(
                    "This is a workspace crate. Please specify a member using the 'member' parameter.\n\
                    Available members: {:?}\n\
                    Example: specify member=\"{}\"",
                    members,
                    members.first().unwrap_or(&"crates/example".to_string())
                );
            }
        }

        // Regular crate, use normal flow
        self.ensure_crate_docs(name, version, None).await
    }

    /// Download or copy a crate based on source type
    pub async fn download_or_copy_crate(
        &self,
        name: &str,
        version: &str,
        source: Option<&str>,
    ) -> Result<PathBuf> {
        let source_type = self.detect_source_type(source);

        match source_type {
            SourceType::CratesIo => self.download_crate(name, version).await,
            SourceType::GitHub { url, repo_path } => {
                self.download_from_github(name, version, &url, repo_path.as_deref())
                    .await
            }
            SourceType::Local { path } => self.copy_from_local(name, version, &path).await,
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

        let temp_dir = std::env::temp_dir().join(format!("rust-docs-mcp-git-{}-{}", name, version));

        // Clean up any existing temp directory
        if temp_dir.exists() {
            fs::remove_dir_all(&temp_dir).context("Failed to clean temp directory")?;
        }

        // Clone the repository
        let repo = Repository::clone(repo_url, &temp_dir)
            .with_context(|| format!("Failed to clone repository: {}", repo_url))?;
        
        // Checkout the specific branch or tag (version contains the branch/tag name)
        // The version parameter here is actually the branch or tag name
        if version != "main" && version != "master" {
            // Try to checkout as a branch first
            let refname = format!("refs/remotes/origin/{}", version);
            if let Ok(reference) = repo.find_reference(&refname) {
                let oid = reference.target().ok_or_else(|| anyhow::anyhow!("Reference has no target"))?;
                repo.set_head_detached(oid)
                    .with_context(|| format!("Failed to checkout branch: {}", version))?;
                repo.checkout_head(Some(git2::build::CheckoutBuilder::default().force()))
                    .with_context(|| format!("Failed to checkout branch: {}", version))?;
            } else {
                // Try as a tag
                let tag_ref = format!("refs/tags/{}", version);
                if let Ok(reference) = repo.find_reference(&tag_ref) {
                    let oid = reference.target().ok_or_else(|| anyhow::anyhow!("Reference has no target"))?;
                    repo.set_head_detached(oid)
                        .with_context(|| format!("Failed to checkout tag: {}", version))?;
                    repo.checkout_head(Some(git2::build::CheckoutBuilder::default().force()))
                        .with_context(|| format!("Failed to checkout tag: {}", version))?;
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
            .with_context(|| format!("Failed to expand path: {}", local_path))?;
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
        let source_path = self.storage.source_path(name, version);
        self.storage.ensure_dir(&source_path)?;

        self.copy_directory_contents(source_path_input, &source_path)
            .context("Failed to copy local directory contents")?;

        // Save metadata with source information
        self.storage
            .save_metadata_with_source(name, version, "local", Some(local_path))?;

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
                fs::copy(&path, &dest_path).with_context(|| {
                    format!(
                        "Failed to copy file from {} to {}",
                        path.display(),
                        dest_path.display()
                    )
                })?;
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

    /// Generate JSON documentation for a workspace member
    pub async fn generate_workspace_member_docs(
        &self,
        name: &str,
        version: &str,
        member_path: &str,
    ) -> Result<PathBuf> {
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
        let package_name = self.get_package_name(&member_cargo_toml)?;

        // Extract the member name from the path (last directory)
        let member_name = member_path.split('/').last().unwrap_or(member_path);
        let docs_path = self.storage.member_docs_path(name, version, member_name);

        tracing::info!(
            "Generating documentation for workspace member {} (package: {}) in {}-{}",
            member_path,
            package_name,
            name,
            version
        );

        // Run cargo rustdoc with JSON output for the specific package
        let output = Command::new("cargo")
            .args(&[
                "+nightly",
                "rustdoc",
                "-p",
                &package_name,
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
        // The JSON file is named after the package name (with underscores)
        let json_file = self.find_json_doc(&doc_dir, &package_name)?;

        // Copy the JSON file to our cache location
        self.storage.ensure_dir(docs_path.parent().unwrap())?;
        std::fs::copy(&json_file, &docs_path).context("Failed to copy documentation to cache")?;

        // Generate and save dependency information for the member
        self.generate_workspace_member_dependencies(name, version, member_path)
            .await?;

        // Update metadata to reflect that docs are now generated
        self.storage.save_member_metadata(
            name,
            version,
            member_name,
            &package_name,
            "github",
            Some(member_path),
        )?;

        // Trigger background analysis
        tracing::info!(
            "Successfully generated documentation for workspace member {} (package: {}) in {}-{}",
            member_path,
            package_name,
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
    
    /// Load workspace member documentation from cache
    pub async fn load_member_docs(&self, name: &str, version: &str, member_name: &str) -> Result<rustdoc_types::Crate> {
        let docs_path = self.storage.member_docs_path(name, version, member_name);

        if !docs_path.exists() {
            bail!("Documentation not found for {}/{} in {}-{}", member_name, name, name, version);
        }

        let json_string = tokio::fs::read_to_string(&docs_path)
            .await
            .context("Failed to read member documentation file")?;

        let crate_docs: rustdoc_types::Crate =
            serde_json::from_str(&json_string).context("Failed to parse member documentation JSON")?;

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

    /// Ensure a crate's source is available, downloading if necessary (without generating docs)
    pub async fn ensure_crate_source(
        &self,
        name: &str,
        version: &str,
        source: Option<&str>,
    ) -> Result<PathBuf> {
        // Check if crate is already downloaded
        if !self.storage.is_cached(name, version) {
            self.download_or_copy_crate(name, version, source).await?;
        }

        Ok(self.storage.source_path(name, version))
    }

    /// Ensure source is available for a crate or workspace member
    pub async fn ensure_crate_or_member_source(
        &self,
        name: &str,
        version: &str,
        member: Option<&str>,
        source: Option<&str>,
    ) -> Result<PathBuf> {
        // Ensure the crate source is downloaded
        let source_path = self.ensure_crate_source(name, version, source).await?;

        // If member is specified, return the member's source path
        if let Some(member_path) = member {
            let member_source_path = source_path.join(member_path);
            let member_cargo_toml = member_source_path.join("Cargo.toml");

            if !member_cargo_toml.exists() {
                bail!(
                    "Workspace member '{}' not found in {}-{}. \
                    Make sure the member path is correct.",
                    member_path,
                    name,
                    version
                );
            }

            return Ok(member_source_path);
        }

        // Check if it's a workspace without member specified
        let cargo_toml_path = source_path.join("Cargo.toml");
        if cargo_toml_path.exists() && self.is_workspace(&cargo_toml_path)? {
            let members = self.get_workspace_members(&cargo_toml_path)?;
            bail!(
                "This is a workspace crate. Please specify a member using the 'member' parameter.\n\
                Available members: {:?}\n\
                Example: specify member=\"{}\"",
                members,
                members.first().unwrap_or(&"crates/example".to_string())
            );
        }

        // Regular crate, return source path
        Ok(source_path)
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

    /// Generate and save dependency information for a workspace member
    async fn generate_workspace_member_dependencies(
        &self,
        name: &str,
        version: &str,
        member_path: &str,
    ) -> Result<()> {
        let source_path = self.storage.source_path(name, version);
        let member_name = member_path.split('/').last().unwrap_or(member_path);
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
            .args(&[
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
        self.storage.ensure_dir(deps_path.parent().unwrap())?;

        // Save the raw metadata output
        tokio::fs::write(&deps_path, &output.stdout)
            .await
            .context("Failed to write dependencies to cache")?;

        Ok(())
    }

    /// Check if a Cargo.toml is a virtual manifest (workspace without [package])
    pub fn is_workspace(&self, cargo_toml_path: &Path) -> Result<bool> {
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
    pub fn get_workspace_members(&self, cargo_toml_path: &Path) -> Result<Vec<String>> {
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
            .ok_or_else(|| anyhow::anyhow!("No [workspace] section found in Cargo.toml"))?;

        let members = workspace
            .get("members")
            .and_then(|m| m.as_array())
            .ok_or_else(|| anyhow::anyhow!("No members array found in [workspace] section"))?;

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
    pub fn get_package_name(&self, cargo_toml_path: &Path) -> Result<String> {
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
            .ok_or_else(|| anyhow::anyhow!("No [package] section found in Cargo.toml"))?;

        let name = package
            .get("name")
            .and_then(|n| n.as_str())
            .ok_or_else(|| anyhow::anyhow!("No 'name' field found in [package] section"))?;

        Ok(name.to_string())
    }

    /// Common method to cache a crate from any source
    pub async fn cache_crate_with_source(&self, source: CrateSource) -> String {
        let (crate_name, version_owned, members, source_str);
        
        match &source {
            CrateSource::CratesIO(params) => {
                crate_name = &params.crate_name;
                version_owned = params.version.clone();
                members = &params.members;
                source_str = None;
            }
            CrateSource::GitHub(params) => {
                crate_name = &params.crate_name;
                // Use branch or tag as version
                version_owned = if let Some(branch) = &params.branch {
                    branch.clone()
                } else if let Some(tag) = &params.tag {
                    tag.clone()
                } else {
                    // This should not happen due to validation in the tool layer
                    return format!(r#"{{"error": "Either branch or tag must be specified"}}"#);
                };
                members = &params.members;
                // Include branch/tag in the source string for tracking
                source_str = if let Some(branch) = &params.branch {
                    Some(format!("{}#branch:{}", params.github_url, branch))
                } else if let Some(tag) = &params.tag {
                    Some(format!("{}#tag:{}", params.github_url, tag))
                } else {
                    Some(params.github_url.clone())
                };
            }
            CrateSource::LocalPath(params) => {
                crate_name = &params.crate_name;
                version_owned = params.version.clone();
                members = &params.members;
                source_str = Some(params.path.clone());
            }
        }
        
        let version = &version_owned;

        // If members are specified, cache those specific workspace members
        if let Some(members) = members {
            let mut results = Vec::new();
            let mut errors = Vec::new();

            for member in members {
                match self
                    .ensure_workspace_member_docs(
                        crate_name,
                        version,
                        source_str.as_deref(),
                        member,
                    )
                    .await
                {
                    Ok(_) => {
                        results.push(format!("Successfully cached member: {}", member));
                    }
                    Err(e) => {
                        errors.push(format!("Failed to cache member {}: {}", member, e));
                    }
                }
            }

            if errors.is_empty() {
                return serde_json::json!({
                    "status": "success",
                    "message": format!("Successfully cached {} workspace members", results.len()),
                    "crate": crate_name,
                    "version": version,
                    "members": members,
                    "results": results
                })
                .to_string();
            } else {
                return serde_json::json!({
                    "status": "partial_success",
                    "message": format!("Cached {} members with {} errors", results.len(), errors.len()),
                    "crate": crate_name,
                    "version": version,
                    "members": members,
                    "results": results,
                    "errors": errors
                })
                .to_string();
            }
        }

        // First, download the crate if not already cached
        let source_path = match self
            .download_or_copy_crate(
                crate_name,
                version,
                source_str.as_deref(),
            )
            .await
        {
            Ok(path) => path,
            Err(e) => return format!(r#"{{"error": "Failed to download crate: {}"}}"#, e),
        };

        // Check if it's a workspace
        let cargo_toml_path = source_path.join("Cargo.toml");
        match self.is_workspace(&cargo_toml_path) {
            Ok(true) => {
                // It's a workspace, get the members
                match self.get_workspace_members(&cargo_toml_path) {
                    Ok(members) => {
                        serde_json::json!({
                            "status": "workspace_detected",
                            "message": "This is a workspace crate. Please specify which members to cache using the 'members' parameter.",
                            "crate": crate_name,
                            "version": version,
                            "workspace_members": members,
                            "example_usage": format!(
                                "cache_crate_from_{}(crate_name=\"{}\", version=\"{}\", members={:?})",
                                match source {
                                    CrateSource::CratesIO(_) => "cratesio",
                                    CrateSource::GitHub(_) => "github",
                                    CrateSource::LocalPath(_) => "local",
                                },
                                crate_name,
                                version,
                                members.get(0..2.min(members.len())).unwrap_or(&[])
                            )
                        })
                        .to_string()
                    }
                    Err(e) => {
                        format!(r#"{{"error": "Failed to get workspace members: {}"}}"#, e)
                    }
                }
            }
            Ok(false) => {
                // Not a workspace, proceed with normal caching
                match self
                    .ensure_crate_docs(
                        crate_name,
                        version,
                        source_str.as_deref(),
                    )
                    .await
                {
                    Ok(_) => serde_json::json!({
                        "status": "success",
                        "message": format!("Successfully cached {}-{}", crate_name, version),
                        "crate": crate_name,
                        "version": version
                    })
                    .to_string(),
                    Err(e) => {
                        format!(r#"{{"error": "Failed to cache crate: {}"}}"#, e)
                    }
                }
            }
            Err(_e) => {
                // Error checking workspace status, try normal caching anyway
                match self
                    .ensure_crate_docs(
                        crate_name,
                        version,
                        source_str.as_deref(),
                    )
                    .await
                {
                    Ok(_) => serde_json::json!({
                        "status": "success",
                        "message": format!("Successfully cached {}-{}", crate_name, version),
                        "crate": crate_name,
                        "version": version
                    })
                    .to_string(),
                    Err(e) => {
                        format!(r#"{{"error": "Failed to cache crate: {}"}}"#, e)
                    }
                }
            }
        }
    }
}
