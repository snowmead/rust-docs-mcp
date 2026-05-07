//! Transport-free application runtime shared by MCP and CLI adapters.
//!
//! [`RustDocsRuntime`] owns the cache, docs, search, deps, and analysis tool
//! instances and exposes one method per logical operation. Each method returns
//! the final `String` intended for stdout (CLI) or raw MCP response.

use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::RwLock;

use anyhow::Result;

use crate::analysis::tools::{AnalysisTools, AnalyzeCrateStructureParams};
use crate::cache::{
    CrateCache,
    task_manager::TaskManager,
    tools::{
        CacheCrateParams, CacheOperationsParams, CacheTools, GetCratesMetadataParams,
        ListCrateVersionsParams, RemoveCrateParams,
    },
};
use crate::deps::tools::{DepsTools, GetDependenciesParams};
use crate::docs::tools::{
    DocsTools, GetItemDetailsParams, GetItemDocsParams, GetItemSourceParams, ListItemsParams,
    SearchItemsParams, SearchItemsPreviewParams,
};
use crate::search::tools::{SearchItemsFuzzyParams, SearchTools};

/// Shared application runtime owning all tool facets.
///
/// Constructed once per invocation; each public method performs the
/// underlying operation synchronously from the caller's perspective
/// and returns a ready-to-emit `String`.
#[derive(Debug, Clone)]
pub struct RustDocsRuntime {
    cache_tools: CacheTools,
    docs_tools: DocsTools,
    deps_tools: DepsTools,
    analysis_tools: AnalysisTools,
    search_tools: SearchTools,
}

impl RustDocsRuntime {
    /// Create a new runtime with the given cache directory.
    ///
    /// `cache_dir: None` uses the default `~/.rust-docs-mcp/cache`.
    pub fn new(cache_dir: Option<PathBuf>) -> Result<Self> {
        let cache = Arc::new(RwLock::new(CrateCache::new(cache_dir)?));
        let task_manager = Arc::new(TaskManager::new());

        Ok(Self {
            cache_tools: CacheTools::new(cache.clone(), task_manager),
            docs_tools: DocsTools::new(cache.clone()),
            deps_tools: DepsTools::new(cache.clone()),
            analysis_tools: AnalysisTools::new(cache.clone()),
            search_tools: SearchTools::new(cache),
        })
    }

    // ── Cache ─────────────────────────────────────────────────────

    /// MCP-style asynchronous (background) cache.
    ///
    /// Validates params, spawns a tracked background task, and returns
    /// a [`CacheTaskStartedOutput`] JSON immediately.
    pub async fn cache_crate_background(&self, params: CacheCrateParams) -> String {
        self.cache_tools.cache_crate(params).await
    }

    /// One-shot blocking cache: validates params, runs the full cache →
    /// doc generation → search-index pipeline, and returns the result
    /// JSON. The call does **not** return until the pipeline finishes.
    pub async fn cache_crate_blocking(&self, params: CacheCrateParams) -> String {
        self.cache_tools.cache_crate_blocking(params).await
    }

    /// Remove a cached crate version.
    pub async fn remove_crate(&self, params: RemoveCrateParams) -> String {
        match self.cache_tools.remove_crate(params).await {
            Ok(output) => output.to_json(),
            Err(error) => error.to_json(),
        }
    }

    /// List all locally cached crates.
    pub async fn list_cached_crates(&self) -> String {
        match self.cache_tools.list_cached_crates().await {
            Ok(output) => output.to_json(),
            Err(error) => error.to_json(),
        }
    }

    /// List locally cached versions of a crate.
    pub async fn list_crate_versions(&self, params: ListCrateVersionsParams) -> String {
        match self.cache_tools.list_crate_versions(params).await {
            Ok(output) => output.to_json(),
            Err(error) => error.to_json(),
        }
    }

    /// Get metadata for multiple crates and their workspace members.
    pub async fn get_crates_metadata(&self, params: GetCratesMetadataParams) -> String {
        self.cache_tools.get_crates_metadata(params).await.to_json()
    }

    /// Manage/monitor background caching tasks (MCP-only concept).
    pub async fn cache_operations(&self, params: CacheOperationsParams) -> String {
        self.cache_tools.cache_operations(params).await
    }

    // ── Docs / listing ────────────────────────────────────────────

    /// List all items in a crate's documentation (full details).
    pub async fn list_crate_items(&self, params: ListItemsParams) -> String {
        match self.docs_tools.list_crate_items(params).await {
            Ok(output) => output.to_json(),
            Err(error) => error.to_json(),
        }
    }

    /// Exact search — full details (may be large).
    pub async fn search_items(&self, params: SearchItemsParams) -> String {
        match self.docs_tools.search_items(params).await {
            Ok(output) => output.to_json(),
            Err(error) => error.to_json(),
        }
    }

    /// Exact search — preview only (id, name, kind, path).
    pub async fn search_items_preview(&self, params: SearchItemsPreviewParams) -> String {
        match self.docs_tools.search_items_preview(params).await {
            Ok(output) => output.to_json(),
            Err(error) => error.to_json(),
        }
    }

    /// Full details for a specific item by numeric id.
    pub async fn get_item_details(&self, params: GetItemDetailsParams) -> String {
        self.docs_tools.get_item_details(params).await.to_json()
    }

    /// Documentation string only for a specific item.
    pub async fn get_item_docs(&self, params: GetItemDocsParams) -> String {
        match self.docs_tools.get_item_docs(params).await {
            Ok(output) => output.to_json(),
            Err(error) => error.to_json(),
        }
    }

    /// Source code for a specific item.
    pub async fn get_item_source(&self, params: GetItemSourceParams) -> String {
        self.docs_tools.get_item_source(params).await.to_json()
    }

    // ── Deps ──────────────────────────────────────────────────────

    /// Get dependency information for a crate.
    pub async fn get_dependencies(&self, params: GetDependenciesParams) -> String {
        match self.deps_tools.get_dependencies(params).await {
            Ok(output) => output.to_json(),
            Err(error) => error.to_json(),
        }
    }

    // ── Analysis ──────────────────────────────────────────────────

    /// View the hierarchical structure tree of a crate.
    pub async fn structure(&self, params: AnalyzeCrateStructureParams) -> String {
        match self.analysis_tools.structure(params).await {
            Ok(output) => output.to_json(),
            Err(error) => error.to_json(),
        }
    }

    // ── Search (fuzzy) ────────────────────────────────────────────

    /// Fuzzy search with Tantivy — typo tolerance.
    pub async fn search_items_fuzzy(&self, params: SearchItemsFuzzyParams) -> String {
        match self.search_tools.search_items_fuzzy(params).await {
            Ok(output) => output.to_json(),
            Err(error) => error.to_json(),
        }
    }
}
