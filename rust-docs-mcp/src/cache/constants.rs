//! Constants for cache file and directory names

/// Directory names
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

/// Default capacity for the in-memory LRU cache of parsed `rustdoc_types::Crate` objects.
///
/// Most sessions query 1-3 crates. At ~100-150 MB heap per parsed large crate,
/// 5 entries caps at ~500-750 MB worst case while avoiding re-parsing on
/// repeated tool calls to the same crate.
pub const DEFAULT_DOCS_CACHE_CAPACITY: usize = 5;

/// Maximum number of workspace members to process concurrently during caching.
///
/// Each member's `cargo rustdoc` build can consume 200-500 MB of peak memory
/// (compilation artifacts + JSON output + parsing). Limiting to 4 keeps peak
/// heap under ~2 GB on a typical dev machine while still parallelizing well
/// on 4+ core systems. Override via `RUST_DOCS_MCP_MAX_PARALLEL_MEMBERS`.
pub const DEFAULT_MAX_PARALLEL_MEMBERS: usize = 4;
