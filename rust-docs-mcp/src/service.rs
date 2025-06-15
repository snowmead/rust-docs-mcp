use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::Mutex;

use anyhow::Result;
use rmcp::{
    ServerHandler,
    model::{ServerCapabilities, ServerInfo},
    tool,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::cache::CrateCache;
use crate::deps::process_cargo_metadata;
use crate::docs::DocQuery;

/// Maximum size for response in bytes (roughly 25k tokens * 4 bytes/token)
const MAX_RESPONSE_SIZE: usize = 100_000;

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

#[derive(Debug, Clone)]
pub struct RustDocsService {
    cache: Arc<Mutex<CrateCache>>,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct ListItemsParams {
    #[schemars(description = "The name of the crate")]
    pub crate_name: String,
    #[schemars(description = "The version of the crate")]
    pub version: String,
    #[schemars(description = "Optional filter by item kind (e.g., 'function', 'struct', 'enum')")]
    pub kind_filter: Option<String>,
    #[schemars(description = "Maximum number of items to return (default: 100)")]
    pub limit: Option<usize>,
    #[schemars(description = "Starting position for pagination (default: 0)")]
    pub offset: Option<usize>,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct SearchItemsParams {
    #[schemars(description = "The name of the crate")]
    pub crate_name: String,
    #[schemars(description = "The version of the crate")]
    pub version: String,
    #[schemars(description = "The pattern to search for in item names")]
    pub pattern: String,
    #[schemars(description = "Maximum number of items to return (default: 100)")]
    pub limit: Option<usize>,
    #[schemars(description = "Starting position for pagination (default: 0)")]
    pub offset: Option<usize>,
    #[schemars(description = "Optional filter by item kind (e.g., 'function', 'struct', 'enum')")]
    pub kind_filter: Option<String>,
    #[schemars(description = "Optional filter by module path prefix")]
    pub path_filter: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct SearchItemsPreviewParams {
    #[schemars(description = "The name of the crate")]
    pub crate_name: String,
    #[schemars(description = "The version of the crate")]
    pub version: String,
    #[schemars(description = "The pattern to search for in item names")]
    pub pattern: String,
    #[schemars(description = "Maximum number of items to return (default: 100)")]
    pub limit: Option<usize>,
    #[schemars(description = "Starting position for pagination (default: 0)")]
    pub offset: Option<usize>,
    #[schemars(description = "Optional filter by item kind (e.g., 'function', 'struct', 'enum')")]
    pub kind_filter: Option<String>,
    #[schemars(description = "Optional filter by module path prefix")]
    pub path_filter: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct GetItemDetailsParams {
    #[schemars(description = "The name of the crate")]
    pub crate_name: String,
    #[schemars(description = "The version of the crate")]
    pub version: String,
    #[schemars(description = "The numeric ID of the item")]
    pub item_id: u32,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct GetItemDocsParams {
    #[schemars(description = "The name of the crate")]
    pub crate_name: String,
    #[schemars(description = "The version of the crate")]
    pub version: String,
    #[schemars(description = "The numeric ID of the item")]
    pub item_id: u32,
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
}

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
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct GetItemSourceParams {
    #[schemars(description = "The name of the crate")]
    pub crate_name: String,
    #[schemars(description = "The version of the crate")]
    pub version: String,
    #[schemars(description = "The numeric ID of the item")]
    pub item_id: u32,
    #[schemars(
        description = "Number of context lines to include before and after the item (default: 3)"
    )]
    pub context_lines: Option<usize>,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct AnalyzeCrateParams {
    #[schemars(description = "The name of the crate")]
    pub crate_name: String,
    #[schemars(description = "The version of the crate")]
    pub version: String,
    #[schemars(
        description = "Package name to analyze in workspace (optional for single package crates)"
    )]
    pub package: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct GetModuleDependenciesParams {
    #[schemars(description = "The name of the crate")]
    pub crate_name: String,
    #[schemars(description = "The version of the crate")]
    pub version: String,
    #[schemars(
        description = "Package name to analyze in workspace (optional for single package crates)"
    )]
    pub package: Option<String>,
    #[schemars(
        description = "Include dependency graph format (default: false, returns tree structure)"
    )]
    pub graph_format: Option<bool>,
}

#[tool(tool_box)]
impl RustDocsService {
    pub fn new(cache_dir: Option<PathBuf>) -> Result<Self> {
        Ok(Self {
            cache: Arc::new(Mutex::new(CrateCache::new(cache_dir)?)),
        })
    }

    /// Helper to check if a response might exceed size limits
    fn estimate_response_size<T: Serialize>(data: &T) -> usize {
        serde_json::to_string(data).map(|s| s.len()).unwrap_or(0)
    }

    #[tool(
        description = "List all items in a crate's documentation. Use when browsing a crate's contents without a specific search term. Returns full item details including documentation. For large crates, consider using search_items_preview for a lighter response that only includes names and types."
    )]
    pub async fn list_crate_items(&self, #[tool(aggr)] params: ListItemsParams) -> String {
        let cache = self.cache.lock().await;
        match cache
            .ensure_crate_docs(&params.crate_name, &params.version, None)
            .await
        {
            Ok(crate_data) => {
                let query = DocQuery::new(crate_data);
                let items = query.list_items(params.kind_filter.as_deref());

                let total_count = items.len();
                let limit = params.limit.unwrap_or(100);
                let offset = params.offset.unwrap_or(0);

                // Apply pagination
                let paginated_items: Vec<_> = items.into_iter().skip(offset).take(limit).collect();

                let response = serde_json::json!({
                    "items": paginated_items,
                    "pagination": {
                        "total": total_count,
                        "limit": limit,
                        "offset": offset,
                        "has_more": offset + paginated_items.len() < total_count
                    }
                });

                serde_json::to_string_pretty(&response).unwrap_or_else(|e| {
                    format!(r#"{{"error": "Failed to serialize items: {}"}}"#, e)
                })
            }
            Err(e) => {
                format!(r#"{{"error": "Failed to get crate docs: {}"}}"#, e)
            }
        }
    }

    #[tool(
        description = "Search for items by name pattern in a crate. Use when looking for specific functions, types, or modules. Returns FULL details including documentation. WARNING: May exceed token limits for large results. Use search_items_preview first for exploration, then get_item_details for specific items."
    )]
    pub async fn search_items(&self, #[tool(aggr)] params: SearchItemsParams) -> String {
        let cache = self.cache.lock().await;
        match cache
            .ensure_crate_docs(&params.crate_name, &params.version, None)
            .await
        {
            Ok(crate_data) => {
                let query = DocQuery::new(crate_data);
                let mut items = query.search_items(&params.pattern);

                // Apply kind filter if provided
                if let Some(kind_filter) = &params.kind_filter {
                    items.retain(|item| item.kind == *kind_filter);
                }

                // Apply path filter if provided
                if let Some(path_filter) = &params.path_filter {
                    items.retain(|item| {
                        let item_path = item.path.join("::");
                        item_path.starts_with(path_filter)
                    });
                }

                let total_count = items.len();
                let limit = params.limit.unwrap_or(100);
                let offset = params.offset.unwrap_or(0);

                // Apply pagination
                let mut paginated_items: Vec<_> =
                    items.into_iter().skip(offset).take(limit).collect();

                // Check response size and truncate if necessary
                let mut actual_limit = limit;
                let mut truncated = false;

                loop {
                    let test_response = serde_json::json!({
                        "items": &paginated_items,
                        "pagination": {
                            "total": total_count,
                            "limit": actual_limit,
                            "offset": offset,
                            "has_more": offset + paginated_items.len() < total_count
                        }
                    });

                    if Self::estimate_response_size(&test_response) <= MAX_RESPONSE_SIZE {
                        break;
                    }

                    // Reduce items by half if too large
                    let new_len = paginated_items.len() / 2;
                    if new_len == 0 {
                        break; // Can't reduce further
                    }
                    paginated_items.truncate(new_len);
                    actual_limit = new_len;
                    truncated = true;
                }

                let mut response = serde_json::json!({
                    "items": paginated_items,
                    "pagination": {
                        "total": total_count,
                        "limit": actual_limit,
                        "offset": offset,
                        "has_more": offset + paginated_items.len() < total_count
                    }
                });

                if truncated {
                    response["warning"] = serde_json::json!(
                        "Response was truncated to stay within size limits. Use smaller limit or preview mode."
                    );
                }

                serde_json::to_string_pretty(&response).unwrap_or_else(|e| {
                    format!(r#"{{"error": "Failed to serialize items: {}"}}"#, e)
                })
            }
            Err(e) => {
                format!(r#"{{"error": "Failed to get crate docs: {}"}}"#, e)
            }
        }
    }

    #[tool(
        description = "Search for items by name pattern in a crate - PREVIEW MODE. Use this FIRST when searching to avoid token limits. Returns only id, name, kind, and path. Once you find items of interest, use get_item_details to fetch full documentation. This is the recommended search method for exploration."
    )]
    pub async fn search_items_preview(
        &self,
        #[tool(aggr)] params: SearchItemsPreviewParams,
    ) -> String {
        let cache = self.cache.lock().await;
        match cache
            .ensure_crate_docs(&params.crate_name, &params.version, None)
            .await
        {
            Ok(crate_data) => {
                let query = DocQuery::new(crate_data);
                let mut items = query.search_items(&params.pattern);

                // Apply kind filter if provided
                if let Some(kind_filter) = &params.kind_filter {
                    items.retain(|item| item.kind == *kind_filter);
                }

                // Apply path filter if provided
                if let Some(path_filter) = &params.path_filter {
                    items.retain(|item| {
                        let item_path = item.path.join("::");
                        item_path.starts_with(path_filter)
                    });
                }

                let total_count = items.len();
                let limit = params.limit.unwrap_or(100);
                let offset = params.offset.unwrap_or(0);

                // Apply pagination and create preview items
                let preview_items: Vec<_> = items
                    .into_iter()
                    .skip(offset)
                    .take(limit)
                    .map(|item| {
                        serde_json::json!({
                            "id": item.id,
                            "name": item.name,
                            "kind": item.kind,
                            "path": item.path,
                        })
                    })
                    .collect();

                let response = serde_json::json!({
                    "items": preview_items,
                    "pagination": {
                        "total": total_count,
                        "limit": limit,
                        "offset": offset,
                        "has_more": offset + preview_items.len() < total_count
                    }
                });

                serde_json::to_string_pretty(&response).unwrap_or_else(|e| {
                    format!(r#"{{"error": "Failed to serialize items: {}"}}"#, e)
                })
            }
            Err(e) => {
                format!(r#"{{"error": "Failed to get crate docs: {}"}}"#, e)
            }
        }
    }

    #[tool(
        description = "Get detailed information about a specific item by ID. Use after search_items_preview to fetch full details including documentation, signatures, fields, methods, etc. The item_id comes from search results. This is the recommended way to get complete information about a specific item."
    )]
    pub async fn get_item_details(&self, #[tool(aggr)] params: GetItemDetailsParams) -> String {
        let cache = self.cache.lock().await;
        match cache
            .ensure_crate_docs(&params.crate_name, &params.version, None)
            .await
        {
            Ok(crate_data) => {
                let query = DocQuery::new(crate_data);
                match query.get_item_details(params.item_id) {
                    Ok(details) => serde_json::to_string_pretty(&details).unwrap_or_else(|e| {
                        format!(r#"{{"error": "Failed to serialize details: {}"}}"#, e)
                    }),
                    Err(e) => format!(r#"{{"error": "Item not found: {}"}}"#, e),
                }
            }
            Err(e) => {
                format!(r#"{{"error": "Failed to get crate docs: {}"}}"#, e)
            }
        }
    }

    #[tool(
        description = "Get ONLY the documentation string for a specific item. Use when you need just the docs without other details. More efficient than get_item_details if you only need the documentation text. Returns null if no documentation exists."
    )]
    pub async fn get_item_docs(&self, #[tool(aggr)] params: GetItemDocsParams) -> String {
        let cache = self.cache.lock().await;
        match cache
            .ensure_crate_docs(&params.crate_name, &params.version, None)
            .await
        {
            Ok(crate_data) => {
                let query = DocQuery::new(crate_data);
                match query.get_item_docs(params.item_id) {
                    Ok(Some(docs)) => serde_json::json!({
                        "documentation": docs
                    })
                    .to_string(),
                    Ok(None) => serde_json::json!({
                        "documentation": null,
                        "message": "No documentation available for this item"
                    })
                    .to_string(),
                    Err(e) => format!(r#"{{"error": "Failed to get docs: {}"}}"#, e),
                }
            }
            Err(e) => {
                format!(r#"{{"error": "Failed to get crate docs: {}"}}"#, e)
            }
        }
    }

    #[tool(
        description = "List all locally cached versions of a crate. Use to check what versions are available offline without downloading. Useful before calling other tools to verify if a version needs to be cached first."
    )]
    pub async fn list_crate_versions(
        &self,
        #[tool(param)]
        #[schemars(description = "The name of the crate")]
        crate_name: String,
    ) -> String {
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

    #[tool(
        description = "Download and cache a specific crate version for offline use. This happens automatically when using other tools, but use this to pre-cache crates. Useful for preparing offline access or ensuring a crate is available before searching."
    )]
    pub async fn cache_crate(&self, #[tool(aggr)] params: CacheCrateParams) -> String {
        let cache = self.cache.lock().await;
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

    #[tool(
        description = "Remove a cached crate version from local storage. Use to free up disk space or remove outdated versions. This only affects the local cache - the crate can be re-downloaded later if needed."
    )]
    pub async fn remove_crate(
        &self,
        #[tool(param)]
        #[schemars(description = "The name of the crate")]
        crate_name: String,
        #[tool(param)]
        #[schemars(description = "The version of the crate")]
        version: String,
    ) -> String {
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

    #[tool(
        description = "Get the source code for a specific item. Returns the actual source code with optional context lines. Use after finding items of interest to view their implementation. The source location is also included in get_item_details responses."
    )]
    pub async fn get_item_source(&self, #[tool(aggr)] params: GetItemSourceParams) -> String {
        let cache = self.cache.lock().await;
        let source_base_path = cache.get_source_path(&params.crate_name, &params.version);

        match cache
            .ensure_crate_docs(&params.crate_name, &params.version, None)
            .await
        {
            Ok(crate_data) => {
                let query = DocQuery::new(crate_data);
                let context_lines = params.context_lines.unwrap_or(3);

                match query.get_item_source(params.item_id, &source_base_path, context_lines) {
                    Ok(source_info) => {
                        serde_json::to_string_pretty(&source_info).unwrap_or_else(|e| {
                            format!(r#"{{"error": "Failed to serialize source info: {}"}}"#, e)
                        })
                    }
                    Err(e) => format!(r#"{{"error": "Failed to get source: {}"}}"#, e),
                }
            }
            Err(e) => {
                format!(r#"{{"error": "Failed to get crate docs: {}"}}"#, e)
            }
        }
    }

    #[tool(
        description = "List all locally cached crates with their versions and sizes. Use to see what crates are available offline and how much disk space they use. Shows cache metadata including when each crate was cached."
    )]
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
                    grouped
                        .entry(crate_meta.name.clone())
                        .or_insert_with(Vec::new)
                        .push(serde_json::json!({
                            "version": crate_meta.version,
                            "cached_at": crate_meta.cached_at,
                            "doc_generated": crate_meta.doc_generated,
                            "size_bytes": crate_meta.size_bytes,
                            "size_human": format_bytes(crate_meta.size_bytes)
                        }));
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

    #[tool(
        description = "Get dependency information for a crate. Returns direct dependencies by default, with option to include full dependency tree. Use this to understand what a crate depends on, check for version conflicts, or explore the dependency graph."
    )]
    pub async fn get_dependencies(&self, #[tool(aggr)] params: GetDependenciesParams) -> String {
        let cache = self.cache.lock().await;

        // First ensure the crate is cached
        match cache
            .ensure_crate_docs(&params.crate_name, &params.version, None)
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
                                        r#"{{"error": "Failed to serialize dependency info: {}"}}"#,
                                        e
                                    )
                                })
                            }
                            Err(e) => {
                                format!(
                                    r#"{{"error": "Failed to process dependency metadata: {}"}}"#,
                                    e
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
                format!(r#"{{"error": "Failed to cache crate: {}"}}"#, e)
            }
        }
    }

    #[tool(
        description = "Analyze a cached crate's module structure. Provides detailed information about module hierarchy and relationships for offline analysis."
    )]
    pub async fn analyze_crate_structure(
        &self,
        #[tool(aggr)] params: AnalyzeCrateParams,
    ) -> String {
        let cache = self.cache.lock().await;

        // Ensure the crate is cached
        match cache
            .ensure_crate_docs(&params.crate_name, &params.version, None)
            .await
        {
            Ok(_) => {
                // Get the source path for the cached crate
                let crate_path = cache.get_source_path(&params.crate_name, &params.version);

                match cargo_modules_lib::analyze_crate(&crate_path) {
                    Ok((crate_id, analysis_host, edition)) => {
                        let db = analysis_host.raw_database();

                        // Build module tree
                        match cargo_modules_lib::build_module_tree(crate_id, db, edition) {
                            Ok(module_tree) => {
                                serde_json::json!({
                                    "status": "success",
                                    "crate_name": params.crate_name,
                                    "version": params.version,
                                    "package": params.package,
                                    "edition": format!("{:?}", edition),
                                    "module_tree": {
                                        "type": "tree_structure",
                                        "node_count": count_tree_nodes(&module_tree),
                                        "description": "Module tree structure successfully built. Use get_module_dependencies for detailed dependency graph."
                                    }
                                }).to_string()
                            }
                            Err(e) => {
                                format!(r#"{{"error": "Failed to build module tree: {}"}}"#, e)
                            }
                        }
                    }
                    Err(e) => {
                        format!(r#"{{"error": "Failed to analyze crate: {}"}}"#, e)
                    }
                }
            }
            Err(e) => {
                format!(r#"{{"error": "Failed to ensure crate is cached: {}"}}"#, e)
            }
        }
    }

    #[tool(
        description = "Get module dependencies and relationships for a cached crate. Returns either a tree structure or dependency graph showing how modules interact with each other."
    )]
    pub async fn get_module_dependencies(
        &self,
        #[tool(aggr)] params: GetModuleDependenciesParams,
    ) -> String {
        let cache = self.cache.lock().await;

        // Ensure the crate is cached
        match cache
            .ensure_crate_docs(&params.crate_name, &params.version, None)
            .await
        {
            Ok(_) => {
                // Get the source path for the cached crate
                let crate_path = cache.get_source_path(&params.crate_name, &params.version);

                match cargo_modules_lib::analyze_crate(&crate_path) {
                    Ok((crate_id, analysis_host, edition)) => {
                        let db = analysis_host.raw_database();

                        if params.graph_format.unwrap_or(false) {
                            // Return dependency graph format
                            match cargo_modules_lib::build_dependency_graph(crate_id, db, edition) {
                                Ok((graph, _root_idx)) => {
                                    serde_json::json!({
                                        "status": "success",
                                        "crate_name": params.crate_name,
                                        "version": params.version,
                                        "package": params.package,
                                        "format": "dependency_graph",
                                        "graph": {
                                            "node_count": graph.node_count(),
                                            "edge_count": graph.edge_count(),
                                            "description": format!("Dependency graph with {} modules and {} relationships", graph.node_count(), graph.edge_count())
                                        }
                                    }).to_string()
                                }
                                Err(e) => {
                                    format!(r#"{{"error": "Failed to build dependency graph: {}"}}"#, e)
                                }
                            }
                        } else {
                            // Return module tree format
                            match cargo_modules_lib::build_module_tree(crate_id, db, edition) {
                                Ok(module_tree) => serde_json::json!({
                                    "status": "success",
                                    "crate_name": params.crate_name,
                                    "version": params.version,
                                    "package": params.package,
                                    "format": "module_tree",
                                    "tree": {
                                        "node_count": count_tree_nodes(&module_tree),
                                        "description": "Module tree showing hierarchical structure"
                                    }
                                })
                                .to_string(),
                                Err(e) => {
                                    format!(r#"{{"error": "Failed to build module tree: {}"}}"#, e)
                                }
                            }
                        }
                    }
                    Err(e) => {
                        format!(r#"{{"error": "Failed to analyze crate: {}"}}"#, e)
                    }
                }
            }
            Err(e) => {
                format!(r#"{{"error": "Failed to ensure crate is cached: {}"}}"#, e)
            }
        }
    }
}

/// Helper function to count nodes in a module tree
fn count_tree_nodes(tree: &cargo_modules_lib::ModuleTree) -> usize {
    1 + tree.subtrees.iter().map(count_tree_nodes).sum::<usize>()
}

#[tool(tool_box)]
impl ServerHandler for RustDocsService {
    fn get_info(&self) -> ServerInfo {
        ServerInfo {
            server_info: rmcp::model::Implementation {
                name: "rust-docs-mcp".to_string(),
                version: "0.1.0".to_string(),
            },
            capabilities: ServerCapabilities {
                tools: Some(Default::default()),
                ..Default::default()
            },
            instructions: Some(
                "MCP server for querying Rust crate documentation and analyzing module dependencies. \
                IMPORTANT: Always use search_items_preview first to avoid token limits. \
                Workflow: search_items_preview → get_item_details for specific items → get_item_source for source code. \
                Use get_dependencies to explore crate dependencies and resolve version conflicts. \
                For crate analysis: analyze_crate_structure → get_module_dependencies for detailed dependency graphs. \
                All tools auto-cache crates from crates.io, GitHub, or local paths. Default limit is 100 items per request. \
                Source locations are included in get_item_details responses."
                    .to_string(),
            ),
            ..Default::default()
        }
    }
}
