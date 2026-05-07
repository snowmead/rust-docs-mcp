//! One-shot CLI adapter over the shared [`RustDocsRuntime`](crate::runtime::RustDocsRuntime).
//!
//! Takes a tool name and optional JSON params, runs the operation through
//! the runtime, and returns the result string to be printed on stdout.

use std::path::PathBuf;

use anyhow::{Context, Result};

use crate::analysis::tools::AnalyzeCrateStructureParams;
use crate::cache::tools::{CacheCrateParams, ListCrateVersionsParams};
use crate::deps::tools::GetDependenciesParams;
use crate::docs::tools::{
    GetItemDetailsParams, GetItemDocsParams, GetItemSourceParams, ListItemsParams,
    SearchItemsParams, SearchItemsPreviewParams,
};
use crate::runtime::RustDocsRuntime;
use crate::search::tools::SearchItemsFuzzyParams;

/// Recognised one-shot tools (subset of MCP tools).
#[derive(Debug, Clone, clap::ValueEnum)]
#[clap(rename_all = "kebab-case")]
pub enum CliTool {
    /// Blocking cache operation
    CacheCrate,
    /// Fuzzy search with Tantivy
    SearchItemsFuzzy,
    /// Exact search — preview only
    SearchItemsPreview,
    /// Exact search — full details
    SearchItems,
    /// List all items in a crate
    ListCrateItems,
    /// Full details for an item by id
    GetItemDetails,
    /// Documentation text only for an item
    GetItemDocs,
    /// Source code for an item
    GetItemSource,
    /// List all locally cached crates
    ListCachedCrates,
    /// List locally cached versions of a crate
    ListCrateVersions,
    /// Get dependency information
    GetDependencies,
    /// View hierarchical structure tree
    Structure,
}

/// Run the requested tool and return its output string.
///
/// `params_json` may be `None` for parameterless tools; otherwise it
/// should be the JSON representation of the tool's parameter struct.
pub async fn call(
    cache_dir: Option<PathBuf>,
    tool: CliTool,
    params_json: Option<String>,
) -> Result<String> {
    let runtime = RustDocsRuntime::new(cache_dir)?;

    match tool {
        CliTool::CacheCrate => {
            let params: CacheCrateParams = parse_params("params", params_json)?;
            Ok(runtime.cache_crate_blocking(params).await)
        }
        CliTool::SearchItemsFuzzy => {
            let params: SearchItemsFuzzyParams = parse_params("params", params_json)?;
            Ok(runtime.search_items_fuzzy(params).await)
        }
        CliTool::SearchItemsPreview => {
            let params: SearchItemsPreviewParams = parse_params("params", params_json)?;
            Ok(runtime.search_items_preview(params).await)
        }
        CliTool::SearchItems => {
            let params: SearchItemsParams = parse_params("params", params_json)?;
            Ok(runtime.search_items(params).await)
        }
        CliTool::ListCrateItems => {
            let params: ListItemsParams = parse_params("params", params_json)?;
            Ok(runtime.list_crate_items(params).await)
        }
        CliTool::GetItemDetails => {
            let params: GetItemDetailsParams = parse_params("params", params_json)?;
            Ok(runtime.get_item_details(params).await)
        }
        CliTool::GetItemDocs => {
            let params: GetItemDocsParams = parse_params("params", params_json)?;
            Ok(runtime.get_item_docs(params).await)
        }
        CliTool::GetItemSource => {
            let params: GetItemSourceParams = parse_params("params", params_json)?;
            Ok(runtime.get_item_source(params).await)
        }
        CliTool::ListCachedCrates => {
            // No params needed
            Ok(runtime.list_cached_crates().await)
        }
        CliTool::ListCrateVersions => {
            let params: ListCrateVersionsParams = parse_params("params", params_json)?;
            Ok(runtime.list_crate_versions(params).await)
        }
        CliTool::GetDependencies => {
            let params: GetDependenciesParams = parse_params("params", params_json)?;
            Ok(runtime.get_dependencies(params).await)
        }
        CliTool::Structure => {
            let params: AnalyzeCrateStructureParams = parse_params("params", params_json)?;
            Ok(runtime.structure(params).await)
        }
    }
}

/// Parse JSON params (or `{}` if missing) into `T`, with a helpful
/// error message wrapping the parameter name.
fn parse_params<T: serde::de::DeserializeOwned>(name: &str, json: Option<String>) -> Result<T> {
    let json = json.unwrap_or_else(|| "{}".to_string());
    serde_json::from_str::<T>(&json)
        .with_context(|| format!("Failed to parse {name} as {}", std::any::type_name::<T>()))
}
