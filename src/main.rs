use anyhow::Result;
use rmcp::{ServiceExt, transport::stdio};
use tracing_subscriber::EnvFilter;

mod cache;
mod deps;
mod docs;
mod service;
use service::RustDocsService;

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize tracing to stderr to avoid conflicts with stdio transport
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env().add_directive(tracing::Level::INFO.into()))
        .with_writer(std::io::stderr)
        .with_ansi(false)
        .init();

    tracing::info!("Starting MCP Rust Docs server on stdio...");

    // Create the service
    let rust_docs_service = RustDocsService::new()?;

    // Serve using stdio transport
    let service = rust_docs_service.serve(stdio()).await.inspect_err(|e| {
        tracing::error!("serving error: {:?}", e);
    })?;

    // Wait for the service to complete
    service.waiting().await?;
    Ok(())
}
