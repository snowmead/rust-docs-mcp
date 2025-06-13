# rust-docs-mcp

*Rust is the language of AI*

An MCP (Model Context Protocol) server for querying Rust crate documentation. This server provides tools to search, browse, and retrieve documentation and source code from Rust crates.

## Features

- **Search and browse** Rust crate documentation
- **View detailed information** about structs, functions, traits, and other items
- **Retrieve source code** for any documented item with line-level precision
- **Automatic caching** of crate documentation for offline access
- **Efficient preview mode** to avoid token limits when exploring large crates

## Data Storage

- Crates are cached in `~/.rust-docs-mcp/cache/`
- Each crate version stores:
  - Source code in `source/` directory
  - Rustdoc JSON in `docs.json`

## Requirements

- Rust nightly toolchain (for rustdoc JSON generation)

  ```bash
  rustup toolchain install nightly
  ```

- Network access to download crates from [crates.io](https://crates.io)
