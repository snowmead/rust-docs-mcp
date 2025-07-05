use anyhow::Result;
use clap::{Parser, Subcommand};
use rmcp::{ServiceExt, transport::stdio};
use std::path::PathBuf;
use std::process;
use tracing_subscriber::EnvFilter;

mod analysis;
mod cache;
mod deps;
mod docs;
mod health;
mod metrics;
mod service;
mod update;
use service::RustDocsService;

/// MCP server for querying Rust crate documentation with offline caching
#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// Custom cache directory path (defaults to ~/.rust-docs-mcp/cache)
    #[arg(long, env = "RUST_DOCS_MCP_CACHE_DIR")]
    cache_dir: Option<PathBuf>,

    /// Enable HTTP metrics server
    #[arg(long, default_value = "false")]
    enable_metrics: bool,

    /// Metrics server bind address
    #[arg(long, default_value = "127.0.0.1:9090")]
    metrics_addr: String,

    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Install the current executable to a directory in PATH
    Install {
        /// Target directory to install to (defaults to ~/.local/bin)
        #[arg(long)]
        target_dir: Option<PathBuf>,
        /// Force overwrite if file already exists
        #[arg(long)]
        force: bool,
    },
    /// Update rust-docs-mcp to the latest version from GitHub
    Update {
        /// Target directory to install to (defaults to ~/.local/bin)
        #[arg(long)]
        target_dir: Option<PathBuf>,
        /// GitHub repository URL (defaults to repository from Cargo.toml)
        #[arg(long)]
        repo_url: Option<String>,
        /// Specific branch to use (defaults to main)
        #[arg(long)]
        branch: Option<String>,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    // Parse command line arguments
    let args = Args::parse();

    // Handle subcommands
    if let Some(command) = args.command {
        return handle_command(command).await;
    }

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

    // Create optional metrics server
    let metrics_server = if args.enable_metrics {
        let metrics = std::sync::Arc::new(metrics::MetricsServer::new()?);
        let metrics_addr = args.metrics_addr.clone();
        
        // Start metrics server in background
        let metrics_handle = {
            let metrics_server = metrics.clone();
            tokio::spawn(async move {
                if let Err(e) = metrics_server.start_server(&metrics_addr).await {
                    tracing::error!("Metrics server error: {:?}", e);
                }
            })
        };
        
        // Start health check updates
        let health_status = metrics.get_health_status();
        let cache_dir = args.cache_dir.clone();
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(30));
            loop {
                interval.tick().await;
                let mut health = health_status.write().await;
                if let Err(e) = health.update_health_checks(cache_dir.as_deref()).await {
                    tracing::warn!("Health check update error: {:?}", e);
                }
            }
        });
        
        Some((metrics, metrics_handle))
    } else {
        None
    };

    // Create the service with optional cache directory and metrics
    let rust_docs_service = RustDocsService::new(args.cache_dir, metrics_server.as_ref().map(|(m, _)| m.clone()))?;

    // Serve using stdio transport
    let service = rust_docs_service.serve(stdio()).await.inspect_err(|e| {
        tracing::error!("serving error: {:?}", e);
    })?;

    // Wait for the service to complete
    service.waiting().await?;
    Ok(())
}

async fn handle_command(command: Commands) -> Result<()> {
    match command {
        Commands::Install { target_dir, force } => install_executable(target_dir, force).await,
        Commands::Update {
            target_dir,
            repo_url,
            branch,
        } => update::update_executable(target_dir, repo_url, branch).await,
    }
}

async fn install_executable(target_dir: Option<PathBuf>, force: bool) -> Result<()> {
    use std::env;
    use std::fs;

    // Get the current executable path
    let current_exe = env::current_exe()?;

    // Determine target directory
    let target_dir = match target_dir {
        Some(dir) => dir,
        None => {
            // Default to ~/.local/bin
            let home = dirs::home_dir()
                .ok_or_else(|| anyhow::anyhow!("Could not determine home directory"))?;
            home.join(".local").join("bin")
        }
    };

    // Create target directory if it doesn't exist
    fs::create_dir_all(&target_dir)?;

    // Target file path
    let target_file = target_dir.join("rust-docs-mcp");

    // Check if file already exists
    if target_file.exists() && !force {
        eprintln!(
            "Error: {} already exists. Use --force to overwrite.",
            target_file.display()
        );
        process::exit(1);
    }

    // Copy the executable
    fs::copy(&current_exe, &target_file)?;

    // Make it executable on Unix systems
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = fs::metadata(&target_file)?.permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&target_file, perms)?;
    }

    println!(
        "Successfully installed rust-docs-mcp to {}",
        target_file.display()
    );

    // Check if target directory is in PATH
    if let Ok(path_var) = env::var("PATH") {
        let paths: Vec<&str> = path_var.split(':').collect();
        let target_dir_str = target_dir.to_string_lossy();

        if !paths.iter().any(|&p| p == target_dir_str) {
            println!("\nWarning: {} is not in your PATH.", target_dir.display());
            println!(
                "Add the following line to your shell configuration file (.bashrc, .zshrc, etc.):"
            );
            println!("export PATH=\"{}:$PATH\"", target_dir.display());
        } else {
            println!("\nYou can now run 'rust-docs-mcp' from anywhere in your terminal.");
        }
    }

    Ok(())
}
