use std::sync::Arc;
use tokio::sync::RwLock;

use rmcp::schemars;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::cache::{
    CrateCache,
    downloader::CrateSource,
    outputs::{
        CacheCrateOutput, CrateMetadata, ErrorOutput, GetCratesMetadataOutput,
        ListCachedCratesOutput, ListCrateVersionsOutput, RemoveCrateOutput, SizeInfo, VersionInfo,
    },
    utils::format_bytes,
};

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

#[derive(Debug, Clone)]
pub struct CacheTools {
    cache: Arc<RwLock<CrateCache>>,
}

impl CacheTools {
    pub fn new(cache: Arc<RwLock<CrateCache>>) -> Self {
        Self { cache }
    }

    pub async fn cache_crate_from_cratesio(
        &self,
        params: CacheCrateFromCratesIOParams,
    ) -> CacheCrateOutput {
        let cache = self.cache.write().await;
        let source = CrateSource::CratesIO(params);
        let json_response = cache.cache_crate_with_source(source).await;
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
        let json_response = cache.cache_crate_with_source(source).await;
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
        let json_response = cache.cache_crate_with_source(source).await;
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
}
