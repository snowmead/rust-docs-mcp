//! Constants for cache file and directory names

/// Directory names
pub const CACHE_ROOT_DIR: &str = ".rust-docs-mcp";
pub const CACHE_DIR: &str = "cache";
pub const CRATES_DIR: &str = "crates";
pub const MEMBERS_DIR: &str = "members";
pub const SOURCE_DIR: &str = "source";
pub const SEARCH_INDEX_DIR: &str = "search_index";
pub const TARGET_DIR: &str = "target";
pub const DOC_DIR: &str = "doc";
pub const BACKUP_DIR_PREFIX: &str = "rust-docs-mcp-backup";

/// File names
pub const METADATA_FILE: &str = "metadata.json";
pub const DOCS_FILE: &str = "docs.json";
pub const DEPENDENCIES_FILE: &str = "dependencies.json";

/// Cargo files
pub const CARGO_TOML: &str = "Cargo.toml";
pub const CARGO_LOCK: &str = "Cargo.lock";