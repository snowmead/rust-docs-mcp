use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::Mutex;

use rmcp::schemars;
use serde::{Deserialize, Serialize};

use crate::cache::CrateCache;

#[derive(Debug, Serialize, Deserialize, schemars::JsonSchema)]
pub struct AnalyzeCrateStructureParams {
    #[schemars(description = "The name of the crate")]
    pub crate_name: String,

    #[schemars(description = "The version of the crate")]
    pub version: String,

    #[schemars(
        description = "For workspace crates, specify the member path (e.g., 'crates/rmcp')"
    )]
    pub member: Option<String>,

    #[schemars(description = "Process only this package's library")]
    pub lib: Option<bool>,

    #[schemars(description = "Process only the specified binary")]
    pub bin: Option<String>,

    #[schemars(description = "Do not activate the default feature")]
    pub no_default_features: Option<bool>,

    #[schemars(description = "Activate all available features")]
    pub all_features: Option<bool>,

    #[schemars(
        description = "List of features to activate. This will be ignored if all_features is provided"
    )]
    pub features: Option<Vec<String>>,

    #[schemars(description = "Analyze for target triple")]
    pub target: Option<String>,

    #[schemars(description = "Analyze with cfg(test) enabled (i.e as if built via cargo test)")]
    pub cfg_test: Option<bool>,

    #[schemars(description = "Filter out functions (e.g. fns, async fns, const fns) from tree")]
    pub no_fns: Option<bool>,

    #[schemars(description = "Filter out traits (e.g. trait, unsafe trait) from tree")]
    pub no_traits: Option<bool>,

    #[schemars(description = "Filter out types (e.g. structs, unions, enums) from tree")]
    pub no_types: Option<bool>,

    #[schemars(description = "The sorting order to use (e.g. name, visibility, kind)")]
    pub sort_by: Option<String>,

    #[schemars(description = "Reverses the sorting order")]
    pub sort_reversed: Option<bool>,

    #[schemars(description = "Focus the graph on a particular path or use-tree's environment")]
    pub focus_on: Option<String>,

    #[schemars(
        description = "The maximum depth of the generated graph relative to the crate's root node, or nodes selected by 'focus_on'"
    )]
    pub max_depth: Option<usize>,
}

#[derive(Debug, Clone)]
pub struct AnalysisTools {
    cache: Arc<Mutex<CrateCache>>,
}

impl AnalysisTools {
    pub fn new(cache: Arc<Mutex<CrateCache>>) -> Self {
        Self { cache }
    }

    pub async fn structure(&self, params: AnalyzeCrateStructureParams) -> String {
        let cache = self.cache.lock().await;

        // Ensure the crate source is available (without requiring docs)
        match cache
            .ensure_crate_or_member_source(
                &params.crate_name,
                &params.version,
                params.member.as_deref(),
                None, // Use default source
            )
            .await
        {
            Ok(source_path) => {
                // The source_path already points to the correct location
                // (either the crate root or the member directory)
                let manifest_path = source_path.join("Cargo.toml");

                // Extract the package name for workspace members
                let package = params.member.as_ref().map(|member| member.split('/').next_back().unwrap_or(member).to_string());

                drop(cache); // Release the lock before the blocking operation

                // Run the analysis
                analyze_with_cargo_modules(manifest_path, package, params).await
            }
            Err(e) => {
                format!(
                    r#"{{"error": "Failed to ensure crate source is available: {e}"}}"#
                )
            }
        }
    }
}

async fn analyze_with_cargo_modules(
    manifest_path: PathBuf,
    package: Option<String>,
    params: AnalyzeCrateStructureParams,
) -> String {
    use cargo_modules::{
        analyzer::LoadOptions,
        options::{GeneralOptions, ProjectOptions},
    };

    let general_options = GeneralOptions { verbose: false };

    let project_options = ProjectOptions {
        lib: params.lib.unwrap_or(false),
        bin: params.bin,
        package,
        no_default_features: params.no_default_features.unwrap_or(false),
        all_features: params.all_features.unwrap_or(false),
        features: params.features.unwrap_or_default(),
        target: params.target,
        manifest_path,
    };

    let load_options = LoadOptions {
        cfg_test: params.cfg_test.unwrap_or(false),
        sysroot: false,
    };

    // Run the analysis synchronously in a blocking task
    let result = tokio::task::spawn_blocking(move || -> Result<String, String> {
        // Load the workspace
        let (crate_id, analysis_host, _vfs, _edition) = cargo_modules::analyzer::load_workspace(
            &general_options,
            &project_options,
            &load_options,
        )
        .map_err(|e| format!("Failed to load workspace: {e}"))?;

        let db = analysis_host.raw_database();

        // Build the tree using cargo_modules internal logic
        use cargo_modules::tree::TreeBuilder;
        let builder = TreeBuilder::new(db, crate_id);
        let tree = builder
            .build()
            .map_err(|e| format!("Failed to build tree: {e}"))?;

        // Format the tree structure as JSON
        let result = serde_json::json!({
            "status": "success",
            "message": "Module structure analysis completed",
            "tree": format_tree(&tree),
        });

        Ok(serde_json::to_string_pretty(&result).unwrap())
    })
    .await;

    match result {
        Ok(Ok(output)) => output,
        Ok(Err(e)) => format!(r#"{{"error": "Analysis failed: {e}"}}"#),
        Err(e) => format!(r#"{{"error": "Task failed: {e}"}}"#),
    }
}

// Helper function to format the tree structure
fn format_tree<N: std::fmt::Debug>(tree: &cargo_modules::tree::Tree<N>) -> serde_json::Value {
    fn format_node<N: std::fmt::Debug>(node: &cargo_modules::tree::Tree<N>) -> serde_json::Value {
        let node_str = format!("{:?}", node.node);

        if node.subtrees.is_empty() {
            serde_json::json!({
                "node": node_str,
            })
        } else {
            serde_json::json!({
                "node": node_str,
                "children": node.subtrees.iter().map(format_node).collect::<Vec<_>>(),
            })
        }
    }

    format_node(tree)
}
