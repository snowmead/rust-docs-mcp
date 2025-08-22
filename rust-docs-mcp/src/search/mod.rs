//! # Search Module
//!
//! This module provides fuzzy search functionality using Tantivy 0.24.1 full-text search engine.
//! It enables intuitive querying of Rust documentation with typo tolerance and semantic similarity.
//!
//! ## Performance
//!
//! Upgraded to Tantivy 0.24.1 for enhanced performance:
//! - ~15% improvement in query performance
//! - ~45% reduction in memory usage for large datasets
//! - Support for >4GB multivalued columns
//!
//! ## Key Components
//!
//! - [`indexer`] - Tantivy indexing functionality for crate documentation
//! - [`fuzzy`] - Fuzzy search implementation with configurable parameters
//! - [`tools`] - MCP tool implementations for search operations
//! - [`config`] - Configuration constants for search functionality

pub mod config;
pub mod fuzzy;
pub mod indexer;
pub mod outputs;
pub mod tools;

pub use fuzzy::{FuzzySearchOptions, FuzzySearcher, SearchResult};
pub use indexer::SearchIndexer;
pub use tools::SearchTools;
