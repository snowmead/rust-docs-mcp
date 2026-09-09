use std::sync::Arc;
use tokio::sync::RwLock;

use rmcp::schemars;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::cache::CrateCache;
use crate::deps::{
    outputs::{CrateIdentifier, Dependency, DepsErrorOutput, GetDependenciesOutput},
    process_cargo_metadata,
};

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
    #[schemars(
        description = "Select the same features used when caching. An explicit list disables defaults unless no_default_features=false. Omitted options use all-features, defaults, then no-defaults fallback. Item IDs belong to the selected variant."
    )]
    pub features: Option<Vec<String>>,
    pub no_default_features: Option<bool>,
    pub all_features: Option<bool>,
}

#[derive(Debug, Clone)]
pub struct DepsTools {
    cache: Arc<RwLock<CrateCache>>,
}

impl DepsTools {
    pub fn new(cache: Arc<RwLock<CrateCache>>) -> Self {
        Self { cache }
    }

    pub async fn get_dependencies(
        &self,
        params: GetDependenciesParams,
    ) -> Result<GetDependenciesOutput, DepsErrorOutput> {
        let guard = self.cache.write().await;
        let cache = guard
            .with_features(
                params.features.clone(),
                params.no_default_features,
                params.all_features,
            )
            .map_err(|e| DepsErrorOutput::new(e.to_string()))?;

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
                    .load_member_dependencies(
                        &params.crate_name,
                        &params.version,
                        params.member.as_deref(),
                    )
                    .await
                {
                    Ok(metadata) => {
                        let source = cache
                            .get_source_path(&params.crate_name, &params.version)
                            .map_err(|e| DepsErrorOutput::new(e.to_string()))?;
                        let manifest = params
                            .member
                            .as_ref()
                            .map(|m| source.join(m))
                            .unwrap_or(source)
                            .join("Cargo.toml");
                        let canonical = std::fs::canonicalize(&manifest)
                            .map_err(|e| DepsErrorOutput::new(e.to_string()))?;
                        let package = metadata["packages"]
                            .as_array()
                            .and_then(|packages| {
                                packages.iter().find(|package| {
                                    package["manifest_path"]
                                        .as_str()
                                        .and_then(|p| std::fs::canonicalize(p).ok())
                                        .as_ref()
                                        == Some(&canonical)
                                })
                            })
                            .ok_or_else(|| {
                                DepsErrorOutput::new(
                                    "Selected package not found in dependency metadata".to_owned(),
                                )
                            })?;
                        match process_cargo_metadata(
                            &metadata,
                            package["name"].as_str().unwrap_or(&params.crate_name),
                            package["version"].as_str().unwrap_or(&params.version),
                            params.include_tree.unwrap_or(false),
                            params.filter.as_deref(),
                        ) {
                            Ok(dep_info) => Ok(GetDependenciesOutput {
                                crate_info: CrateIdentifier {
                                    name: dep_info.crate_info.name,
                                    version: dep_info.crate_info.version,
                                },
                                direct_dependencies: dep_info
                                    .direct_dependencies
                                    .into_iter()
                                    .map(|d| Dependency {
                                        name: d.name,
                                        version_req: d.version_req,
                                        resolved_version: d.resolved_version,
                                        kind: d.kind,
                                        optional: d.optional,
                                        features: d.features,
                                        target: d.target,
                                    })
                                    .collect(),
                                dependency_tree: dep_info.dependency_tree,
                                total_dependencies: dep_info.total_dependencies,
                            }),
                            Err(e) => Err(DepsErrorOutput::new(format!(
                                "Failed to process dependency metadata: {e}"
                            ))),
                        }
                    }
                    Err(e) => Err(DepsErrorOutput::new(format!(
                        "Dependencies not available for {}-{}. Error: {}",
                        params.crate_name, params.version, e
                    ))),
                }
            }
            Err(e) => Err(DepsErrorOutput::new(format!("Failed to cache crate: {e}"))),
        }
    }
}
