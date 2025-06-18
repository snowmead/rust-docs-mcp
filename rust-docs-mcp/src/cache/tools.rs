use std::sync::Arc;
use tokio::sync::Mutex;

use rmcp::schemars;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::CrateCache;

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
        format!("{:.0} {}", size, unit)
    } else {
        format!("{:.2} {}", size, unit)
    }
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct CacheCrateParams {
    #[schemars(description = "The name of the crate to cache")]
    pub crate_name: String,
    #[schemars(description = "The version of the crate to cache")]
    pub version: String,
    #[schemars(
        description = "Optional source for the crate. Supports three formats:\n- GitHub URLs: https://github.com/user/repo or https://github.com/user/repo/tree/branch/path/to/crate\n- Local paths: /absolute/path, ~/home/path, ../relative/path, or ./current/path\n- If not provided, defaults to crates.io"
    )]
    pub source: Option<String>,
    #[schemars(
        description = "Optional list of workspace members to cache. If the crate is a workspace and this is not provided, the tool will return a list of available members. Specify member paths relative to the workspace root (e.g., [\"crates/rmcp\", \"crates/rmcp-macros\"])."
    )]
    pub members: Option<Vec<String>>,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
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

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
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

    pub async fn cache_crate(&self, params: CacheCrateParams) -> String {
        let cache = self.cache.lock().await;

        // If members are specified, cache those specific workspace members
        if let Some(members) = &params.members {
            let mut results = Vec::new();
            let mut errors = Vec::new();

            for member in members {
                match cache
                    .ensure_workspace_member_docs(
                        &params.crate_name,
                        &params.version,
                        params.source.as_deref(),
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
                    "crate": params.crate_name,
                    "version": params.version,
                    "members": members,
                    "results": results
                })
                .to_string();
            } else {
                return serde_json::json!({
                    "status": "partial_success",
                    "message": format!("Cached {} members with {} errors", results.len(), errors.len()),
                    "crate": params.crate_name,
                    "version": params.version,
                    "members": members,
                    "results": results,
                    "errors": errors
                })
                .to_string();
            }
        }

        // First, download the crate if not already cached
        let source_path = match cache
            .download_or_copy_crate(
                &params.crate_name,
                &params.version,
                params.source.as_deref(),
            )
            .await
        {
            Ok(path) => path,
            Err(e) => return format!(r#"{{"error": "Failed to download crate: {}"}}"#, e),
        };

        // Check if it's a workspace
        let cargo_toml_path = source_path.join("Cargo.toml");
        match cache.is_workspace(&cargo_toml_path) {
            Ok(true) => {
                // It's a workspace, get the members
                match cache.get_workspace_members(&cargo_toml_path) {
                    Ok(members) => {
                        serde_json::json!({
                            "status": "workspace_detected",
                            "message": "This is a workspace crate. Please specify which members to cache using the 'members' parameter.",
                            "crate": params.crate_name,
                            "version": params.version,
                            "workspace_members": members,
                            "example_usage": format!(
                                "cache_crate(crate_name=\"{}\", version=\"{}\", source={:?}, members={:?})",
                                params.crate_name,
                                params.version,
                                params.source,
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
                match cache
                    .ensure_crate_docs(
                        &params.crate_name,
                        &params.version,
                        params.source.as_deref(),
                    )
                    .await
                {
                    Ok(_) => serde_json::json!({
                        "status": "success",
                        "message": format!("Successfully cached {}-{}", params.crate_name, params.version),
                        "crate": params.crate_name,
                        "version": params.version
                    })
                    .to_string(),
                    Err(e) => {
                        format!(r#"{{"error": "Failed to cache crate: {}"}}"#, e)
                    }
                }
            }
            Err(_e) => {
                // Error checking workspace status, try normal caching anyway
                match cache
                    .ensure_crate_docs(
                        &params.crate_name,
                        &params.version,
                        params.source.as_deref(),
                    )
                    .await
                {
                    Ok(_) => serde_json::json!({
                        "status": "success",
                        "message": format!("Successfully cached {}-{}", params.crate_name, params.version),
                        "crate": params.crate_name,
                        "version": params.version
                    })
                    .to_string(),
                    Err(e) => {
                        format!(r#"{{"error": "Failed to cache crate: {}"}}"#, e)
                    }
                }
            }
        }
    }

    pub async fn remove_crate(&self, crate_name: String, version: String) -> String {
        let cache = self.cache.lock().await;
        match cache.remove_crate(&crate_name, &version).await {
            Ok(_) => serde_json::json!({
                "status": "success",
                "message": format!("Successfully removed {}-{}", crate_name, version),
                "crate": crate_name,
                "version": version
            })
            .to_string(),
            Err(e) => {
                format!(r#"{{"error": "Failed to remove crate: {}"}}"#, e)
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
                    format!(r#"{{"error": "Failed to serialize cached crates: {}"}}"#, e)
                })
            }
            Err(e) => {
                format!(r#"{{"error": "Failed to list cached crates: {}"}}"#, e)
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
                format!(r#"{{"error": "Failed to get cached versions: {}"}}"#, e)
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
                            "error": format!("Failed to load metadata: {}", e)
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
                    let member_name = member_path.split('/').last().unwrap_or(&member_path);
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
                                        "error": format!("Failed to load member metadata: {}", e)
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
