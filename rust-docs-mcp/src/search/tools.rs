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
//! # use tokio::sync::RwLock;
//! # use rust_docs_mcp::cache::CrateCache;
//! # use rust_docs_mcp::search::tools::{SearchTools, SearchItemsFuzzyParams};
//! # async fn example() -> anyhow::Result<()> {
//! let cache = Arc::new(RwLock::new(CrateCache::new(None)?));
//! let tools = SearchTools::new(cache);
//!
//! let params = SearchItemsFuzzyParams {
//!     no_default_features: None,
//!     all_features: None,
//!     features: None,
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
use tokio::sync::RwLock;

use rmcp::schemars;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::cache::{CrateCache, storage::CacheStorage};
use crate::search::config::{
    DEFAULT_FUZZY_DISTANCE, DEFAULT_SEARCH_LIMIT, MAX_FUZZY_DISTANCE, MAX_SEARCH_LIMIT,
};
use crate::search::outputs::{SearchErrorOutput, SearchItemsFuzzyOutput};
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
    #[schemars(
        description = "Select the same features used when caching. An explicit list disables defaults unless no_default_features=false. Omitted options use all-features, defaults, then no-defaults fallback. Item IDs belong to the selected variant."
    )]
    pub features: Option<Vec<String>>,
    pub no_default_features: Option<bool>,
    pub all_features: Option<bool>,
}

#[derive(Debug, Clone)]
pub struct SearchTools {
    cache: Arc<RwLock<CrateCache>>,
}

impl SearchTools {
    pub fn new(cache: Arc<RwLock<CrateCache>>) -> Self {
        Self { cache }
    }

    /// Perform the actual search without holding any locks
    async fn perform_search(
        &self,
        params: SearchItemsFuzzyParams,
        storage: CacheStorage,
    ) -> Result<Vec<SearchResult>, anyhow::Error> {
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
                "Fuzzy distance must be between 0 and {MAX_FUZZY_DISTANCE}"
            ));
        }

        // Validate limit
        let limit = params.limit.unwrap_or(DEFAULT_SEARCH_LIMIT);
        if limit > MAX_SEARCH_LIMIT {
            return Err(anyhow::anyhow!("Limit must not exceed {MAX_SEARCH_LIMIT}"));
        }

        // Build search options
        let options = FuzzySearchOptions {
            fuzzy_enabled: params.fuzzy_enabled.unwrap_or(true),
            fuzzy_distance,
            limit,
            kind_filter: params.kind_filter.clone(),
            crate_filter: Some(params.crate_name.clone()),
            member_filter: params.member.clone(),
        };

        // Perform search
        fuzzy_searcher.search(&params.query, &options)
    }

    /// Perform fuzzy search on crate items
    pub async fn search_items_fuzzy(
        &self,
        params: SearchItemsFuzzyParams,
    ) -> Result<SearchItemsFuzzyOutput, SearchErrorOutput> {
        let query = params.query.clone();
        let fuzzy_enabled = params.fuzzy_enabled.unwrap_or(true);
        let crate_name = params.crate_name.clone();
        let version = params.version.clone();
        let member = params.member.clone();
        let result = async {
            {
                let guard = self.cache.read().await;
                let cache = guard.with_features(
                    params.features.clone(),
                    params.no_default_features,
                    params.all_features,
                )?;
                if cache.has_docs(
                    &params.crate_name,
                    &params.version,
                    params.member.as_deref(),
                ) && cache.storage.has_search_index(
                    &params.crate_name,
                    &params.version,
                    params.member.as_deref(),
                ) {
                    return self.perform_search(params, cache.storage.clone()).await;
                }
            }
            let guard = self.cache.write().await;
            let cache = guard.with_features(
                params.features.clone(),
                params.no_default_features,
                params.all_features,
            )?;
            cache
                .ensure_crate_or_member_docs(
                    &params.crate_name,
                    &params.version,
                    params.member.as_deref(),
                )
                .await?;
            if !cache.storage.has_search_index(
                &params.crate_name,
                &params.version,
                params.member.as_deref(),
            ) {
                cache
                    .create_search_index(
                        &params.crate_name,
                        &params.version,
                        params.member.as_deref(),
                    )
                    .await?;
            }
            self.perform_search(params, cache.storage.clone()).await
        }
        .await;

        match result {
            Ok(results) => {
                let total = results.len();
                Ok(SearchItemsFuzzyOutput {
                    results: results
                        .into_iter()
                        .map(|r| crate::search::outputs::SearchResult {
                            score: r.score,
                            item_id: r.item_id,
                            name: r.name,
                            path: r.path,
                            kind: r.kind,
                            crate_name: r.crate_name,
                            version: r.version,
                            visibility: r.visibility,
                            doc_preview: None, // fuzzy::SearchResult doesn't have doc_preview
                            member: r.member,
                        })
                        .collect(),
                    query,
                    total_results: total,
                    fuzzy_enabled,
                    crate_name,
                    version,
                    member,
                })
            }
            Err(e) => Err(SearchErrorOutput::new(format!("Search failed: {e}"))),
        }
    }
}
