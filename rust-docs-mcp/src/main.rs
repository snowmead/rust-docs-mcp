use anyhow::Result;
use clap::Parser;
use rmcp::{ServiceExt, transport::stdio};
use std::path::PathBuf;
use tracing_subscriber::EnvFilter;

mod analysis;
mod cache;
mod deps;
mod docs;
mod service;
use service::RustDocsService;

/// MCP server for querying Rust crate documentation with offline caching
#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// Custom cache directory path (defaults to ~/.rust-docs-mcp/cache)
    #[arg(long, env = "RUST_DOCS_MCP_CACHE_DIR")]
    cache_dir: Option<PathBuf>,
}

#[tokio::main]
async fn main() -> Result<()> {
    // Parse command line arguments
    let args = Args::parse();

    // Initialize tracing to stderr to avoid conflicts with stdio transport
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env().add_directive(tracing::Level::INFO.into()))
        .with_writer(std::io::stderr)
        .with_ansi(false)
        .init();

    tracing::info!("Starting MCP Rust Docs server on stdio...");
    if let Some(ref cache_dir) = args.cache_dir {
        tracing::info!("Using custom cache directory: {}", cache_dir.display());
    }

    // Create the service with optional cache directory
    let rust_docs_service = RustDocsService::new(args.cache_dir)?;

    // Serve using stdio transport
    let service = rust_docs_service.serve(stdio()).await.inspect_err(|e| {
        tracing::error!("serving error: {:?}", e);
    })?;

    // Wait for the service to complete
    service.waiting().await?;
    Ok(())
}
