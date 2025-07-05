//! # Search Module
//!
//! This module provides fuzzy search functionality using Tantivy full-text search engine.
//! It enables intuitive querying of Rust documentation with typo tolerance and semantic similarity.
//!
//! ## Key Components
//!
//! - [`indexer`] - Tantivy indexing functionality for crate documentation
//! - [`fuzzy`] - Fuzzy search implementation with configurable parameters
//! - [`tools`] - MCP tool implementations for search operations

pub mod indexer;
pub mod fuzzy;
pub mod tools;

pub use indexer::SearchIndexer;
pub use fuzzy::{FuzzySearcher, FuzzySearchOptions, SearchResult};
pub use tools::SearchTools;