//! One-shot CLI adapter over the shared [`RustDocsRuntime`](crate::runtime::RustDocsRuntime).
//!
//! Takes a tool name and optional JSON params, runs the operation through
//! the runtime, and returns the result string to be printed on stdout.

use std::path::PathBuf;

use anyhow::{Context, Result, bail};

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

/// Result of a one-shot CLI tool invocation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CliCallResult {
    /// Tool output to emit on stdout.
    pub output: String,
    /// Whether the output represents a tool-level failure.
    pub failed: bool,
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
    Ok(call_with_status(cache_dir, tool, params_json).await?.output)
}

/// Run the requested tool and return both output and failure status.
///
/// Tool-level failures are represented as JSON on stdout. This wrapper lets
/// binary callers preserve that stdout while still choosing a non-zero exit.
pub async fn call_with_status(
    cache_dir: Option<PathBuf>,
    tool: CliTool,
    params_json: Option<String>,
) -> Result<CliCallResult> {
    let output = call_output(cache_dir, tool, params_json).await?;
    let failed = output_indicates_error(&output);
    Ok(CliCallResult { output, failed })
}

async fn call_output(
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
            ensure_empty_params("list-cached-crates", params_json)?;
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

fn ensure_empty_params(tool_name: &str, json: Option<String>) -> Result<()> {
    let Some(json) = json else {
        return Ok(());
    };

    let value: serde_json::Value = serde_json::from_str(&json)
        .with_context(|| format!("Failed to parse params for {tool_name}"))?;

    match value {
        serde_json::Value::Object(map) if map.is_empty() => Ok(()),
        _ => bail!("{tool_name} does not accept parameters; omit --params or pass {{}}"),
    }
}

/// Return true when a tool output JSON object represents an error.
///
/// The existing output types encode failures either as `{ "error": ... }`
/// or as a tagged cache response `{ "status": "error", ... }`.
pub fn output_indicates_error(output: &str) -> bool {
    let Ok(serde_json::Value::Object(object)) = serde_json::from_str(output) else {
        return false;
    };

    object
        .get("status")
        .and_then(serde_json::Value::as_str)
        .is_some_and(|status| status == "error")
        || object.contains_key("error")
}
