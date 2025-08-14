use std::sync::Arc;
use tokio::sync::RwLock;

use rmcp::schemars;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::cache::CrateCache;
use crate::docs::{
    DocQuery,
    outputs::{
        DetailedItem, DocsErrorOutput, GetItemDetailsOutput, GetItemDocsOutput,
        GetItemSourceOutput, ItemInfo, ItemPreview, ListCrateItemsOutput, PaginationInfo,
        SearchItemsOutput, SearchItemsPreviewOutput, SourceInfo, SourceLocation,
    },
};

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
    pub limit: Option<i64>,
    #[schemars(description = "Starting position for pagination (default: 0)")]
    pub offset: Option<i64>,
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
    #[schemars(
        description = "The pattern to search for in item names. Note: passing '*' will not return any items - use specific Rust symbols or generalize over common names (e.g., 'new', 'parse', 'Error') to get meaningful results"
    )]
    pub pattern: String,
    #[schemars(description = "Maximum number of items to return (default: 100)")]
    pub limit: Option<i64>,
    #[schemars(description = "Starting position for pagination (default: 0)")]
    pub offset: Option<i64>,
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
    #[schemars(
        description = "The pattern to search for in item names. Note: passing '*' will not return any items - use specific Rust symbols or generalize over common names (e.g., 'new', 'parse', 'Error') to get meaningful results"
    )]
    pub pattern: String,
    #[schemars(description = "Maximum number of items to return (default: 100)")]
    pub limit: Option<i64>,
    #[schemars(description = "Starting position for pagination (default: 0)")]
    pub offset: Option<i64>,
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
    pub item_id: i32,
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
    pub item_id: i32,
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
    pub item_id: i32,
    #[schemars(
        description = "Number of context lines to include before and after the item (default: 3)"
    )]
    pub context_lines: Option<i64>,
    #[schemars(
        description = "For workspace crates, specify the member path (e.g., 'crates/rmcp')"
    )]
    pub member: Option<String>,
}

#[derive(Debug, Clone)]
pub struct DocsTools {
    cache: Arc<RwLock<CrateCache>>,
}

impl DocsTools {
    pub fn new(cache: Arc<RwLock<CrateCache>>) -> Self {
        Self { cache }
    }

    /// Helper to check if a response might exceed size limits
    fn estimate_response_size<T: Serialize>(data: &T) -> usize {
        serde_json::to_string(data).map(|s| s.len()).unwrap_or(0)
    }

    pub async fn list_crate_items(
        &self,
        params: ListItemsParams,
    ) -> Result<ListCrateItemsOutput, DocsErrorOutput> {
        let cache = self.cache.write().await;
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
                let limit = params.limit.unwrap_or(100).max(0) as usize;
                let offset = params.offset.unwrap_or(0).max(0) as usize;

                // Apply pagination
                let paginated_items: Vec<_> = items
                    .into_iter()
                    .skip(offset)
                    .take(limit)
                    .map(|item| ItemInfo {
                        id: item.id.to_string(),
                        name: item.name.clone(),
                        kind: item.kind.clone(),
                        path: item.path.clone(),
                        docs: item.docs.clone(),
                        visibility: item.visibility.clone(),
                    })
                    .collect();

                Ok(ListCrateItemsOutput {
                    items: paginated_items,
                    pagination: PaginationInfo {
                        total: total_count,
                        limit,
                        offset,
                        has_more: offset + limit < total_count,
                    },
                })
            }
            Err(e) => Err(DocsErrorOutput::new(format!(
                "Failed to get crate docs: {e}"
            ))),
        }
    }

    pub async fn search_items(
        &self,
        params: SearchItemsParams,
    ) -> Result<SearchItemsOutput, DocsErrorOutput> {
        let cache = self.cache.write().await;
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
                let limit = params.limit.unwrap_or(100).max(0) as usize;
                let offset = params.offset.unwrap_or(0).max(0) as usize;

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

                let warning = if truncated {
                    Some("Response was truncated to stay within size limits. Use smaller limit or preview mode.".to_string())
                } else {
                    None
                };

                Ok(SearchItemsOutput {
                    items: paginated_items
                        .into_iter()
                        .map(|item| ItemInfo {
                            id: item.id.to_string(),
                            name: item.name.clone(),
                            kind: item.kind.clone(),
                            path: item.path.clone(),
                            docs: item.docs.clone(),
                            visibility: item.visibility.clone(),
                        })
                        .collect(),
                    pagination: PaginationInfo {
                        total: total_count,
                        limit: actual_limit,
                        offset,
                        has_more: offset + actual_limit < total_count,
                    },
                    warning,
                })
            }
            Err(e) => Err(DocsErrorOutput::new(format!(
                "Failed to get crate docs: {e}"
            ))),
        }
    }

    pub async fn search_items_preview(
        &self,
        params: SearchItemsPreviewParams,
    ) -> Result<SearchItemsPreviewOutput, DocsErrorOutput> {
        let cache = self.cache.write().await;
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
                let limit = params.limit.unwrap_or(100).max(0) as usize;
                let offset = params.offset.unwrap_or(0).max(0) as usize;

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

                Ok(SearchItemsPreviewOutput {
                    items: preview_items
                        .into_iter()
                        .map(|item| ItemPreview {
                            id: item["id"].as_str().unwrap_or("").to_string(),
                            name: item["name"].as_str().unwrap_or("").to_string(),
                            kind: item["kind"].as_str().unwrap_or("").to_string(),
                            path: item["path"]
                                .as_array()
                                .map(|arr| {
                                    arr.iter()
                                        .filter_map(|v| v.as_str().map(String::from))
                                        .collect()
                                })
                                .unwrap_or_default(),
                        })
                        .collect(),
                    pagination: PaginationInfo {
                        total: total_count,
                        limit,
                        offset,
                        has_more: offset + limit < total_count,
                    },
                })
            }
            Err(e) => Err(DocsErrorOutput::new(format!(
                "Failed to get crate docs: {e}"
            ))),
        }
    }

    pub async fn get_item_details(&self, params: GetItemDetailsParams) -> GetItemDetailsOutput {
        let cache = self.cache.write().await;
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
                match query.get_item_details(params.item_id.max(0) as u32) {
                    Ok(details) => {
                        // Convert the details to our output format
                        GetItemDetailsOutput::Success(Box::new(DetailedItem {
                            info: ItemInfo {
                                id: details.info.id.clone(),
                                name: details.info.name.clone(),
                                kind: details.info.kind.clone(),
                                path: details.info.path.clone(),
                                docs: details.info.docs.clone(),
                                visibility: details.info.visibility.clone(),
                            },
                            signature: details.signature.clone(),
                            generics: details.generics.clone(),
                            fields: details.fields.map(|fields| {
                                fields
                                    .into_iter()
                                    .map(|f| ItemInfo {
                                        id: f.id,
                                        name: f.name,
                                        kind: f.kind,
                                        path: f.path,
                                        docs: f.docs,
                                        visibility: f.visibility,
                                    })
                                    .collect()
                            }),
                            variants: details.variants.map(|variants| {
                                variants
                                    .into_iter()
                                    .map(|v| ItemInfo {
                                        id: v.id,
                                        name: v.name,
                                        kind: v.kind,
                                        path: v.path,
                                        docs: v.docs,
                                        visibility: v.visibility,
                                    })
                                    .collect()
                            }),
                            methods: details.methods.map(|methods| {
                                methods
                                    .into_iter()
                                    .map(|m| ItemInfo {
                                        id: m.id,
                                        name: m.name,
                                        kind: m.kind,
                                        path: m.path,
                                        docs: m.docs,
                                        visibility: m.visibility,
                                    })
                                    .collect()
                            }),
                            source_location: details.source_location.map(|loc| SourceLocation {
                                filename: loc.filename,
                                line_start: loc.line_start,
                                column_start: loc.column_start,
                                line_end: loc.line_end,
                                column_end: loc.column_end,
                            }),
                        }))
                    }
                    Err(e) => GetItemDetailsOutput::Error {
                        error: format!("Item not found: {e}"),
                    },
                }
            }
            Err(e) => GetItemDetailsOutput::Error {
                error: format!("Failed to get crate docs: {e}"),
            },
        }
    }

    pub async fn get_item_docs(
        &self,
        params: GetItemDocsParams,
    ) -> Result<GetItemDocsOutput, DocsErrorOutput> {
        let cache = self.cache.write().await;
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
                match query.get_item_docs(params.item_id.max(0) as u32) {
                    Ok(docs) => {
                        let message = if docs.is_none() {
                            Some("No documentation available for this item".to_string())
                        } else {
                            None
                        };
                        Ok(GetItemDocsOutput {
                            documentation: docs,
                            message,
                        })
                    }
                    Err(e) => Err(DocsErrorOutput::new(format!("Failed to get docs: {e}"))),
                }
            }
            Err(e) => Err(DocsErrorOutput::new(format!(
                "Failed to get crate docs: {e}"
            ))),
        }
    }

    pub async fn get_item_source(&self, params: GetItemSourceParams) -> GetItemSourceOutput {
        let cache = self.cache.write().await;
        let source_base_path = match cache.get_source_path(&params.crate_name, &params.version) {
            Ok(path) => path,
            Err(e) => {
                return GetItemSourceOutput::Error {
                    error: format!("Failed to get source path: {e}"),
                };
            }
        };

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
                let context_lines = params.context_lines.unwrap_or(3).max(0) as usize;

                match query.get_item_source(
                    params.item_id.max(0) as u32,
                    &source_base_path,
                    context_lines,
                ) {
                    Ok(source_info) => GetItemSourceOutput::Success(SourceInfo {
                        location: SourceLocation {
                            filename: source_info.location.filename,
                            line_start: source_info.location.line_start,
                            column_start: source_info.location.column_start,
                            line_end: source_info.location.line_end,
                            column_end: source_info.location.column_end,
                        },
                        code: source_info.code,
                        context_lines: source_info.context_lines,
                    }),
                    Err(e) => GetItemSourceOutput::Error {
                        error: format!("Failed to get source: {e}"),
                    },
                }
            }
            Err(e) => GetItemSourceOutput::Error {
                error: format!("Failed to get crate docs: {e}"),
            },
        }
    }
}
