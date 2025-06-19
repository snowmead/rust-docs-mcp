use std::sync::Arc;
use tokio::sync::Mutex;

use rmcp::schemars;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::cache::CrateCache;
use crate::docs::DocQuery;

/// Maximum size for response in bytes (roughly 25k tokens * 4 bytes/token)
const MAX_RESPONSE_SIZE: usize = 100_000;

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
    #[schemars(
        description = "For workspace crates, specify the member path (e.g., 'crates/rmcp')"
    )]
    pub member: Option<String>,
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
    #[schemars(
        description = "For workspace crates, specify the member path (e.g., 'crates/rmcp')"
    )]
    pub member: Option<String>,
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
    #[schemars(
        description = "For workspace crates, specify the member path (e.g., 'crates/rmcp')"
    )]
    pub member: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct GetItemDetailsParams {
    #[schemars(description = "The name of the crate")]
    pub crate_name: String,
    #[schemars(description = "The version of the crate")]
    pub version: String,
    #[schemars(description = "The numeric ID of the item")]
    pub item_id: u32,
    #[schemars(
        description = "For workspace crates, specify the member path (e.g., 'crates/rmcp')"
    )]
    pub member: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct GetItemDocsParams {
    #[schemars(description = "The name of the crate")]
    pub crate_name: String,
    #[schemars(description = "The version of the crate")]
    pub version: String,
    #[schemars(description = "The numeric ID of the item")]
    pub item_id: u32,
    #[schemars(
        description = "For workspace crates, specify the member path (e.g., 'crates/rmcp')"
    )]
    pub member: Option<String>,
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
    #[schemars(
        description = "For workspace crates, specify the member path (e.g., 'crates/rmcp')"
    )]
    pub member: Option<String>,
}

#[derive(Debug, Clone)]
pub struct DocsTools {
    cache: Arc<Mutex<CrateCache>>,
}

impl DocsTools {
    pub fn new(cache: Arc<Mutex<CrateCache>>) -> Self {
        Self { cache }
    }

    /// Helper to check if a response might exceed size limits
    fn estimate_response_size<T: Serialize>(data: &T) -> usize {
        serde_json::to_string(data).map(|s| s.len()).unwrap_or(0)
    }

    pub async fn list_crate_items(&self, params: ListItemsParams) -> String {
        let cache = self.cache.lock().await;
        match cache
            .ensure_crate_or_member_docs(
                &params.crate_name,
                &params.version,
                params.member.as_deref(),
            )
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
                    format!(r#"{{"error": "Failed to serialize items: {e}"}}"#)
                })
            }
            Err(e) => {
                format!(r#"{{"error": "Failed to get crate docs: {e}"}}"#)
            }
        }
    }

    pub async fn search_items(&self, params: SearchItemsParams) -> String {
        let cache = self.cache.lock().await;
        match cache
            .ensure_crate_or_member_docs(
                &params.crate_name,
                &params.version,
                params.member.as_deref(),
            )
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
                    format!(r#"{{"error": "Failed to serialize items: {e}"}}"#)
                })
            }
            Err(e) => {
                format!(r#"{{"error": "Failed to get crate docs: {e}"}}"#)
            }
        }
    }

    pub async fn search_items_preview(&self, params: SearchItemsPreviewParams) -> String {
        let cache = self.cache.lock().await;
        match cache
            .ensure_crate_or_member_docs(
                &params.crate_name,
                &params.version,
                params.member.as_deref(),
            )
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
                    format!(r#"{{"error": "Failed to serialize items: {e}"}}"#)
                })
            }
            Err(e) => {
                format!(r#"{{"error": "Failed to get crate docs: {e}"}}"#)
            }
        }
    }

    pub async fn get_item_details(&self, params: GetItemDetailsParams) -> String {
        let cache = self.cache.lock().await;
        match cache
            .ensure_crate_or_member_docs(
                &params.crate_name,
                &params.version,
                params.member.as_deref(),
            )
            .await
        {
            Ok(crate_data) => {
                let query = DocQuery::new(crate_data);
                match query.get_item_details(params.item_id) {
                    Ok(details) => serde_json::to_string_pretty(&details).unwrap_or_else(|e| {
                        format!(r#"{{"error": "Failed to serialize details: {e}"}}"#)
                    }),
                    Err(e) => format!(r#"{{"error": "Item not found: {e}"}}"#),
                }
            }
            Err(e) => {
                format!(r#"{{"error": "Failed to get crate docs: {e}"}}"#)
            }
        }
    }

    pub async fn get_item_docs(&self, params: GetItemDocsParams) -> String {
        let cache = self.cache.lock().await;
        match cache
            .ensure_crate_or_member_docs(
                &params.crate_name,
                &params.version,
                params.member.as_deref(),
            )
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
                    Err(e) => format!(r#"{{"error": "Failed to get docs: {e}"}}"#),
                }
            }
            Err(e) => {
                format!(r#"{{"error": "Failed to get crate docs: {e}"}}"#)
            }
        }
    }

    pub async fn get_item_source(&self, params: GetItemSourceParams) -> String {
        let cache = self.cache.lock().await;
        let source_base_path = cache.get_source_path(&params.crate_name, &params.version);

        match cache
            .ensure_crate_or_member_docs(
                &params.crate_name,
                &params.version,
                params.member.as_deref(),
            )
            .await
        {
            Ok(crate_data) => {
                let query = DocQuery::new(crate_data);
                let context_lines = params.context_lines.unwrap_or(3);

                match query.get_item_source(params.item_id, &source_base_path, context_lines) {
                    Ok(source_info) => {
                        serde_json::to_string_pretty(&source_info).unwrap_or_else(|e| {
                            format!(r#"{{"error": "Failed to serialize source info: {e}"}}"#)
                        })
                    }
                    Err(e) => format!(r#"{{"error": "Failed to get source: {e}"}}"#),
                }
            }
            Err(e) => {
                format!(r#"{{"error": "Failed to get crate docs: {e}"}}"#)
            }
        }
    }
}
