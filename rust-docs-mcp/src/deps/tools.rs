use std::sync::Arc;
use tokio::sync::RwLock;

use rmcp::schemars;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::cache::CrateCache;
use crate::deps::process_cargo_metadata;

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct GetDependenciesParams {
    #[schemars(description = "The name of the crate")]
    pub crate_name: String,
    #[schemars(description = "The version of the crate")]
    pub version: String,
    #[schemars(
        description = "Include the full dependency tree (default: false, only shows direct dependencies)"
    )]
    pub include_tree: Option<bool>,
    #[schemars(description = "Filter dependencies by name (partial match)")]
    pub filter: Option<String>,
    #[schemars(
        description = "For workspace crates, specify the member path (e.g., 'crates/rmcp')"
    )]
    pub member: Option<String>,
}

#[derive(Debug, Clone)]
pub struct DepsTools {
    cache: Arc<RwLock<CrateCache>>,
}

impl DepsTools {
    pub fn new(cache: Arc<RwLock<CrateCache>>) -> Self {
        Self { cache }
    }

    pub async fn get_dependencies(&self, params: GetDependenciesParams) -> String {
        let cache = self.cache.write().await;

        // First ensure the crate is cached
        match cache
            .ensure_crate_or_member_docs(
                &params.crate_name,
                &params.version,
                params.member.as_deref(),
            )
            .await
        {
            Ok(_) => {
                // Load the dependency metadata
                match cache
                    .load_dependencies(&params.crate_name, &params.version)
                    .await
                {
                    Ok(metadata) => {
                        // Process the metadata to extract dependency information
                        match process_cargo_metadata(
                            &metadata,
                            &params.crate_name,
                            &params.version,
                            params.include_tree.unwrap_or(false),
                            params.filter.as_deref(),
                        ) {
                            Ok(dep_info) => {
                                serde_json::to_string_pretty(&dep_info).unwrap_or_else(|e| {
                                    format!(
                                        r#"{{"error": "Failed to serialize dependency info: {e}"}}"#
                                    )
                                })
                            }
                            Err(e) => {
                                format!(
                                    r#"{{"error": "Failed to process dependency metadata: {e}"}}"#
                                )
                            }
                        }
                    }
                    Err(e) => {
                        format!(
                            r#"{{"error": "Dependencies not available for {}-{}. Error: {}"}}"#,
                            params.crate_name, params.version, e
                        )
                    }
                }
            }
            Err(e) => {
                format!(r#"{{"error": "Failed to cache crate: {e}"}}"#)
            }
        }
    }
}
