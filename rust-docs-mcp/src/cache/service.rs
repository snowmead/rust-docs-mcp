use crate::cache::constants::*;
use crate::cache::docgen::DocGenerator;
use crate::cache::downloader::{CrateDownloader, CrateSource};
use crate::cache::member_utils::normalize_member_path;
use crate::cache::storage::{CacheStorage, MemberInfo};
use crate::cache::transaction::CacheTransaction;
use crate::cache::utils::CacheResponse;
use crate::cache::workspace::WorkspaceHandler;
use anyhow::{Context, Result, bail};
use std::path::{Path, PathBuf};

/// Service for managing crate caching and documentation generation
#[derive(Debug, Clone)]
pub struct CrateCache {
    pub(crate) storage: CacheStorage,
    downloader: CrateDownloader,
    doc_generator: DocGenerator,
}

impl CrateCache {
    /// Create a new crate cache instance
    pub fn new(cache_dir: Option<PathBuf>) -> Result<Self> {
        let storage = CacheStorage::new(cache_dir)?;
        let downloader = CrateDownloader::new(storage.clone());
        let doc_generator = DocGenerator::new(storage.clone());

        Ok(Self {
            storage,
            downloader,
            doc_generator,
        })
    }

    /// Ensure a crate's documentation is available, downloading and generating if necessary
    pub async fn ensure_crate_docs(
        &self,
        name: &str,
        version: &str,
        source: Option<&str>,
    ) -> Result<rustdoc_types::Crate> {
        // Check if docs already exist
        if self.storage.has_docs(name, version, None) {
            return self.load_docs(name, version, None).await;
        }

        // Check if crate is downloaded but docs not generated
        if !self.storage.is_cached(name, version) {
            self.download_or_copy_crate(name, version, source).await?;
        }

        // Generate documentation
        self.generate_docs(name, version).await?;

        // Load and return the generated docs
        self.load_docs(name, version, None).await
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
        if self.storage.has_docs(name, version, Some(member_path)) {
            return self.load_docs(name, version, Some(member_path)).await;
        }

        // Check if crate is downloaded
        if !self.storage.is_cached(name, version) {
            self.download_or_copy_crate(name, version, source).await?;
        }

        // Generate documentation for the specific workspace member
        self.generate_workspace_member_docs(name, version, member_path)
            .await?;
            
        // Get package name for the member
        let member_cargo_toml = self.storage.source_path(name, version)?
            .join(member_path)
            .join(CARGO_TOML);
        let package_name = WorkspaceHandler::get_package_name(&member_cargo_toml)?;
        
        // Create member info
        let member_info = MemberInfo {
            original_path: member_path.to_string(),
            normalized_path: normalize_member_path(member_path),
            package_name,
        };
        
        // Save unified metadata
        self.storage.save_metadata_with_source(
            name,
            version,
            source.unwrap_or("unknown"),
            None,
            Some(member_info),
        )?;

        // Load and return the generated docs
        self.load_docs(name, version, Some(member_path)).await
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
            let source_path = self.storage.source_path(name, version)?;
            let cargo_toml_path = source_path.join("Cargo.toml");

            // Check if it's a workspace
            if cargo_toml_path.exists() && WorkspaceHandler::is_workspace(&cargo_toml_path)? {
                // It's a workspace without member specified
                let members = WorkspaceHandler::get_workspace_members(&cargo_toml_path)?;
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
        self.downloader
            .download_or_copy_crate(name, version, source)
            .await
    }

    /// Generate JSON documentation for a crate
    pub async fn generate_docs(&self, name: &str, version: &str) -> Result<PathBuf> {
        self.doc_generator.generate_docs(name, version).await
    }

    /// Generate JSON documentation for a workspace member
    pub async fn generate_workspace_member_docs(
        &self,
        name: &str,
        version: &str,
        member_path: &str,
    ) -> Result<PathBuf> {
        self.doc_generator
            .generate_workspace_member_docs(name, version, member_path)
            .await
    }

    /// Load documentation from cache for a crate or workspace member
    pub async fn load_docs(
        &self,
        name: &str,
        version: &str,
        member_name: Option<&str>,
    ) -> Result<rustdoc_types::Crate> {
        let json_value = self.doc_generator.load_docs(name, version, member_name).await?;
        let context_msg = if member_name.is_some() {
            "Failed to parse member documentation JSON"
        } else {
            "Failed to parse documentation JSON"
        };
        let crate_docs: rustdoc_types::Crate =
            serde_json::from_value(json_value).context(context_msg)?;
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
    ) -> Result<Vec<crate::cache::storage::CacheMetadata>> {
        self.storage.list_cached_crates()
    }

    /// Remove a cached crate version
    pub async fn remove_crate(&self, name: &str, version: &str) -> Result<()> {
        self.storage.remove_crate(name, version)
    }

    /// Check if docs exist without ensuring they're generated
    pub fn has_docs(&self, crate_name: &str, version: &str, member: Option<&str>) -> bool {
        self.storage.has_docs(crate_name, version, member)
    }

    /// Try to load existing docs without generating
    pub async fn try_load_docs(
        &self,
        crate_name: &str,
        version: &str,
        member: Option<&str>,
    ) -> Result<Option<rustdoc_types::Crate>> {
        if self.storage.has_docs(crate_name, version, member) {
            if let Some(member_name) = member {
                Ok(Some(self.load_docs(crate_name, version, Some(member_name)).await?))
            } else {
                Ok(Some(self.load_docs(crate_name, version, None).await?))
            }
        } else {
            Ok(None)
        }
    }


    /// Get the source path for a crate
    pub fn get_source_path(&self, name: &str, version: &str) -> Result<PathBuf> {
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

        self.storage.source_path(name, version)
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
        if cargo_toml_path.exists() && WorkspaceHandler::is_workspace(&cargo_toml_path)? {
            let members = WorkspaceHandler::get_workspace_members(&cargo_toml_path)?;
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

    /// Load dependency information from cache
    pub async fn load_dependencies(&self, name: &str, version: &str) -> Result<serde_json::Value> {
        self.doc_generator.load_dependencies(name, version).await
    }

    /// Internal implementation for caching a crate during update
    async fn cache_crate_with_update_impl(
        &self,
        crate_name: &str,
        version: &str,
        members: &Option<Vec<String>>,
        source_str: Option<&str>,
        source: &CrateSource,
    ) -> Result<CacheResponse> {
        // If members are specified, cache those specific workspace members
        if let Some(members) = members {
            let response = self
                .cache_workspace_members(crate_name, version, members, source_str, true)
                .await;

            // Check if all failed for proper error handling
            if let CacheResponse::PartialSuccess {
                results, errors, ..
            } = &response
                && results.is_empty()
            {
                bail!("Failed to update any workspace members: {:?}", errors);
            }

            return Ok(response);
        }

        // Download the crate
        let source_path = self
            .download_or_copy_crate(crate_name, version, source_str)
            .await?;

        // Check if it's a workspace
        let cargo_toml_path = source_path.join("Cargo.toml");
        if WorkspaceHandler::is_workspace(&cargo_toml_path)? {
            // It's a workspace, get the members
            let members = WorkspaceHandler::get_workspace_members(&cargo_toml_path)?;
            Ok(self.generate_workspace_response(crate_name, version, members, source, true))
        } else {
            // Not a workspace, proceed with normal caching
            self.ensure_crate_docs(crate_name, version, source_str)
                .await?;

            Ok(CacheResponse::success_updated(crate_name, version))
        }
    }

    /// Extract source parameters from CrateSource enum
    fn extract_source_params(
        &self,
        source: &CrateSource,
    ) -> (String, String, Option<Vec<String>>, Option<String>, bool) {
        match source {
            CrateSource::CratesIO(params) => (
                params.crate_name.clone(),
                params.version.clone(),
                params.members.clone(),
                None,
                params.update.unwrap_or(false),
            ),
            CrateSource::GitHub(params) => {
                let version = if let Some(branch) = &params.branch {
                    branch.clone()
                } else if let Some(tag) = &params.tag {
                    tag.clone()
                } else {
                    // This should not happen due to validation in the tool layer
                    String::new()
                };

                let source_str = if let Some(branch) = &params.branch {
                    Some(format!("{}#branch:{branch}", params.github_url))
                } else if let Some(tag) = &params.tag {
                    Some(format!("{}#tag:{tag}", params.github_url))
                } else {
                    Some(params.github_url.clone())
                };

                (
                    params.crate_name.clone(),
                    version,
                    params.members.clone(),
                    source_str,
                    params.update.unwrap_or(false),
                )
            }
            CrateSource::LocalPath(params) => (
                params.crate_name.clone(),
                params
                    .version
                    .clone()
                    .expect("Version should be resolved before extraction"),
                params.members.clone(),
                Some(params.path.clone()),
                params.update.unwrap_or(false),
            ),
        }
    }

    /// Handle caching workspace members
    async fn cache_workspace_members(
        &self,
        crate_name: &str,
        version: &str,
        members: &[String],
        source_str: Option<&str>,
        updated: bool,
    ) -> CacheResponse {
        use futures::future::join_all;

        // Create futures for all member caching operations
        let member_futures: Vec<_> = members
            .iter()
            .map(|member| {
                let member_clone = member.clone();
                async move {
                    let result = self
                        .ensure_workspace_member_docs(
                            crate_name,
                            version,
                            source_str,
                            &member_clone,
                        )
                        .await;
                    (member_clone, result)
                }
            })
            .collect();

        // Execute all futures concurrently
        let results_with_members = join_all(member_futures).await;

        // Collect results and errors
        let mut results = Vec::new();
        let mut errors = Vec::new();

        for (member, result) in results_with_members {
            match result {
                Ok(_) => {
                    results.push(format!("Successfully cached member: {member}"));
                }
                Err(e) => {
                    errors.push(format!("Failed to cache member {member}: {e}"));
                }
            }
        }

        if errors.is_empty() {
            CacheResponse::members_success(crate_name, version, members.to_vec(), results, updated)
        } else {
            CacheResponse::members_partial(
                crate_name,
                version,
                members.to_vec(),
                results,
                errors,
                updated,
            )
        }
    }

    /// Generate workspace detection response
    fn generate_workspace_response(
        &self,
        crate_name: &str,
        version: &str,
        members: Vec<String>,
        source: &CrateSource,
        updated: bool,
    ) -> CacheResponse {
        let source_type = match source {
            CrateSource::CratesIO(_) => "cratesio",
            CrateSource::GitHub(_) => "github",
            CrateSource::LocalPath(_) => "local",
        };

        CacheResponse::workspace_detected(crate_name, version, members, source_type, updated)
    }

    /// Handle update operation for a crate
    async fn handle_crate_update(
        &self,
        crate_name: &str,
        version: &str,
        members: &Option<Vec<String>>,
        source_str: Option<&str>,
        source: &CrateSource,
    ) -> String {
        // Create transaction for safe update
        let mut transaction = CacheTransaction::new(&self.storage, crate_name, version);

        // Begin transaction (creates backup and removes existing cache)
        if let Err(e) = transaction.begin() {
            return CacheResponse::error(format!("Failed to start update transaction: {e}"))
                .to_json();
        }

        // Try to re-cache the crate
        let update_result = self
            .cache_crate_with_update_impl(crate_name, version, members, source_str, source)
            .await;

        // Check if update was successful
        match update_result {
            Ok(response) => {
                // Success - commit transaction
                if let Err(e) = transaction.commit() {
                    return CacheResponse::error(format!(
                        "Update succeeded but failed to cleanup: {e}"
                    ))
                    .to_json();
                }
                response.to_json()
            }
            Err(e) => {
                // Failed - transaction will automatically rollback on drop
                CacheResponse::error(format!("Update failed, restored from backup: {e}")).to_json()
            }
        }
    }

    /// Handle workspace members caching
    async fn handle_workspace_members(
        &self,
        crate_name: &str,
        version: &str,
        members: &[String],
        source_str: Option<&str>,
        updated: bool,
    ) -> CacheResponse {
        self.cache_workspace_members(crate_name, version, members, source_str, updated)
            .await
    }

    /// Detect and handle workspace crates
    async fn detect_and_handle_workspace(
        &self,
        crate_name: &str,
        version: &str,
        source_path: &std::path::Path,
        source: &CrateSource,
        source_str: Option<&str>,
        updated: bool,
    ) -> Result<CacheResponse> {
        let cargo_toml_path = source_path.join("Cargo.toml");

        match WorkspaceHandler::is_workspace(&cargo_toml_path) {
            Ok(true) => {
                // It's a workspace, get the members
                let members = WorkspaceHandler::get_workspace_members(&cargo_toml_path)
                    .context("Failed to get workspace members")?;
                Ok(self.generate_workspace_response(crate_name, version, members, source, updated))
            }
            Ok(false) => {
                // Not a workspace, proceed with normal caching
                self.cache_regular_crate(crate_name, version, source_str)
                    .await
            }
            Err(_e) => {
                // Error checking workspace status, try normal caching anyway
                self.cache_regular_crate(crate_name, version, source_str)
                    .await
            }
        }
    }

    /// Cache a regular (non-workspace) crate
    async fn cache_regular_crate(
        &self,
        crate_name: &str,
        version: &str,
        source_str: Option<&str>,
    ) -> Result<CacheResponse> {
        self.ensure_crate_docs(crate_name, version, source_str)
            .await
            .context("Failed to cache crate")?;
        Ok(CacheResponse::success(crate_name, version))
    }

    /// Resolve version for local paths
    async fn resolve_local_path_version(
        &self,
        params: &crate::cache::tools::CacheCrateFromLocalParams,
    ) -> Result<(String, bool)> {
        // Expand path to handle ~ and other shell expansions
        let expanded_path = shellexpand::full(&params.path)
            .with_context(|| format!("Failed to expand path: {}", params.path))?;
        let local_path = Path::new(expanded_path.as_ref());

        // Check if path exists
        if !local_path.exists() {
            bail!("Local path does not exist: {}", local_path.display());
        }

        let cargo_toml = local_path.join("Cargo.toml");
        if !cargo_toml.exists() {
            bail!("No Cargo.toml found at path: {}", local_path.display());
        }

        // Get the actual version from Cargo.toml
        let actual_version = WorkspaceHandler::get_package_version(&cargo_toml)?;

        match &params.version {
            Some(provided_version) => {
                // Version was provided, validate it matches
                if provided_version != &actual_version {
                    bail!(
                        "Version mismatch: provided version '{}' does not match actual version '{}' in Cargo.toml",
                        provided_version,
                        actual_version
                    );
                }
                Ok((actual_version, false)) // Version was validated, not auto-detected
            }
            None => {
                // No version provided, use the detected one
                Ok((actual_version, true)) // Version was auto-detected
            }
        }
    }

    /// Common method to cache a crate from any source
    pub async fn cache_crate_with_source(&self, source: CrateSource) -> String {
        // For local paths, resolve version if needed
        let source = if let CrateSource::LocalPath(mut params) = source {
            match self.resolve_local_path_version(&params).await {
                Ok((resolved_version, auto_detected)) => {
                    // Update params with resolved version
                    params.version = Some(resolved_version.clone());

                    // Log if version was auto-detected
                    if auto_detected {
                        tracing::info!(
                            "Auto-detected version '{}' from local path for crate '{}'",
                            resolved_version,
                            params.crate_name
                        );
                    }

                    CrateSource::LocalPath(params)
                }
                Err(e) => {
                    return CacheResponse::error(format!("Failed to resolve local path: {e}"))
                        .to_json();
                }
            }
        } else {
            source
        };

        // Extract parameters from source
        let (crate_name, version, members, source_str, update) =
            self.extract_source_params(&source);

        // Validate GitHub source
        if matches!(&source, CrateSource::GitHub(_)) && version.is_empty() {
            return CacheResponse::error("Either branch or tag must be specified").to_json();
        }

        // Handle update logic if requested
        if update && self.storage.is_cached(&crate_name, &version) {
            return self
                .handle_crate_update(
                    &crate_name,
                    &version,
                    &members,
                    source_str.as_deref(),
                    &source,
                )
                .await;
        }

        // If members are specified, cache those specific workspace members
        if let Some(members) = members {
            let response = self
                .handle_workspace_members(
                    &crate_name,
                    &version,
                    &members,
                    source_str.as_deref(),
                    false,
                )
                .await;
            return response.to_json();
        }

        // First, download the crate if not already cached
        let source_path = match self
            .download_or_copy_crate(&crate_name, &version, source_str.as_deref())
            .await
        {
            Ok(path) => path,
            Err(e) => {
                return CacheResponse::error(format!("Failed to download crate: {e}")).to_json();
            }
        };

        // Detect and handle workspace vs regular crate
        match self
            .detect_and_handle_workspace(
                &crate_name,
                &version,
                &source_path,
                &source,
                source_str.as_deref(),
                false,
            )
            .await
        {
            Ok(response) => response.to_json(),
            Err(e) => CacheResponse::error(format!("Failed to cache crate: {e}")).to_json(),
        }
    }

    /// Create search index for a crate or workspace member (exposed for search module)
    pub async fn create_search_index(
        &self,
        name: &str,
        version: &str,
        member_name: Option<&str>,
    ) -> Result<()> {
        self.doc_generator
            .create_search_index(name, version, member_name)
            .await
    }
}
