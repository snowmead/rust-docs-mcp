//! # Search Configuration Module
//!
//! Provides configuration constants for search indexing and querying.
//!
//! These constants control resource usage and performance characteristics
//! of the search functionality.

/// Default buffer size for the Tantivy index writer (50MB)
pub const DEFAULT_BUFFER_SIZE: usize = 50_000_000;

/// Maximum buffer size for the Tantivy index writer (200MB)
pub const MAX_BUFFER_SIZE: usize = 200_000_000;

/// Maximum number of items to index per crate
pub const MAX_ITEMS_PER_CRATE: usize = 100_000;

/// Default limit for search results
pub const DEFAULT_SEARCH_LIMIT: usize = 50;

/// Maximum allowed limit for search results
pub const MAX_SEARCH_LIMIT: usize = 1000;

/// Maximum allowed query length in characters
pub const MAX_QUERY_LENGTH: usize = 1000;

/// Default fuzzy distance for typo tolerance
pub const DEFAULT_FUZZY_DISTANCE: u8 = 1;

/// Maximum fuzzy distance allowed
pub const MAX_FUZZY_DISTANCE: u8 = 2;
