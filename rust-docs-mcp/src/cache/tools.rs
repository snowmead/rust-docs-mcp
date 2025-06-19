use std::sync::Arc;
use tokio::sync::Mutex;

use rmcp::schemars;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::CrateCache;
use crate::cache::service::CrateSource;

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
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct CacheCrateFromGitHubParams {
    #[schemars(description = "The name of the crate")]
    pub crate_name: String,
    #[schemars(description = "GitHub repository URL (e.g., https://github.com/user/repo)")]
    pub github_url: String,
    #[schemars(description = "Branch to use (e.g., 'main', 'develop'). Only one of branch or tag can be specified.")]
    pub branch: Option<String>,
    #[schemars(description = "Tag to use (e.g., 'v1.0.0', '0.2.1'). Only one of branch or tag can be specified.")]
    pub tag: Option<String>,
    #[schemars(
        description = "Optional list of workspace members to cache. If the crate is a workspace and this is not provided, the tool will return a list of available members. Specify member paths relative to the workspace root (e.g., [\"crates/rmcp\", \"crates/rmcp-macros\"])."
    )]
    pub members: Option<Vec<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct CacheCrateFromLocalParams {
    #[schemars(description = "The name of the crate")]
    pub crate_name: String,
    #[schemars(description = "The version to use for caching (e.g., '0.1.0')")]
    pub version: String,
    #[schemars(
        description = "Local file system path. Supports absolute paths (/path), home paths (~/path), and relative paths (./path, ../path)"
    )]
    pub path: String,
    #[schemars(
        description = "Optional list of workspace members to cache. If the crate is a workspace and this is not provided, the tool will return a list of available members. Specify member paths relative to the workspace root (e.g., [\"crates/rmcp\", \"crates/rmcp-macros\"])."
    )]
    pub members: Option<Vec<String>>,
}

/// Format bytes into human-readable string
fn format_bytes(bytes: u64) -> String {
    const UNITS: &[&str] = &["B", "KB", "MB", "GB", "TB"];
    if bytes == 0 {
        return "0 B".to_string();
    }

    let base = 1024_f64;
    let exponent = (bytes as f64).ln() / base.ln();
    let exponent = exponent.floor() as usize;

    let unit = UNITS.get(exponent).unwrap_or(&"TB");
    let size = bytes as f64 / base.powi(exponent as i32);

    if size.fract() == 0.0 {
        format!("{size:.0} {unit}")
    } else {
        format!("{size:.2} {unit}")
    }
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

#[derive(Debug, Clone)]
pub struct CacheTools {
    cache: Arc<Mutex<CrateCache>>,
}

impl CacheTools {
    pub fn new(cache: Arc<Mutex<CrateCache>>) -> Self {
        Self { cache }
    }

    pub async fn cache_crate_from_cratesio(&self, params: CacheCrateFromCratesIOParams) -> String {
        let cache = self.cache.lock().await;
        let source = CrateSource::CratesIO(params);
        cache.cache_crate_with_source(source).await
    }

    pub async fn cache_crate_from_github(&self, params: CacheCrateFromGitHubParams) -> String {
        // Validate that only one of branch or tag is provided
        match (&params.branch, &params.tag) {
            (Some(_), Some(_)) => {
                return serde_json::json!({
                    "error": "Only one of 'branch' or 'tag' can be specified, not both"
                }).to_string();
            }
            (None, None) => {
                return serde_json::json!({
                    "error": "Either 'branch' or 'tag' must be specified"
                }).to_string();
            }
            _ => {} // Valid: exactly one is provided
        }
        
        let cache = self.cache.lock().await;
        let source = CrateSource::GitHub(params);
        cache.cache_crate_with_source(source).await
    }

    pub async fn cache_crate_from_local(&self, params: CacheCrateFromLocalParams) -> String {
        let cache = self.cache.lock().await;
        let source = CrateSource::LocalPath(params);
        cache.cache_crate_with_source(source).await
    }

    pub async fn remove_crate(&self, crate_name: String, version: String) -> String {
        let cache = self.cache.lock().await;
        match cache.remove_crate(&crate_name, &version).await {
            Ok(_) => serde_json::json!({
                "status": "success",
                "message": format!("Successfully removed {crate_name}-{version}"),
                "crate": crate_name,
                "version": version
            })
            .to_string(),
            Err(e) => {
                format!(r#"{{"error": "Failed to remove crate: {e}"}}"#)
            }
        }
    }

    pub async fn list_cached_crates(&self) -> String {
        let cache = self.cache.lock().await;
        match cache.list_all_cached_crates().await {
            Ok(mut crates) => {
                // Sort by name and version for consistent output
                crates.sort_by(|a, b| {
                    a.name.cmp(&b.name).then_with(|| b.version.cmp(&a.version)) // Newer versions first
                });

                // Calculate total size
                let total_size_bytes: u64 = crates.iter().map(|c| c.size_bytes).sum();

                // Group by crate name for better organization
                let mut grouped: std::collections::HashMap<String, Vec<_>> =
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

                    let mut version_info = serde_json::json!({
                        "version": crate_meta.version,
                        "cached_at": crate_meta.cached_at,
                        "doc_generated": crate_meta.doc_generated,
                        "size_bytes": crate_meta.size_bytes,
                        "size_human": format_bytes(crate_meta.size_bytes)
                    });

                    // Add members field if there are any
                    if let Some(member_list) = members {
                        version_info["members"] = serde_json::json!(member_list);
                    }

                    grouped
                        .entry(crate_name)
                        .or_insert_with(Vec::new)
                        .push(version_info);
                }

                let response = serde_json::json!({
                    "cached_crates": grouped,
                    "total_crates": grouped.len(),
                    "total_versions": grouped.values().map(|v| v.len()).sum::<usize>(),
                    "total_size_bytes": total_size_bytes,
                    "total_size_human": format_bytes(total_size_bytes)
                });
                serde_json::to_string_pretty(&response).unwrap_or_else(|e| {
                    format!(r#"{{"error": "Failed to serialize cached crates: {e}"}}"#)
                })
            }
            Err(e) => {
                format!(r#"{{"error": "Failed to list cached crates: {e}"}}"#)
            }
        }
    }

    pub async fn list_crate_versions(&self, crate_name: String) -> String {
        let cache = self.cache.lock().await;
        match cache.get_cached_versions(&crate_name).await {
            Ok(versions) => serde_json::json!({
                "crate": crate_name,
                "versions": versions
            })
            .to_string(),
            Err(e) => {
                format!(r#"{{"error": "Failed to get cached versions: {e}"}}"#)
            }
        }
    }

    pub async fn get_crates_metadata(&self, params: GetCratesMetadataParams) -> String {
        let cache = self.cache.lock().await;
        let mut results = Vec::new();

        for query in params.queries {
            let crate_name = &query.crate_name;
            let version = &query.version;

            // Check if main crate is cached
            let main_crate_result = if cache.storage.is_cached(crate_name, version) {
                match cache.storage.load_metadata(crate_name, version) {
                    Ok(metadata) => {
                        serde_json::json!({
                            "crate_name": crate_name,
                            "version": version,
                            "member": null,
                            "cached": true,
                            "cached_at": metadata.cached_at,
                            "cache_size": metadata.size_bytes
                        })
                    }
                    Err(e) => {
                        serde_json::json!({
                            "crate_name": crate_name,
                            "version": version,
                            "member": null,
                            "cached": true,
                            "error": format!("Failed to load metadata: {e}")
                        })
                    }
                }
            } else {
                serde_json::json!({
                    "crate_name": crate_name,
                    "version": version,
                    "member": null,
                    "cached": false
                })
            };
            results.push(main_crate_result);

            // Check requested members if any
            if let Some(members) = query.members {
                for member_path in members {
                    let member_name = member_path.split('/').next_back().unwrap_or(&member_path);
                    let member_result =
                        if cache
                            .storage
                            .is_member_cached(crate_name, version, &member_path)
                        {
                            match cache.storage.load_member_metadata(
                                crate_name,
                                version,
                                member_name,
                            ) {
                                Ok(metadata) => {
                                    serde_json::json!({
                                        "crate_name": crate_name,
                                        "version": version,
                                        "member": member_path,
                                        "cached": true,
                                        "cached_at": metadata.cached_at,
                                        "cache_size": metadata.size_bytes
                                    })
                                }
                                Err(e) => {
                                    serde_json::json!({
                                        "crate_name": crate_name,
                                        "version": version,
                                        "member": member_path,
                                        "cached": true,
                                        "error": format!("Failed to load member metadata: {e}")
                                    })
                                }
                            }
                        } else {
                            serde_json::json!({
                                "crate_name": crate_name,
                                "version": version,
                                "member": member_path,
                                "cached": false
                            })
                        };
                    results.push(member_result);
                }
            }
        }

        serde_json::json!({
            "metadata": results,
            "total_queried": results.len()
        })
        .to_string()
    }
}
