# rust-docs-mcp

An MCP (Model Context Protocol) server for querying Rust crate documentation. This server provides tools to search, browse, and retrieve documentation and source code from Rust crates.

## Features

- **Search and browse** Rust crate documentation
- **View detailed information** about structs, functions, traits, and other items
- **Retrieve source code** for any documented item with line-level precision
- **Automatic caching** of crate documentation for offline access
- **Efficient preview mode** to avoid token limits when exploring large crates

## Data Storage

- Crates are cached in `~/.mcp-rust-docs/cache/`
- Each crate version stores:
  - Source code in `source/` directory
  - Rustdoc JSON in `docs.json`

## Source Code Retrieval

The server leverages rustdoc's JSON output which includes source span information:
- Each documented item includes file path and line/column positions
- Source code is extracted from the cached crate source files
- Context lines can be specified to include surrounding code

## Requirements

- Rust nightly toolchain (for rustdoc JSON generation)
- Network access to download crates from crates.io

## License

MIT