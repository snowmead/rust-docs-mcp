# 🦀 rust-docs-mcp

> *Rust is the language of AI*

An MCP (Model Context Protocol) server that gives AI agents superpowers to explore Rust crate documentation, analyze source code, and build with confidence.

## ⚡ Quick Install

```bash
curl -sSL https://raw.githubusercontent.com/snowmead/rust-docs-mcp/main/install.sh | bash
```

## ✨ Features

- 🔍 **Search & browse** Rust crate documentation with AI precision
- 📖 **View detailed info** about structs, functions, traits, and modules
- 📄 **Retrieve source code** with line-level precision
- 🌳 **Explore dependency trees** to understand relationships and resolve conflicts
- 💾 **Automatic caching** for lightning-fast offline access
- 🚀 **Efficient preview mode** to respect token limits

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

## 🔧 Available MCP Tools

| Tool | Description |
|------|-------------|
| `search_items_preview` | 🔍 Search items with minimal info (token-friendly) |
| `search_items` | 📋 Search with full documentation |
| `list_crate_items` | 📂 List all items in a crate |
| `get_item_details` | 📖 Get detailed item information |
| `get_item_docs` | 📄 Get documentation for an item |
| `get_item_source` | 💻 View source code of an item |
| `get_dependencies` | 🌳 Get crate dependency info |
| `cache_crate` | 💾 Pre-cache crate for offline use |
| `list_cached_crates` | 📦 List all cached crates |
| `remove_crate` | 🗑️ Remove cached crate |

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
Customize cache location (default: `~/.rust-docs-mcp/cache/`):

```bash
# Command line
rust-docs-mcp --cache-dir /custom/path

# Environment variable
export RUST_DOCS_MCP_CACHE_DIR=/custom/path
rust-docs-mcp
```

## 📋 Requirements

- **Rust nightly** (auto-installed by script)
  ```bash
  rustup toolchain install nightly
  ```
- **Network access** to download from [crates.io](https://crates.io)

## 📁 Data Storage

Cache structure (default: `~/.rust-docs-mcp/cache/`):
```
~/.rust-docs-mcp/cache/
├── crate-name/
│   └── version/
│       ├── source/           # Source code
│       ├── docs.json         # Rustdoc JSON
│       └── dependencies.json # Dependency metadata
```

---

**🎯 Ready to supercharge your Rust development with AI?**  
Install now: `curl -sSL https://raw.githubusercontent.com/snowmead/rust-docs-mcp/main/install.sh | bash`
