# rust-docs-mcp

*Rust is the language of AI*

An MCP (Model Context Protocol) server for agents to explore crate docs, analyze source code, and build with confidence.

## Features

- **Search and browse** Rust crate documentation
- **View detailed information** about structs, functions, traits, and other items
- **Retrieve source code** for any documented item with line-level precision
- **Explore dependency trees** to understand crate relationships and resolve version conflicts
- **Automatic caching** of crate documentation for offline access
- **Efficient preview mode** to avoid token limits when exploring large crates

## Available Tools

- **`search_items_preview`** - Search for items by name (returns minimal info to avoid token limits)
- **`search_items`** - Search with full documentation (may exceed token limits)
- **`list_crate_items`** - List all items in a crate
- **`get_item_details`** - Get detailed information about a specific item
- **`get_item_docs`** - Get just the documentation string for an item
- **`get_item_source`** - View the source code of an item
- **`get_dependencies`** - Get dependency information for a crate
- **`cache_crate`** - Pre-cache a crate for offline use
- **`list_crate_versions`** - List cached versions of a specific crate
- **`list_cached_crates`** - List all cached crates with their versions and disk usage
- **`remove_crate`** - Remove a cached crate version

## Configuration

### Cache Directory

By default, crates are cached in `~/.rust-docs-mcp/cache/`. You can customize this location using:

1. **Command line argument:**
   ```bash
   rust-docs-mcp --cache-dir /custom/path/to/cache
   ```

2. **Environment variable:**
   ```bash
   export RUST_DOCS_MCP_CACHE_DIR=/custom/path/to/cache
   rust-docs-mcp
   ```

The cache directory will be created automatically if it doesn't exist.

## Data Storage

- Crates are cached in the configured cache directory (default: `~/.rust-docs-mcp/cache/`)
- Each crate version stores:
  - Source code in `source/` directory
  - Rustdoc JSON in `docs.json`
  - Dependency metadata in `dependencies.json`

## Requirements

- Rust nightly toolchain (for rustdoc JSON generation)

  ```bash
  rustup toolchain install nightly
  ```

- Network access to download crates from [crates.io](https://crates.io)

## Installation

> **Note:** This crate is not yet published to crates.io because it depends on `rmcp` which is awaiting its first release. For now, you'll need to build from source.

### Building from Source

```bash
git clone https://github.com/snowmead/rust-docs-mcp
cd rust-docs-mcp
cargo build --release
```

### MCP Configuration

Add the server to your MCP configuration:

```json
{
  "rust-docs": {
    "command": "/path/to/rust-docs-mcp/target/release/rust-docs-mcp",
    "transport": "stdio"
  }
}
```
