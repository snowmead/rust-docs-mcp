//! # Fuzzy Search Module
//!
//! Provides fuzzy search capabilities with typo tolerance using Tantivy.
//!
//! ## Key Components
//! - [`FuzzySearcher`] - Main searcher with fuzzy and standard search modes
//! - [`FuzzySearchOptions`] - Configuration for search behavior
//! - [`SearchResult`] - Structure containing search result information
//!
//! ## Example
//! ```no_run
//! # use rust_docs_mcp::search::fuzzy::{FuzzySearcher, FuzzySearchOptions};
//! # use rust_docs_mcp::search::indexer::SearchIndexer;
//! # use rust_docs_mcp::cache::storage::CacheStorage;
//! # use anyhow::Result;
//! # fn main() -> Result<()> {
//! let storage = CacheStorage::new(None)?;
//! let indexer = SearchIndexer::new_for_crate("tokio", "1.35.0", &storage, None)?;
//! let searcher = FuzzySearcher::from_indexer(&indexer)?;
//! let options = FuzzySearchOptions {
//!     fuzzy_enabled: true,
//!     fuzzy_distance: 1,
//!     ..Default::default()
//! };
//! let results = searcher.search("Vec", &options)?;
//! # Ok(())
//! # }
//! ```

use crate::search::config::{
    DEFAULT_FUZZY_DISTANCE, DEFAULT_SEARCH_LIMIT, FUZZY_TRANSPOSE_COST_ONE, MAX_QUERY_LENGTH,
};
use crate::search::indexer::SearchIndexer;
use anyhow::{Context, Result};
use rmcp::schemars;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use tantivy::{
    Index, TantivyDocument, Term,
    collector::TopDocs,
    query::{BooleanQuery, FuzzyTermQuery, Occur, Query, QueryParser, TermQuery},
    schema::{Field, Value},
};

/// Fuzzy search implementation using Tantivy
pub struct FuzzySearcher {
    index: Index,
    query_parser: QueryParser,
    fields: FuzzySearchFields,
}

#[derive(Debug, Clone)]
struct FuzzySearchFields {
    name: Field,
    docs: Field,
    path: Field,
    kind: Field,
    crate_name: Field,
    version: Field,
    item_id: Field,
    visibility: Field,
    member: Field,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct FuzzySearchOptions {
    #[schemars(description = "Enable fuzzy matching for typo tolerance")]
    pub fuzzy_enabled: bool,
    #[schemars(description = "Edit distance for fuzzy matching (0-2)")]
    pub fuzzy_distance: u8,
    #[schemars(description = "Maximum number of results to return")]
    pub limit: usize,
    #[schemars(description = "Filter by item kind")]
    pub kind_filter: Option<String>,
    #[schemars(description = "Filter by crate name")]
    pub crate_filter: Option<String>,
    #[schemars(description = "Filter by workspace member")]
    pub member_filter: Option<String>,
}

impl Default for FuzzySearchOptions {
    fn default() -> Self {
        Self {
            fuzzy_enabled: true,
            fuzzy_distance: DEFAULT_FUZZY_DISTANCE,
            limit: DEFAULT_SEARCH_LIMIT,
            kind_filter: None,
            crate_filter: None,
            member_filter: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct SearchResult {
    #[schemars(description = "Relevance score")]
    pub score: f32,
    #[schemars(description = "Item ID")]
    pub item_id: u32,
    #[schemars(description = "Item name")]
    pub name: String,
    #[schemars(description = "Item path")]
    pub path: String,
    #[schemars(description = "Item kind")]
    pub kind: String,
    #[schemars(description = "Crate name")]
    pub crate_name: String,
    #[schemars(description = "Crate version")]
    pub version: String,
    #[schemars(description = "Item visibility")]
    pub visibility: String,
    #[schemars(description = "Workspace member name (if applicable)")]
    pub member: Option<String>,
}

impl FuzzySearcher {
    /// Create a new fuzzy searcher from an indexer
    pub fn from_indexer(indexer: &SearchIndexer) -> Result<Self> {
        let index = indexer.get_index().clone();

        let fields = FuzzySearchFields {
            name: indexer.get_name_field(),
            docs: indexer.get_docs_field(),
            path: indexer.get_path_field(),
            kind: indexer.get_kind_field(),
            crate_name: indexer.get_crate_name_field(),
            version: indexer.get_version_field(),
            item_id: indexer.get_item_id_field(),
            visibility: indexer.get_visibility_field(),
            member: indexer.get_member_field(),
        };

        // Create query parser for multiple fields
        let query_parser =
            QueryParser::for_index(&index, vec![fields.name, fields.docs, fields.path]);

        Ok(Self {
            index,
            query_parser,
            fields,
        })
    }

    /// Perform fuzzy search with the given query and options
    pub fn search(&self, query: &str, options: &FuzzySearchOptions) -> Result<Vec<SearchResult>> {
        // Validate query length
        if query.len() > MAX_QUERY_LENGTH {
            return Err(anyhow::anyhow!(
                "Query too long (max {} characters)",
                MAX_QUERY_LENGTH
            ));
        }

        // Sanitize query to escape special characters
        let sanitized_query = Self::sanitize_query(query);

        let reader = self.index.reader()?;
        let searcher = reader.searcher();

        // Build the query based on options
        let search_query = if options.fuzzy_enabled {
            self.build_fuzzy_query(&sanitized_query, options)?
        } else {
            self.build_standard_query(&sanitized_query, options)?
        };

        // Execute search
        let top_docs = searcher.search(&search_query, &TopDocs::with_limit(options.limit))?;

        // Convert results
        let mut results = Vec::new();
        for (score, doc_address) in top_docs {
            let doc = searcher.doc(doc_address)?;
            if let Some(result) = self.doc_to_search_result(&doc, score)? {
                // Apply additional filters
                if self.matches_filters(&result, options) {
                    results.push(result);
                }
            }
        }

        Ok(results)
    }

    /// Build fuzzy query with typo tolerance
    fn build_fuzzy_query(
        &self,
        query: &str,
        options: &FuzzySearchOptions,
    ) -> Result<Box<dyn Query>> {
        // Split query into terms
        let terms: Vec<&str> = query.split_whitespace().collect();

        let mut main_clauses = Vec::new();

        for term in terms {
            // Build fuzzy queries for this term across all searchable fields
            let mut term_clauses = Vec::new();

            // Add fuzzy queries for searchable fields
            for field in &[self.fields.name, self.fields.docs, self.fields.path] {
                let fuzzy_query = FuzzyTermQuery::new(
                    Term::from_field_text(*field, term),
                    options.fuzzy_distance,
                    FUZZY_TRANSPOSE_COST_ONE,
                );
                term_clauses.push((Occur::Should, Box::new(fuzzy_query) as Box<dyn Query>));
            }

            // Create a boolean query for this term
            let term_query = BooleanQuery::new(term_clauses);
            main_clauses.push((Occur::Should, Box::new(term_query) as Box<dyn Query>));
        }

        // Add crate filter if specified
        if let Some(crate_name) = &options.crate_filter {
            let crate_term = Term::from_field_text(self.fields.crate_name, crate_name);
            let crate_query = TermQuery::new(crate_term, tantivy::schema::IndexRecordOption::Basic);
            main_clauses.push((Occur::Must, Box::new(crate_query) as Box<dyn Query>));
        }

        // Add member filter if specified
        if let Some(member_name) = &options.member_filter {
            let member_term = Term::from_field_text(self.fields.member, member_name);
            let member_query = TermQuery::new(member_term, tantivy::schema::IndexRecordOption::Basic);
            main_clauses.push((Occur::Must, Box::new(member_query) as Box<dyn Query>));
        }

        let boolean_query = BooleanQuery::new(main_clauses);
        Ok(Box::new(boolean_query))
    }

    /// Build standard query without fuzzy matching
    fn build_standard_query(
        &self,
        query: &str,
        options: &FuzzySearchOptions,
    ) -> Result<Box<dyn Query>> {
        let mut clauses = Vec::new();

        // Parse the query using the query parser
        let parsed_query = self
            .query_parser
            .parse_query(query)
            .with_context(|| format!("Failed to parse query: {query}"))?;
        clauses.push((Occur::Must, parsed_query));

        // Add crate filter if specified
        if let Some(crate_name) = &options.crate_filter {
            let crate_term = Term::from_field_text(self.fields.crate_name, crate_name);
            let crate_query = TermQuery::new(crate_term, tantivy::schema::IndexRecordOption::Basic);
            clauses.push((Occur::Must, Box::new(crate_query) as Box<dyn Query>));
        }

        // Add member filter if specified
        if let Some(member_name) = &options.member_filter {
            let member_term = Term::from_field_text(self.fields.member, member_name);
            let member_query = TermQuery::new(member_term, tantivy::schema::IndexRecordOption::Basic);
            clauses.push((Occur::Must, Box::new(member_query) as Box<dyn Query>));
        }

        let boolean_query = BooleanQuery::new(clauses);
        Ok(Box::new(boolean_query))
    }

    /// Convert Tantivy document to SearchResult
    fn doc_to_search_result(
        &self,
        doc: &TantivyDocument,
        score: f32,
    ) -> Result<Option<SearchResult>> {
        let get_text_field = |field: Field| -> Option<String> {
            doc.get_first(field)?.as_str().map(|s| s.to_string())
        };

        let get_u64_field = |field: Field| -> Option<u64> { doc.get_first(field)?.as_u64() };

        let item_id = get_u64_field(self.fields.item_id)
            .ok_or_else(|| anyhow::anyhow!("Missing item_id"))? as u32;
        let name =
            get_text_field(self.fields.name).ok_or_else(|| anyhow::anyhow!("Missing name"))?;
        let path =
            get_text_field(self.fields.path).ok_or_else(|| anyhow::anyhow!("Missing path"))?;
        let kind =
            get_text_field(self.fields.kind).ok_or_else(|| anyhow::anyhow!("Missing kind"))?;
        let crate_name = get_text_field(self.fields.crate_name)
            .ok_or_else(|| anyhow::anyhow!("Missing crate_name"))?;
        let version = get_text_field(self.fields.version)
            .ok_or_else(|| anyhow::anyhow!("Missing version"))?;
        let visibility = get_text_field(self.fields.visibility).unwrap_or_default();
        let member = get_text_field(self.fields.member);

        Ok(Some(SearchResult {
            score,
            item_id,
            name,
            path,
            kind,
            crate_name,
            version,
            visibility,
            member,
        }))
    }

    /// Check if result matches additional filters
    fn matches_filters(&self, result: &SearchResult, options: &FuzzySearchOptions) -> bool {
        if let Some(kind_filter) = &options.kind_filter
            && result.kind != *kind_filter
        {
            return false;
        }

        true
    }

    /// Sanitize query to escape special Tantivy syntax characters
    fn sanitize_query(query: &str) -> String {
        // Escape special characters that have meaning in Tantivy query syntax
        // These include: + - && || ! ( ) { } [ ] ^ " ~ * ? : \ /
        query
            .chars()
            .map(|c| match c {
                '+' | '-' | '!' | '(' | ')' | '{' | '}' | '[' | ']' | '^' | '"' | '~' | '*'
                | '?' | ':' | '\\' | '/' => format!("\\{c}"),
                _ => c.to_string(),
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::search::indexer::SearchIndexer;
    use tempfile::TempDir;

    #[test]
    fn test_sanitize_query() {
        assert_eq!(FuzzySearcher::sanitize_query("hello world"), "hello world");
        assert_eq!(FuzzySearcher::sanitize_query("test+query"), "test\\+query");
        assert_eq!(FuzzySearcher::sanitize_query("(test)"), "\\(test\\)");
        assert_eq!(
            FuzzySearcher::sanitize_query("wild*card?"),
            "wild\\*card\\?"
        );
        assert_eq!(
            FuzzySearcher::sanitize_query("path/to/file"),
            "path\\/to\\/file"
        );
    }

    #[test]
    fn test_fuzzy_search_options_default() {
        let options = FuzzySearchOptions::default();
        assert!(options.fuzzy_enabled);
        assert_eq!(options.fuzzy_distance, 1);
        assert_eq!(options.limit, 50);
        assert!(options.kind_filter.is_none());
        assert!(options.crate_filter.is_none());
        assert!(options.member_filter.is_none());
    }

    #[test]
    fn test_search_query_validation() {
        let temp_dir = TempDir::new().expect("Failed to create temporary directory for test");
        let index_path = temp_dir.path().join("test_index");
        let indexer = SearchIndexer::new_at_path(&index_path)
            .expect("Failed to create search indexer for test");
        let fuzzy_searcher = FuzzySearcher::from_indexer(&indexer)
            .expect("Failed to create fuzzy searcher for test");

        // Test query length validation
        let long_query = "a".repeat(1001);
        let options = FuzzySearchOptions::default();
        let result = fuzzy_searcher.search(&long_query, &options);
        assert!(result.is_err());
        assert!(
            result
                .expect_err("Expected error for query length validation")
                .to_string()
                .contains("Query too long")
        );
    }
}
