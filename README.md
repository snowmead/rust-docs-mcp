[![rust-docs banner](./assets/rust_docs_banner.jpeg)](https://github.com/snowmead/rust-docs-mcp)

# 🦀 rust-docs-mcp

> *Rust is the language of AI*

An MCP (Model Context Protocol) server that provides comprehensive access to Rust crate documentation, source code analysis, dependency trees, and module structure visualization. Built for agents to gain quality insights into Rust projects and build with confidence.

## ⚡ Quick Install

```bash
curl -sSL https://raw.githubusercontent.com/snowmead/rust-docs-mcp/main/install.sh | bash
```

## ✨ Agent Capabilities

- [x] **Multi-source caching** — crates.io, GitHub repositories, local filesystem paths
- [x] **Workspace support** — Individual member analysis and caching for cargo workspaces
- [x] **Documentation search** — Pattern matching with kind/path filtering and preview modes
- [x] **Item inspection** — Detailed signatures, fields, methods, and documentation strings
- [x] **Source code access** — Line-level precision with parameterized surrounding context
- [x] **Dependency analysis** — Direct and transitive dependency trees with metadata
- [x] **Module structure** — Hierarchical tree generation via cargo-modules integration
- [x] **Offline operation** — Full functionality after initial crate caching
- [x] **Token management** — Response truncation and preview modes for LLM compatibility

## 🛠️ Installation Options

### One-liner (Recommended)
```bash
curl -sSL https://raw.githubusercontent.com/snowmead/rust-docs-mcp/main/install.sh | bash
```

### Custom install directory
```bash
curl -sSL https://raw.githubusercontent.com/snowmead/rust-docs-mcp/main/install.sh | bash -s -- --install-dir /usr/local/bin
```

### Manual build from source
```bash
git clone https://github.com/snowmead/rust-docs-mcp
cd rust-docs-mcp/rust-docs-mcp
cargo build --release
./target/release/rust-docs-mcp install
```

### CLI Commands
```bash
rust-docs-mcp                    # Start MCP server
rust-docs-mcp install           # Install to ~/.local/bin
rust-docs-mcp install --force   # Force overwrite existing installation
rust-docs-mcp --help            # Show help
```

## 🔧 MCP Tools

### Cache Management

- `cache_crate` - Download and cache crates from crates.io, GitHub, or local paths
- `remove_crate` - Remove cached crate versions to free disk space
- `list_cached_crates` - View all cached crates with versions and sizes
- `list_crate_versions` - List cached versions for a specific crate
- `get_crates_metadata` - Batch metadata queries for multiple crates

### Documentation Queries

- `search_items_preview` - Lightweight search returning only IDs, names, and types
- `search_items` - Full search with complete documentation (may hit token limits)
- `list_crate_items` - Browse all items in a crate with optional filtering
- `get_item_details` - Detailed information about specific items (signatures, fields, etc.)
- `get_item_docs` - Extract just the documentation string for an item
- `get_item_source` - View source code with configurable context lines

### Dependency Analysis

- `get_dependencies` - Analyze direct and transitive dependencies with filtering

### Structure Analysis

- `structure` - Generate hierarchical module tree using integrated cargo-modules

## ⚙️ Configuration

### MCP Setup
Add to your MCP configuration file:

```json
{
  "rust-docs": {
    "command": "rust-docs-mcp",
    "transport": "stdio"
  }
}
```

### Cache Directory

By default, crates are cached in `~/.rust-docs-mcp/cache/`. You can customize this location using:

```bash
# Command line option
rust-docs-mcp --cache-dir /custom/path/to/cache

# Environment variable
export RUST_DOCS_MCP_CACHE_DIR=/custom/path/to/cache
rust-docs-mcp
```

## 📋 Requirements

- **Rust nightly** (auto-installed by script)
  ```bash
  rustup toolchain install nightly
  ```
- **Network access** to download from [crates.io](https://crates.io)

## 📁 Data Storage

### Cache Structure

```
~/.rust-docs-mcp/cache/
├── crate-name/
│   └── version/
│       ├── source/                    # Complete source code
│       ├── metadata.json              # Cache metadata and timestamps
│       ├── members/                   # For workspace crates
│       │   └── {member-name}/
│       │       ├── docs.json          # Rustdoc JSON documentation
│       │       ├── dependencies.json  # Cargo dependency metadata
│       │       └── metadata.json      # Member-specific cache metadata
│       ├── docs.json                  # For single crates
│       └── dependencies.json          # For single crates
```

---

**🎯 Ready to supercharge your Rust development with AI?**  
Install now: `curl -sSL https://raw.githubusercontent.com/snowmead/rust-docs-mcp/main/install.sh | bash`