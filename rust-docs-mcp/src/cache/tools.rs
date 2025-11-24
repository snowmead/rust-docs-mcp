use std::sync::Arc;
use tokio::sync::RwLock;

use rmcp::schemars;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::cache::{
    CrateCache,
    downloader::CrateSource,
    outputs::{
        CacheCrateOutput, CacheTaskStartedOutput, CrateMetadata, ErrorOutput,
        GetCratesMetadataOutput, ListCachedCratesOutput, ListCrateVersionsOutput,
        RemoveCrateOutput, SizeInfo, VersionInfo,
    },
    task_formatter,
    task_manager::{CachingStage, TaskManager, TaskStatus},
    utils::format_bytes,
};

/// Parameters for the unified cache_crate tool
///
/// This struct uses a flat design where all source-specific fields are optional,
/// and the `source_type` field determines which fields are required.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct CacheCrateParams {
    #[schemars(description = "The name of the crate")]
    pub crate_name: String,

    #[schemars(description = "Source type: must be 'cratesio', 'github', or 'local'")]
    pub source_type: String,

    // CratesIO parameters
    #[schemars(
        description = "Version of the crate (REQUIRED for source_type='cratesio', e.g., '1.0.0')"
    )]
    pub version: Option<String>,

    // GitHub parameters
    #[schemars(
        description = "GitHub repository URL (REQUIRED for source_type='github', e.g., 'https://github.com/user/repo')"
    )]
    pub github_url: Option<String>,
    #[schemars(
        description = "Branch name (REQUIRED for source_type='github' if tag not provided, e.g., 'main', 'develop')"
    )]
    pub branch: Option<String>,
    #[schemars(
        description = "Tag name (REQUIRED for source_type='github' if branch not provided, e.g., 'v1.0.0', '0.2.1')"
    )]
    pub tag: Option<String>,

    // Local parameters
    #[schemars(
        description = "Local file system path (REQUIRED for source_type='local', supports absolute paths (/path), home paths (~/path), and relative paths (./path, ../path))"
    )]
    pub path: Option<String>,

    // Common parameters
    #[schemars(
        description = "Optional list of workspace members to cache. If the crate is a workspace and this is not provided, the tool will return a list of available members. Specify member paths relative to the workspace root (e.g., [\"crates/rmcp\", \"crates/rmcp-macros\"])."
    )]
    pub members: Option<Vec<String>>,
    #[schemars(
        description = "Force re-download and re-cache the crate even if it already exists. Defaults to false. The existing cache is preserved until the update succeeds."
    )]
    pub update: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct CacheCrateFromCratesIOParams {
    #[schemars(description = "The name of the crate")]
    pub crate_name: String,
    #[schemars(description = "The version of the crate")]
    pub version: String,
    #[schemars(
        description = "Optional list of workspace members to cache. If the crate is a workspace and this is not provided, the tool will return a list of available members. Specify member paths relative to the workspace root (e.g., [\"crates/rmcp\", \"crates/rmcp-macros\"])."
    )]
    pub members: Option<Vec<String>>,
    #[schemars(
        description = "Force re-download and re-cache the crate even if it already exists. Defaults to false. The existing cache is preserved until the update succeeds."
    )]
    pub update: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct CacheCrateFromGitHubParams {
    #[schemars(description = "The name of the crate")]
    pub crate_name: String,
    #[schemars(description = "GitHub repository URL (e.g., https://github.com/user/repo)")]
    pub github_url: String,
    #[schemars(
        description = "Branch to use (e.g., 'main', 'develop'). Only one of branch or tag can be specified."
    )]
    pub branch: Option<String>,
    #[schemars(
        description = "Tag to use (e.g., 'v1.0.0', '0.2.1'). Only one of branch or tag can be specified."
    )]
    pub tag: Option<String>,
    #[schemars(
        description = "Optional list of workspace members to cache. If the crate is a workspace and this is not provided, the tool will return a list of available members. Specify member paths relative to the workspace root (e.g., [\"crates/rmcp\", \"crates/rmcp-macros\"])."
    )]
    pub members: Option<Vec<String>>,
    #[schemars(
        description = "Force re-download and re-cache the crate even if it already exists. Defaults to false. The existing cache is preserved until the update succeeds."
    )]
    pub update: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct CacheCrateFromLocalParams {
    #[schemars(description = "The name of the crate")]
    pub crate_name: String,
    #[schemars(
        description = "Optional version to use for caching. If not provided, the version from the local crate's Cargo.toml will be used. If provided, it will be validated against the actual version."
    )]
    pub version: Option<String>,
    #[schemars(
        description = "Local file system path. Supports absolute paths (/path), home paths (~/path), and relative paths (./path, ../path)"
    )]
    pub path: String,
    #[schemars(
        description = "Optional list of workspace members to cache. If the crate is a workspace and this is not provided, the tool will return a list of available members. Specify member paths relative to the workspace root (e.g., [\"crates/rmcp\", \"crates/rmcp-macros\"])."
    )]
    pub members: Option<Vec<String>>,
    #[schemars(
        description = "Force re-download and re-cache the crate even if it already exists. Defaults to false. The existing cache is preserved until the update succeeds."
    )]
    pub update: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct CrateMetadataQuery {
    #[schemars(description = "The name of the crate")]
    pub crate_name: String,
    #[schemars(description = "The version of the crate")]
    pub version: String,
    #[schemars(
        description = "Optional list of workspace members to query (e.g., ['crates/rmcp', 'crates/rmcp-macros'])"
    )]
    pub members: Option<Vec<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct GetCratesMetadataParams {
    #[schemars(description = "List of crates and their members to query metadata for")]
    pub queries: Vec<CrateMetadataQuery>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct RemoveCrateParams {
    #[schemars(description = "The name of the crate")]
    pub crate_name: String,
    #[schemars(description = "The version of the crate")]
    pub version: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ListCrateVersionsParams {
    #[schemars(description = "The name of the crate")]
    pub crate_name: String,
}

/// Parameters for the cache_operations tool
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct CacheOperationsParams {
    #[schemars(
        description = "Optional task_id to query specific task or to cancel/clear. If not provided, lists all tasks"
    )]
    pub task_id: Option<String>,

    #[schemars(
        description = "Optional status filter when listing tasks: \"pending\", \"in_progress\", \"completed\", \"failed\", \"cancelled\""
    )]
    pub status_filter: Option<String>,

    #[schemars(description = "Set to true to cancel the specified task (requires task_id)")]
    #[serde(default)]
    pub cancel: bool,

    #[schemars(
        description = "Set to true to remove completed/failed tasks from memory (clears specified task or all if no task_id)"
    )]
    #[serde(default)]
    pub clear: bool,
}

#[derive(Debug, Clone)]
pub struct CacheTools {
    cache: Arc<RwLock<CrateCache>>,
    task_manager: Arc<TaskManager>,
}

impl CacheTools {
    /// Create a new CacheTools instance
    pub fn new(cache: Arc<RwLock<CrateCache>>, task_manager: Arc<TaskManager>) -> Self {
        Self {
            cache,
            task_manager,
        }
    }

    pub async fn cache_crate_from_cratesio(
        &self,
        params: CacheCrateFromCratesIOParams,
    ) -> CacheCrateOutput {
        let cache = self.cache.write().await;
        let source = CrateSource::CratesIO(params);
        let json_response = cache.cache_crate_with_source(source, None, None).await;
        serde_json::from_str(&json_response).unwrap_or_else(|_| CacheCrateOutput::Error {
            error: "Failed to parse cache response".to_string(),
        })
    }

    pub async fn cache_crate_from_github(
        &self,
        params: CacheCrateFromGitHubParams,
    ) -> CacheCrateOutput {
        // Validate that only one of branch or tag is provided
        match (&params.branch, &params.tag) {
            (Some(_), Some(_)) => {
                return CacheCrateOutput::Error {
                    error: "Only one of 'branch' or 'tag' can be specified, not both".to_string(),
                };
            }
            (None, None) => {
                return CacheCrateOutput::Error {
                    error: "Either 'branch' or 'tag' must be specified".to_string(),
                };
            }
            _ => {} // Valid: exactly one is provided
        }

        let cache = self.cache.write().await;
        let source = CrateSource::GitHub(params);
        let json_response = cache.cache_crate_with_source(source, None, None).await;
        serde_json::from_str(&json_response).unwrap_or_else(|_| CacheCrateOutput::Error {
            error: "Failed to parse cache response".to_string(),
        })
    }

    pub async fn cache_crate_from_local(
        &self,
        params: CacheCrateFromLocalParams,
    ) -> CacheCrateOutput {
        let cache = self.cache.write().await;
        let source = CrateSource::LocalPath(params);
        let json_response = cache.cache_crate_with_source(source, None, None).await;
        serde_json::from_str(&json_response).unwrap_or_else(|_| CacheCrateOutput::Error {
            error: "Failed to parse cache response".to_string(),
        })
    }

    pub async fn remove_crate(
        &self,
        params: RemoveCrateParams,
    ) -> Result<RemoveCrateOutput, ErrorOutput> {
        let cache = self.cache.write().await;
        match cache
            .remove_crate(&params.crate_name, &params.version)
            .await
        {
            Ok(_) => Ok(RemoveCrateOutput {
                status: "success".to_string(),
                message: format!(
                    "Successfully removed {}-{}",
                    params.crate_name, params.version
                ),
                crate_name: params.crate_name,
                version: params.version,
            }),
            Err(e) => Err(ErrorOutput::new(format!("Failed to remove crate: {e}"))),
        }
    }

    pub async fn list_cached_crates(&self) -> Result<ListCachedCratesOutput, ErrorOutput> {
        let cache = self.cache.read().await;
        match cache.list_all_cached_crates().await {
            Ok(mut crates) => {
                // Sort by name and version for consistent output
                crates.sort_by(|a, b| {
                    a.name.cmp(&b.name).then_with(|| b.version.cmp(&a.version)) // Newer versions first
                });

                // Calculate total size
                let total_size_bytes: u64 = crates.iter().map(|c| c.size_bytes).sum();

                // Group by crate name for better organization
                let mut grouped: std::collections::HashMap<String, Vec<VersionInfo>> =
                    std::collections::HashMap::new();
                for crate_meta in crates {
                    let crate_name = crate_meta.name.clone();
                    let version = crate_meta.version.clone();

                    // Get workspace members for this crate version
                    let members = match cache.storage.list_workspace_members(&crate_name, &version)
                    {
                        Ok(members) if !members.is_empty() => Some(members),
                        _ => None,
                    };

                    let version_info = VersionInfo {
                        version: crate_meta.version,
                        cached_at: crate_meta.cached_at.to_string(),
                        doc_generated: crate_meta.doc_generated,
                        size_bytes: crate_meta.size_bytes,
                        size_human: format_bytes(crate_meta.size_bytes),
                        members,
                    };

                    grouped.entry(crate_name).or_default().push(version_info);
                }

                Ok(ListCachedCratesOutput {
                    crates: grouped.clone(),
                    total_crates: grouped.len(),
                    total_versions: grouped.values().map(|v| v.len()).sum::<usize>(),
                    total_size: SizeInfo {
                        bytes: total_size_bytes,
                        human: format_bytes(total_size_bytes),
                    },
                })
            }
            Err(e) => Err(ErrorOutput::new(format!(
                "Failed to list cached crates: {e}"
            ))),
        }
    }

    pub async fn list_crate_versions(
        &self,
        params: ListCrateVersionsParams,
    ) -> Result<ListCrateVersionsOutput, ErrorOutput> {
        let cache = self.cache.read().await;

        // Get all cached metadata for this crate
        match cache.storage.list_cached_crates() {
            Ok(all_crates) => {
                // Filter to just this crate's versions
                let mut versions: Vec<VersionInfo> = all_crates
                    .into_iter()
                    .filter(|meta| meta.name == params.crate_name)
                    .map(|meta| {
                        // Get workspace members if any
                        let members = match cache
                            .storage
                            .list_workspace_members(&meta.name, &meta.version)
                        {
                            Ok(members) if !members.is_empty() => Some(members),
                            _ => None,
                        };

                        VersionInfo {
                            version: meta.version,
                            cached_at: meta.cached_at.to_string(),
                            doc_generated: meta.doc_generated,
                            size_bytes: meta.size_bytes,
                            size_human: format_bytes(meta.size_bytes),
                            members,
                        }
                    })
                    .collect();

                // Sort versions (newest first)
                versions.sort_by(|a, b| b.version.cmp(&a.version));

                Ok(ListCrateVersionsOutput {
                    crate_name: params.crate_name.clone(),
                    versions: versions.clone(),
                    count: versions.len(),
                })
            }
            Err(e) => Err(ErrorOutput::new(format!(
                "Failed to get cached versions: {e}"
            ))),
        }
    }

    pub async fn get_crates_metadata(
        &self,
        params: GetCratesMetadataParams,
    ) -> GetCratesMetadataOutput {
        let cache = self.cache.read().await;
        let mut metadata_list = Vec::new();
        let mut total_cached = 0;
        let total_queried = params.queries.len();

        for query in params.queries {
            let crate_name = &query.crate_name;
            let version = &query.version;

            // Check if main crate is cached
            if cache.storage.is_cached(crate_name, version) {
                total_cached += 1;

                let main_metadata = match cache.storage.load_metadata(crate_name, version, None) {
                    Ok(metadata) => {
                        // Check if docs are analyzed
                        let analyzed = cache.storage.has_docs(crate_name, version, None);

                        CrateMetadata {
                            crate_name: crate_name.clone(),
                            version: version.clone(),
                            cached: true,
                            analyzed,
                            cache_size_bytes: Some(metadata.size_bytes),
                            cache_size_human: Some(format_bytes(metadata.size_bytes)),
                            member: None,
                            workspace_members: None,
                        }
                    }
                    Err(_) => CrateMetadata {
                        crate_name: crate_name.clone(),
                        version: version.clone(),
                        cached: true,
                        analyzed: false,
                        cache_size_bytes: None,
                        cache_size_human: None,
                        member: None,
                        workspace_members: None,
                    },
                };
                metadata_list.push(main_metadata);
            } else {
                metadata_list.push(CrateMetadata {
                    crate_name: crate_name.clone(),
                    version: version.clone(),
                    cached: false,
                    analyzed: false,
                    cache_size_bytes: None,
                    cache_size_human: None,
                    member: None,
                    workspace_members: None,
                });
            }

            // Check requested members if any
            if let Some(members) = query.members {
                for member_path in members {
                    if cache
                        .storage
                        .is_member_cached(crate_name, version, &member_path)
                    {
                        total_cached += 1;

                        let member_metadata = match cache.storage.load_metadata(
                            crate_name,
                            version,
                            Some(&member_path),
                        ) {
                            Ok(metadata) => {
                                let analyzed =
                                    cache
                                        .storage
                                        .has_docs(crate_name, version, Some(&member_path));

                                CrateMetadata {
                                    crate_name: crate_name.clone(),
                                    version: version.clone(),
                                    cached: true,
                                    analyzed,
                                    cache_size_bytes: Some(metadata.size_bytes),
                                    cache_size_human: Some(format_bytes(metadata.size_bytes)),
                                    member: Some(member_path),
                                    workspace_members: None,
                                }
                            }
                            Err(_) => CrateMetadata {
                                crate_name: crate_name.clone(),
                                version: version.clone(),
                                cached: true,
                                analyzed: false,
                                cache_size_bytes: None,
                                cache_size_human: None,
                                member: Some(member_path),
                                workspace_members: None,
                            },
                        };
                        metadata_list.push(member_metadata);
                    } else {
                        metadata_list.push(CrateMetadata {
                            crate_name: crate_name.clone(),
                            version: version.clone(),
                            cached: false,
                            analyzed: false,
                            cache_size_bytes: None,
                            cache_size_human: None,
                            member: Some(member_path),
                            workspace_members: None,
                        });
                    }
                }
            }
        }

        GetCratesMetadataOutput {
            metadata: metadata_list,
            total_queried,
            total_cached,
        }
    }

    /// Resolve version from local Cargo.toml synchronously
    ///
    /// Returns `(version, auto_detected)` tuple or error message.
    /// This helper reads the Cargo.toml to get the real version before creating a task,
    /// ensuring task metadata is accurate from the start.
    fn resolve_local_version(
        path: &str,
        provided_version: Option<&str>,
    ) -> Result<(String, bool), String> {
        use crate::cache::workspace::WorkspaceHandler;
        use std::path::Path;

        // Expand path (handles ~ and relative paths)
        let expanded_path = match shellexpand::full(path) {
            Ok(p) => p,
            Err(e) => return Err(format!("Failed to expand path: {e}")),
        };
        let local_path = Path::new(expanded_path.as_ref());

        // Validate path exists
        if !local_path.exists() {
            return Err(format!(
                "Local path does not exist: {}",
                local_path.display()
            ));
        }

        let cargo_toml = local_path.join("Cargo.toml");
        if !cargo_toml.exists() {
            return Err(format!(
                "No Cargo.toml found at path: {}",
                local_path.display()
            ));
        }

        // Check if workspace
        match WorkspaceHandler::is_workspace(&cargo_toml) {
            Ok(true) => {
                // Workspace - version must be provided
                match provided_version {
                    Some(v) => Ok((v.to_string(), false)),
                    None => Err(format!(
                        "The path '{}' contains a workspace manifest. Please provide a version for caching.",
                        local_path.display()
                    )),
                }
            }
            Ok(false) => {
                // Regular package - get version from Cargo.toml
                match WorkspaceHandler::get_package_version(&cargo_toml) {
                    Ok(actual_version) => {
                        if let Some(provided) = provided_version {
                            // Validate provided version matches
                            if provided != actual_version {
                                return Err(format!(
                                    "Version mismatch: provided '{provided}' does not match actual '{actual_version}' in Cargo.toml"
                                ));
                            }
                            Ok((actual_version, false))
                        } else {
                            // Auto-detected version
                            Ok((actual_version, true))
                        }
                    }
                    Err(e) => Err(format!("Failed to read version from Cargo.toml: {e}")),
                }
            }
            Err(e) => Err(format!("Failed to check workspace status: {e}")),
        }
    }

    /// Unified cache_crate method that accepts all source types
    ///
    /// Validates parameters, spawns async task, and returns immediately with task ID.
    /// Returns JSON-formatted [`CacheTaskStartedOutput`] for structured monitoring.
    pub async fn cache_crate(&self, params: CacheCrateParams) -> String {
        // Validate and extract source details for task creation
        let (crate_name, version, source_details) = match params.source_type.as_str() {
            "cratesio" => {
                let version = match &params.version {
                    Some(v) => v.clone(),
                    None => {
                        return "# Error\n\nMissing required parameter 'version' for source_type='cratesio'".to_string();
                    }
                };
                (params.crate_name.clone(), version, None)
            }
            "github" => {
                let github_url = match &params.github_url {
                    Some(url) => url.clone(),
                    None => {
                        return "# Error\n\nMissing required parameter 'github_url' for source_type='github'".to_string();
                    }
                };

                match (&params.branch, &params.tag) {
                    (Some(_), Some(_)) => {
                        return "# Error\n\nOnly one of 'branch' or 'tag' can be specified for source_type='github', not both".to_string();
                    }
                    (None, None) => {
                        return "# Error\n\nEither 'branch' or 'tag' must be specified for source_type='github'".to_string();
                    }
                    _ => {}
                }

                let version = params
                    .branch
                    .clone()
                    .or_else(|| params.tag.clone())
                    .unwrap();
                let ref_type = if params.branch.is_some() {
                    "branch"
                } else {
                    "tag"
                };
                let details = format!("{github_url}, {ref_type}: {version}");
                (params.crate_name.clone(), version, Some(details))
            }
            "local" => {
                let path = match &params.path {
                    Some(p) => p.clone(),
                    None => {
                        return "# Error\n\nMissing required parameter 'path' for source_type='local'".to_string();
                    }
                };

                // Resolve version synchronously before creating task (fixes bug #2)
                let (version, auto_detected) =
                    match Self::resolve_local_version(&path, params.version.as_deref()) {
                        Ok(result) => result,
                        Err(error_msg) => {
                            return format!("# Error\n\n{error_msg}");
                        }
                    };

                // Add auto-detection note to source details
                let details = if auto_detected {
                    format!("{path} (version auto-detected from Cargo.toml)")
                } else {
                    path
                };

                (params.crate_name.clone(), version, Some(details))
            }
            _ => {
                return format!(
                    "# Error\n\nInvalid source_type '{}'. Must be one of: 'cratesio', 'github', 'local'",
                    params.source_type
                );
            }
        };

        // Create task
        let task = self
            .task_manager
            .create_task(
                crate_name,
                version,
                params.source_type.clone(),
                source_details,
            )
            .await;

        // Update status to InProgress before returning (fixes race condition bug #1)
        self.task_manager
            .update_status(&task.task_id, TaskStatus::InProgress)
            .await;

        // Spawn background task
        let cache = self.cache.clone();
        let task_manager = self.task_manager.clone();
        let task_id = task.task_id.clone();
        let cancellation_token = task.cancellation_token.clone();
        let params = params.clone(); // Clone params for the spawned task

        tokio::spawn(async move {
            // Build CrateSource from params
            let crate_source = Self::params_to_source(&params);

            // Run the caching operation
            let cache_guard = cache.write().await;

            // Set initial stage - Downloading
            task_manager
                .update_stage(&task_id, CachingStage::Downloading)
                .await;

            // Check for cancellation before starting
            if cancellation_token.is_cancelled() {
                task_manager
                    .update_status(&task_id, TaskStatus::Cancelled)
                    .await;
                return;
            }

            // Pass task manager and task ID to enable real progress tracking
            let json_response = cache_guard
                .cache_crate_with_source(
                    crate_source,
                    Some(task_manager.clone()),
                    Some(task_id.clone()),
                )
                .await;
            drop(cache_guard); // Release lock

            // Check for cancellation after caching
            if cancellation_token.is_cancelled() {
                task_manager
                    .update_status(&task_id, TaskStatus::Cancelled)
                    .await;
                return;
            }

            // Parse result and update task status
            match serde_json::from_str::<CacheCrateOutput>(&json_response) {
                Ok(output) => match output {
                    CacheCrateOutput::Success { .. } | CacheCrateOutput::PartialSuccess { .. } => {
                        task_manager
                            .update_status(&task_id, TaskStatus::Completed)
                            .await;
                    }
                    CacheCrateOutput::WorkspaceDetected { .. } => {
                        task_manager
                            .set_error(
                                &task_id,
                                "Workspace detected. Please specify member(s) to cache."
                                    .to_string(),
                            )
                            .await;
                    }
                    CacheCrateOutput::Error { error } => {
                        task_manager.set_error(&task_id, error).await;
                    }
                },
                Err(_) => {
                    task_manager
                        .set_error(&task_id, "Failed to parse cache response".to_string())
                        .await;
                }
            }
        });

        // Return JSON formatted task started response
        let output = CacheTaskStartedOutput {
            task_id: task.task_id.clone(),
            crate_name: task.crate_name.clone(),
            version: task.version.clone(),
            source_type: task.source_type.clone(),
            source_details: task.source_details.clone(),
            status: "in_progress".to_string(),
            message: format!(
                "Caching task started for {}-{}. Use cache_operations to monitor progress.",
                task.crate_name, task.version
            ),
        };
        output.to_json()
    }

    /// Helper to convert CacheCrateParams to CrateSource
    fn params_to_source(params: &CacheCrateParams) -> CrateSource {
        match params.source_type.as_str() {
            "cratesio" => CrateSource::CratesIO(CacheCrateFromCratesIOParams {
                crate_name: params.crate_name.clone(),
                version: params.version.clone().unwrap(),
                members: params.members.clone(),
                update: params.update,
            }),
            "github" => CrateSource::GitHub(CacheCrateFromGitHubParams {
                crate_name: params.crate_name.clone(),
                github_url: params.github_url.clone().unwrap(),
                branch: params.branch.clone(),
                tag: params.tag.clone(),
                members: params.members.clone(),
                update: params.update,
            }),
            "local" => CrateSource::LocalPath(CacheCrateFromLocalParams {
                crate_name: params.crate_name.clone(),
                version: params.version.clone(),
                path: params.path.clone().unwrap(),
                members: params.members.clone(),
                update: params.update,
            }),
            _ => unreachable!("Invalid source type should have been caught earlier"),
        }
    }

    /// Unified cache_operations method for managing and monitoring caching tasks
    ///
    /// Returns markdown-formatted text optimized for LLM consumption
    pub async fn cache_operations(&self, params: CacheOperationsParams) -> String {
        // Handle cancel action
        if params.cancel {
            let Some(task_id) = &params.task_id else {
                return "# Error\n\nCannot cancel without specifying a task_id.".to_string();
            };

            return match self.task_manager.cancel_task(task_id).await {
                Some(task) => task_formatter::format_cancel_result(&task),
                None => format!("# Error\n\nTask `{task_id}` not found."),
            };
        }

        // Handle clear action
        if params.clear {
            return if let Some(task_id) = &params.task_id {
                // Clear specific task
                match self.task_manager.get_task(task_id).await {
                    Some(task) if task.is_terminal() => {
                        self.task_manager.remove_task(task_id).await;
                        task_formatter::format_clear_result(vec![task])
                    }
                    Some(_) => format!(
                        "# Error\n\nCannot clear task `{task_id}` because it is still in progress. Cancel it first or wait for completion."
                    ),
                    None => format!("# Error\n\nTask `{task_id}` not found."),
                }
            } else {
                // Clear all terminal tasks
                let cleared = self.task_manager.clear_terminal_tasks().await;
                task_formatter::format_clear_result(cleared)
            };
        }

        // Handle query operations
        if let Some(task_id) = &params.task_id {
            // Get specific task
            match self.task_manager.get_task(task_id).await {
                Some(task) => task_formatter::format_single_task(&task),
                None => format!("# Error\n\nTask `{task_id}` not found."),
            }
        } else {
            // List all tasks with optional filter
            let status_filter = params
                .status_filter
                .as_ref()
                .and_then(|s| match s.as_str() {
                    "pending" => Some(TaskStatus::Pending),
                    "in_progress" => Some(TaskStatus::InProgress),
                    "completed" => Some(TaskStatus::Completed),
                    "failed" => Some(TaskStatus::Failed),
                    "cancelled" => Some(TaskStatus::Cancelled),
                    _ => None,
                });

            let tasks = self.task_manager.list_tasks(status_filter.as_ref()).await;
            task_formatter::format_task_list(tasks)
        }
    }
}
