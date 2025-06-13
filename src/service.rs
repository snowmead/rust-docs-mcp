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
use crate::docs::DocQuery;

/// Maximum size for response in bytes (roughly 25k tokens * 4 bytes/token)
const MAX_RESPONSE_SIZE: usize = 100_000;

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

#[tool(tool_box)]
impl RustDocsService {
    pub fn new() -> Result<Self> {
        Ok(Self {
            cache: Arc::new(Mutex::new(CrateCache::new()?)),
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
            .ensure_crate_docs(&params.crate_name, &params.version)
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
            .ensure_crate_docs(&params.crate_name, &params.version)
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
            .ensure_crate_docs(&params.crate_name, &params.version)
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
            .ensure_crate_docs(&params.crate_name, &params.version)
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
            .ensure_crate_docs(&params.crate_name, &params.version)
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
            .ensure_crate_docs(&params.crate_name, &params.version)
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
            .ensure_crate_docs(&params.crate_name, &params.version)
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
                "MCP server for querying Rust crate documentation. \
                IMPORTANT: Always use search_items_preview first to avoid token limits. \
                Workflow: search_items_preview → get_item_details for specific items → get_item_source for source code. \
                All tools auto-cache crates. Default limit is 100 items per request. \
                Source locations are included in get_item_details responses."
                    .to_string(),
            ),
            ..Default::default()
        }
    }
}
