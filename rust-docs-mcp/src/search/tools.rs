//! # Search Tools Module
//!
//! Provides MCP tool integration for fuzzy search functionality.
//!
//! ## Key Components
//! - [`SearchTools`] - MCP tool handler for search operations
//! - [`SearchItemsFuzzyParams`] - Parameters for fuzzy search requests
//!
//! ## Features
//! - Automatic crate indexing on first search
//! - Fuzzy search with configurable edit distance
//! - Result filtering by kind and crate
//!
//! ## Example
//! ```no_run
//! # use std::sync::Arc;
//! # use tokio::sync::Mutex;
//! # use rust_docs_mcp::cache::CrateCache;
//! # use rust_docs_mcp::search::tools::{SearchTools, SearchItemsFuzzyParams};
//! # async fn example() -> anyhow::Result<()> {
//! let cache = Arc::new(Mutex::new(CrateCache::new(None)?));
//! let tools = SearchTools::new(cache);
//!
//! let params = SearchItemsFuzzyParams {
//!     crate_name: "serde".to_string(),
//!     version: "1.0.0".to_string(),
//!     query: "deserialize".to_string(),
//!     fuzzy_enabled: Some(true),
//!     fuzzy_distance: Some(1),
//!     limit: Some(10),
//!     kind_filter: None,
//!     member: None,
//! };
//!
//! let results = tools.search_items_fuzzy(params).await;
//! # Ok(())
//! # }
//! ```

use std::sync::Arc;
use tokio::sync::Mutex;

use rmcp::schemars;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::cache::CrateCache;
use crate::search::config::{
    DEFAULT_FUZZY_DISTANCE, DEFAULT_SEARCH_LIMIT, MAX_FUZZY_DISTANCE, MAX_SEARCH_LIMIT,
};
use crate::search::{FuzzySearchOptions, FuzzySearcher, SearchIndexer, SearchResult};

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct SearchItemsFuzzyParams {
    #[schemars(description = "The name of the crate")]
    pub crate_name: String,
    #[schemars(description = "The version of the crate")]
    pub version: String,
    #[schemars(description = "The search query")]
    pub query: String,
    #[schemars(description = "Enable fuzzy matching for typo tolerance")]
    pub fuzzy_enabled: Option<bool>,
    #[schemars(description = "Edit distance for fuzzy matching (0-2)")]
    pub fuzzy_distance: Option<u8>,
    #[schemars(description = "Maximum number of results to return")]
    pub limit: Option<usize>,
    #[schemars(description = "Filter by item kind")]
    pub kind_filter: Option<String>,
    #[schemars(
        description = "For workspace crates, specify the member path (e.g., 'crates/rmcp')"
    )]
    pub member: Option<String>,
}

#[derive(Debug, Clone)]
pub struct SearchTools {
    cache: Arc<Mutex<CrateCache>>,
}

impl SearchTools {
    pub fn new(cache: Arc<Mutex<CrateCache>>) -> Self {
        Self { cache }
    }

    /// Check if a crate has a search index
    async fn has_search_index(
        &self,
        crate_name: &str,
        version: &str,
        member: Option<&str>,
    ) -> bool {
        let cache = self.cache.lock().await;
        match member {
            Some(member_name) => {
                cache
                    .storage
                    .has_member_search_index(crate_name, version, member_name)
            }
            None => cache.storage.has_search_index(crate_name, version),
        }
    }

    /// Perform fuzzy search on crate items
    pub async fn search_items_fuzzy(&self, params: SearchItemsFuzzyParams) -> String {
        let result = async {
            // Check if crate has a search index
            if !self
                .has_search_index(
                    &params.crate_name,
                    &params.version,
                    params.member.as_deref(),
                )
                .await
            {
                // Ensure documentation exists (which will create the index)
                let cache = self.cache.lock().await;
                cache
                    .ensure_crate_or_member_docs(
                        &params.crate_name,
                        &params.version,
                        params.member.as_deref(),
                    )
                    .await?;

                // Check again after ensuring docs
                if !self
                    .has_search_index(
                        &params.crate_name,
                        &params.version,
                        params.member.as_deref(),
                    )
                    .await
                {
                    return Err(anyhow::anyhow!(
                        "Search index not available for {}-{}",
                        params.crate_name,
                        params.version
                    ));
                }
            }

            // Get storage to access index path
            let storage = {
                let cache = self.cache.lock().await;
                cache.storage.clone()
            };

            // Create indexer for the specific crate
            let indexer = SearchIndexer::new_for_crate(
                &params.crate_name,
                &params.version,
                &storage,
                params.member.as_deref(),
            )?;

            // Create fuzzy searcher
            let fuzzy_searcher = FuzzySearcher::from_indexer(&indexer)?;

            // Validate fuzzy distance
            let fuzzy_distance = params.fuzzy_distance.unwrap_or(DEFAULT_FUZZY_DISTANCE);
            if fuzzy_distance > MAX_FUZZY_DISTANCE {
                return Err(anyhow::anyhow!(
                    "Fuzzy distance must be between 0 and {}",
                    MAX_FUZZY_DISTANCE
                ));
            }

            // Validate limit
            let limit = params.limit.unwrap_or(DEFAULT_SEARCH_LIMIT);
            if limit > MAX_SEARCH_LIMIT {
                return Err(anyhow::anyhow!(
                    "Limit must not exceed {}",
                    MAX_SEARCH_LIMIT
                ));
            }

            // Build search options
            let options = FuzzySearchOptions {
                fuzzy_enabled: params.fuzzy_enabled.unwrap_or(true),
                fuzzy_distance,
                limit,
                kind_filter: params.kind_filter.clone(),
                crate_filter: Some(params.crate_name.clone()),
            };

            // Perform search
            let results = fuzzy_searcher.search(&params.query, &options)?;

            Ok::<Vec<SearchResult>, anyhow::Error>(results)
        }
        .await;

        match result {
            Ok(results) => {
                let response = serde_json::json!({
                    "results": results,
                    "query": params.query,
                    "total_results": results.len(),
                    "fuzzy_enabled": params.fuzzy_enabled.unwrap_or(true),
                    "crate_name": params.crate_name,
                    "version": params.version
                });

                serde_json::to_string_pretty(&response).unwrap_or_else(|e| {
                    format!(r#"{{"error": "Failed to serialize search results: {e}"}}"#)
                })
            }
            Err(e) => {
                format!(r#"{{"error": "Search failed: {e}"}}"#)
            }
        }
    }
}
