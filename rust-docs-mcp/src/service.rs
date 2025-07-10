use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::Mutex;

use anyhow::Result;
use rmcp::schemars::{self, JsonSchema};
use rmcp::{
    ServerHandler,
    handler::server::{prompt::Arguments, router::tool::ToolRouter, tool::Parameters},
    model::{
        GetPromptRequestParam, GetPromptResult, ListPromptsResult, PaginatedRequestParam,
        PromptMessage, PromptMessageRole, ServerCapabilities, ServerInfo,
    },
    service::{RequestContext, RoleServer},
    tool, tool_handler, tool_router,
};
use serde::{Deserialize, Serialize};

use crate::analysis::tools::AnalysisTools;
use crate::cache::{
    CrateCache,
    tools::{
        CacheCrateFromCratesIOParams, CacheCrateFromGitHubParams, CacheCrateFromLocalParams,
        CacheTools,
    },
};
use crate::deps::tools::DepsTools;
use crate::docs::tools::DocsTools;

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
struct FindImplementationArgs {
    /// The name of the crate to search in
    pub crate_name: String,

    /// Optional member name if searching in a workspace member
    #[serde(skip_serializing_if = "Option::is_none")]
    pub member_name: Option<String>,

    /// What you are trying to find (e.g., "async runtime initialization", "error handling")
    pub query: String,
}

#[derive(Debug, Clone)]
pub struct RustDocsService {
    cache_tools: CacheTools,
    docs_tools: DocsTools,
    deps_tools: DepsTools,
    analysis_tools: AnalysisTools,
    tool_router: ToolRouter<Self>,
}

impl RustDocsService {
    pub fn new(cache_dir: Option<PathBuf>) -> Result<Self> {
        let cache = Arc::new(Mutex::new(CrateCache::new(cache_dir)?));

        Ok(Self {
            cache_tools: CacheTools::new(cache.clone()),
            docs_tools: DocsTools::new(cache.clone()),
            deps_tools: DepsTools::new(cache.clone()),
            analysis_tools: AnalysisTools::new(cache),
            tool_router: Self::tool_router(),
        })
    }
}

#[tool_router]
impl RustDocsService {
    // Cache tools
    #[tool(
        description = "Download and cache a specific crate version from crates.io for offline use. This happens automatically when using other tools, but use this to pre-cache crates. Useful for preparing offline access or ensuring a crate is available before searching."
    )]
    pub async fn cache_crate_from_cratesio(
        &self,
        params: Parameters<CacheCrateFromCratesIOParams>,
    ) -> String {
        self.cache_tools.cache_crate_from_cratesio(params.0).await
    }

    #[tool(
        description = "Download and cache a specific crate version from GitHub for offline use. Supports cloning from any GitHub repository URL. You must specify either a branch OR a tag (but not both). The crate will be cached using the branch/tag name as the version."
    )]
    pub async fn cache_crate_from_github(
        &self,
        params: Parameters<CacheCrateFromGitHubParams>,
    ) -> String {
        self.cache_tools.cache_crate_from_github(params.0).await
    }

    #[tool(
        description = "Cache a specific crate version from a local file system path. Supports absolute paths, home paths (~), and relative paths. The specified directory must contain a Cargo.toml file."
    )]
    pub async fn cache_crate_from_local(
        &self,
        params: Parameters<CacheCrateFromLocalParams>,
    ) -> String {
        self.cache_tools.cache_crate_from_local(params.0).await
    }

    #[tool(
        description = "Remove a cached crate version from local storage. Use to free up disk space or remove outdated versions. This only affects the local cache - the crate can be re-downloaded later if needed."
    )]
    pub async fn remove_crate(
        &self,
        params: Parameters<crate::cache::tools::RemoveCrateParams>,
    ) -> String {
        self.cache_tools.remove_crate(params.0).await
    }

    #[tool(
        description = "List all locally cached crates with their versions and sizes. Use to see what crates are available offline and how much disk space they use. Shows cache metadata including when each crate was cached."
    )]
    pub async fn list_cached_crates(&self) -> String {
        self.cache_tools.list_cached_crates().await
    }

    #[tool(
        description = "List all locally cached versions of a crate. Use to check what versions are available offline without downloading. Useful before calling other tools to verify if a version needs to be cached first."
    )]
    pub async fn list_crate_versions(
        &self,
        params: Parameters<crate::cache::tools::ListCrateVersionsParams>,
    ) -> String {
        self.cache_tools.list_crate_versions(params.0).await
    }

    #[tool(
        description = "Get metadata for multiple crates and their workspace members in a single call. Use this to efficiently check the caching and analysis status of multiple crates at once. Returns metadata including caching status, analysis status, and cache sizes for each requested crate and member."
    )]
    pub async fn get_crates_metadata(
        &self,
        params: Parameters<crate::cache::tools::GetCratesMetadataParams>,
    ) -> String {
        self.cache_tools.get_crates_metadata(params.0).await
    }

    // Docs tools
    #[tool(
        description = "List all items in a crate's documentation. Use when browsing a crate's contents without a specific search term. Returns full item details including documentation. For large crates, consider using search_items_preview for a lighter response that only includes names and types. For workspace crates, specify the member parameter with the member path (e.g., 'crates/rmcp')."
    )]
    pub async fn list_crate_items(
        &self,
        params: Parameters<crate::docs::tools::ListItemsParams>,
    ) -> String {
        self.docs_tools.list_crate_items(params.0).await
    }

    #[tool(
        description = "Search for items by name pattern in a crate. Use when looking for specific functions, types, or modules. Returns FULL details including documentation. WARNING: May exceed token limits for large results. Use search_items_preview first for exploration, then get_item_details for specific items. For workspace crates, specify the member parameter with the member path (e.g., 'crates/rmcp')."
    )]
    pub async fn search_items(
        &self,
        params: Parameters<crate::docs::tools::SearchItemsParams>,
    ) -> String {
        self.docs_tools.search_items(params.0).await
    }

    #[tool(
        description = "Search for items by name pattern in a crate - PREVIEW MODE. Use this FIRST when searching to avoid token limits. Returns only id, name, kind, and path. Once you find items of interest, use get_item_details to fetch full documentation. This is the recommended search method for exploration. For workspace crates, specify the member parameter with the member path (e.g., 'crates/rmcp')."
    )]
    pub async fn search_items_preview(
        &self,
        params: Parameters<crate::docs::tools::SearchItemsPreviewParams>,
    ) -> String {
        self.docs_tools.search_items_preview(params.0).await
    }

    #[tool(
        description = "Get detailed information about a specific item by ID. Use after search_items_preview to fetch full details including documentation, signatures, fields, methods, etc. The item_id comes from search results. This is the recommended way to get complete information about a specific item. For workspace crates, specify the member parameter with the member path (e.g., 'crates/rmcp')."
    )]
    pub async fn get_item_details(
        &self,
        params: Parameters<crate::docs::tools::GetItemDetailsParams>,
    ) -> String {
        self.docs_tools.get_item_details(params.0).await
    }

    #[tool(
        description = "Get ONLY the documentation string for a specific item. Use when you need just the docs without other details. More efficient than get_item_details if you only need the documentation text. Returns null if no documentation exists. For workspace crates, specify the member parameter with the member path (e.g., 'crates/rmcp')."
    )]
    pub async fn get_item_docs(
        &self,
        params: Parameters<crate::docs::tools::GetItemDocsParams>,
    ) -> String {
        self.docs_tools.get_item_docs(params.0).await
    }

    #[tool(
        description = "Get the source code for a specific item. Returns the actual source code with optional context lines. Use after finding items of interest to view their implementation. The source location is also included in get_item_details responses. For workspace crates, specify the member parameter with the member path (e.g., 'crates/rmcp')."
    )]
    pub async fn get_item_source(
        &self,
        params: Parameters<crate::docs::tools::GetItemSourceParams>,
    ) -> String {
        self.docs_tools.get_item_source(params.0).await
    }

    // Deps tools
    #[tool(
        description = "Get dependency information for a crate. Returns direct dependencies by default, with option to include full dependency tree. Use this to understand what a crate depends on, check for version conflicts, or explore the dependency graph. For workspace crates, specify the member parameter with the member path (e.g., 'crates/rmcp')."
    )]
    pub async fn get_dependencies(
        &self,
        params: Parameters<crate::deps::tools::GetDependenciesParams>,
    ) -> String {
        self.deps_tools.get_dependencies(params.0).await
    }

    // Analysis tools
    #[tool(
        description = "View the hierarchical structure as a tree to view the high level components of the crate. This is a good starting point to have a high-level overview of the crate's organization. This will allow you to narrow down your search confidently to find what you are looking for."
    )]
    pub async fn structure(
        &self,
        params: Parameters<crate::analysis::tools::AnalyzeCrateStructureParams>,
    ) -> String {
        self.analysis_tools.structure(params.0).await
    }
}

async fn find_source_code_prompt_template(
    _service: &RustDocsService,
    Arguments(args): Arguments<FindImplementationArgs>,
    _ctx: RequestContext<RoleServer>,
) -> Result<Vec<PromptMessage>, rmcp::Error> {
    let mut messages = vec![];

    messages.push(PromptMessage::new_text(
        PromptMessageRole::User,
        format!(
            "Find implementations of '{}' in crate '{}'{}. Ensure to first check the version crate that is in the Cargo.toml file. If it is not there, then ask the user for the version. If it is a github url or local path dependency, then you can use those.",
            args.query,
            args.crate_name,
            args.member_name
                .as_ref()
                .map(|m| format!(" (member: {})", m))
                .unwrap_or_default()
        ),
    ));

    Ok(messages)
}

#[tool_handler(router = self.tool_router)]
impl ServerHandler for RustDocsService {
    fn get_info(&self) -> ServerInfo {
        ServerInfo {
            server_info: rmcp::model::Implementation {
                name: "rust-docs-mcp".to_string(),
                version: "0.1.0".to_string(),
            },
            capabilities: ServerCapabilities {
                tools: Some(Default::default()),
                prompts: Some(Default::default()),
                ..Default::default()
            },
            instructions: Some(
                "MCP server for analyzing crate structure and querying documentation, dependencies and source code. Use the structure tool to get a high-level overview of the crate's organization before narrowing down your search. Use list_cached_crates to see what crates are already cached and to easily find the crate or member from a workspace crate instead of guessing. Common workflow: search_items_preview to find items quickly by symbol name, then get_item_details to fetch full documentation. Use get_item_source to view the actual source code of items. Use get_dependencies to understand a crate's dependency graph.".to_string(),
            ),
            ..Default::default()
        }
    }

    async fn list_prompts(
        &self,
        _request: Option<PaginatedRequestParam>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListPromptsResult, rmcp::Error> {
        Ok(ListPromptsResult {
            next_cursor: None,
            prompts: vec![rmcp::model::Prompt {
                name: "find_implementation".to_string(),
                description: Some(
                    "Find implementations of specific functionality within a Rust crate"
                        .to_string(),
                ),
                arguments: rmcp::handler::server::prompt::arguments_from_schema::<
                    FindImplementationArgs,
                >(),
            }],
        })
    }

    async fn get_prompt(
        &self,
        request: GetPromptRequestParam,
        context: RequestContext<RoleServer>,
    ) -> Result<GetPromptResult, rmcp::Error> {
        match request.name.as_str() {
            "find_implementation" => {
                let args = if let Some(args_obj) = request.arguments {
                    serde_json::from_value::<FindImplementationArgs>(serde_json::Value::Object(
                        args_obj,
                    ))
                    .map_err(|e| {
                        rmcp::Error::invalid_params(format!("Invalid arguments: {}", e), None)
                    })?
                } else {
                    return Err(rmcp::Error::invalid_params(
                        "Missing required arguments",
                        None,
                    ));
                };

                let messages =
                    find_source_code_prompt_template(self, Arguments(args), context).await?;

                Ok(GetPromptResult {
                    description: None,
                    messages,
                })
            }
            _ => Err(rmcp::Error::invalid_params("Prompt not found", None)),
        }
    }
}
