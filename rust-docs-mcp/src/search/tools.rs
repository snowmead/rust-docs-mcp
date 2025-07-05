use std::sync::Arc;
use tokio::sync::Mutex;
use anyhow::Result;

use rmcp::schemars;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::cache::CrateCache;
use crate::search::{SearchIndexer, FuzzySearcher, FuzzySearchOptions, SearchResult};

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

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct SearchSuggestionsParams {
    #[schemars(description = "The name of the crate")]
    pub crate_name: String,
    #[schemars(description = "The version of the crate")]
    pub version: String,
    #[schemars(description = "Partial query for suggestions")]
    pub partial_query: String,
    #[schemars(description = "Maximum number of suggestions to return")]
    pub limit: Option<usize>,
    #[schemars(
        description = "For workspace crates, specify the member path (e.g., 'crates/rmcp')"
    )]
    pub member: Option<String>,
}

#[derive(Debug, Clone)]
pub struct SearchTools {
    cache: Arc<Mutex<CrateCache>>,
    indexer: Arc<Mutex<Option<SearchIndexer>>>,
}

impl SearchTools {
    pub fn new(cache: Arc<Mutex<CrateCache>>) -> Self {
        Self {
            cache,
            indexer: Arc::new(Mutex::new(None)),
        }
    }
    
    /// Get or create the search indexer
    async fn get_indexer(&self) -> Result<SearchIndexer> {
        let mut indexer_guard = self.indexer.lock().await;
        
        if indexer_guard.is_none() {
            let cache = self.cache.lock().await;
            let cache_dir = cache.storage.cache_dir();
            let indexer = SearchIndexer::new(cache_dir)?;
            *indexer_guard = Some(indexer);
        }
        
        // Clone the indexer to return it
        // Note: This is not ideal as it creates a new indexer each time
        // In a real implementation, we'd want to share the indexer properly
        let cache = self.cache.lock().await;
        let cache_dir = cache.storage.cache_dir();
        SearchIndexer::new(cache_dir)
    }
    
    /// Ensure a crate is indexed for search
    async fn ensure_crate_indexed(&self, crate_name: &str, version: &str, member: Option<&str>) -> Result<()> {
        let mut indexer = self.get_indexer().await?;
        
        // Check if already indexed
        if indexer.is_crate_indexed(crate_name, version)? {
            return Ok(());
        }
        
        // Get the crate documentation
        let cache = self.cache.lock().await;
        let crate_data = cache.ensure_crate_or_member_docs(crate_name, version, member).await?;
        drop(cache); // Release the lock
        
        // Index the crate
        indexer.add_crate_items(crate_name, version, &crate_data)?;
        
        Ok(())
    }
    
    /// Perform fuzzy search on crate items
    pub async fn search_items_fuzzy(&self, params: SearchItemsFuzzyParams) -> String {
        let result = async {
            // Ensure crate is indexed
            self.ensure_crate_indexed(&params.crate_name, &params.version, params.member.as_deref()).await?;
            
            // Create fuzzy searcher
            let indexer = self.get_indexer().await?;
            let fuzzy_searcher = FuzzySearcher::from_indexer(&indexer)?;
            
            // Build search options
            let options = FuzzySearchOptions {
                fuzzy_enabled: params.fuzzy_enabled.unwrap_or(true),
                fuzzy_distance: params.fuzzy_distance.unwrap_or(1),
                limit: params.limit.unwrap_or(50),
                kind_filter: params.kind_filter.clone(),
                crate_filter: Some(params.crate_name.clone()),
            };
            
            // Perform search
            let results = fuzzy_searcher.search(&params.query, &options)?;
            
            Ok::<Vec<SearchResult>, anyhow::Error>(results)
        }.await;
        
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
                
                serde_json::to_string_pretty(&response)
                    .unwrap_or_else(|e| format!(r#"{{"error": "Failed to serialize search results: {e}"}}"#))
            }
            Err(e) => {
                format!(r#"{{"error": "Search failed: {e}"}}"#)
            }
        }
    }
    
    /// Get search suggestions based on partial query
    pub async fn get_search_suggestions(&self, params: SearchSuggestionsParams) -> String {
        let result = async {
            // Ensure crate is indexed
            self.ensure_crate_indexed(&params.crate_name, &params.version, params.member.as_deref()).await?;
            
            // Create fuzzy searcher
            let indexer = self.get_indexer().await?;
            let fuzzy_searcher = FuzzySearcher::from_indexer(&indexer)?;
            
            // Get suggestions
            let limit = params.limit.unwrap_or(10);
            let suggestions = fuzzy_searcher.get_suggestions(&params.partial_query, limit)?;
            
            Ok::<Vec<String>, anyhow::Error>(suggestions)
        }.await;
        
        match result {
            Ok(suggestions) => {
                let response = serde_json::json!({
                    "suggestions": suggestions,
                    "partial_query": params.partial_query,
                    "crate_name": params.crate_name,
                    "version": params.version
                });
                
                serde_json::to_string_pretty(&response)
                    .unwrap_or_else(|e| format!(r#"{{"error": "Failed to serialize suggestions: {e}"}}"#))
            }
            Err(e) => {
                format!(r#"{{"error": "Failed to get suggestions: {e}"}}"#)
            }
        }
    }
    
    /// Get search index statistics
    pub async fn get_search_stats(&self, crate_name: String, version: String) -> String {
        let result = async {
            let indexer = self.get_indexer().await?;
            let stats = indexer.get_stats()?;
            
            let fuzzy_searcher = FuzzySearcher::from_indexer(&indexer)?;
            let search_stats = fuzzy_searcher.get_search_stats()?;
            
            Ok::<(std::collections::HashMap<String, serde_json::Value>, std::collections::HashMap<String, serde_json::Value>), anyhow::Error>((stats, search_stats))
        }.await;
        
        match result {
            Ok((index_stats, search_stats)) => {
                let response = serde_json::json!({
                    "crate_name": crate_name,
                    "version": version,
                    "index_stats": index_stats,
                    "search_stats": search_stats
                });
                
                serde_json::to_string_pretty(&response)
                    .unwrap_or_else(|e| format!(r#"{{"error": "Failed to serialize stats: {e}"}}"#))
            }
            Err(e) => {
                format!(r#"{{"error": "Failed to get search stats: {e}"}}"#)
            }
        }
    }
}