use std::collections::HashMap;
use anyhow::{Context, Result};
use tantivy::{
    Index, Term,
    query::{FuzzyTermQuery, BooleanQuery, Query, QueryParser, Occur, TermQuery},
    collector::TopDocs,
    schema::Field,
    TantivyDocument,
};
use rmcp::schemars;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use crate::search::indexer::SearchIndexer;

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
}

impl Default for FuzzySearchOptions {
    fn default() -> Self {
        Self {
            fuzzy_enabled: true,
            fuzzy_distance: 1,
            limit: 50,
            kind_filter: None,
            crate_filter: None,
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
        };
        
        // Create query parser for multiple fields
        let query_parser = QueryParser::for_index(
            &index,
            vec![fields.name, fields.docs, fields.path],
        );
        
        Ok(Self {
            index,
            query_parser,
            fields,
        })
    }
    
    /// Perform fuzzy search with the given query and options
    pub fn search(&self, query: &str, options: &FuzzySearchOptions) -> Result<Vec<SearchResult>> {
        let reader = self.index.reader()?;
        let searcher = reader.searcher();
        
        // Build the query based on options
        let search_query = if options.fuzzy_enabled {
            self.build_fuzzy_query(query, options)?
        } else {
            self.build_standard_query(query, options)?
        };
        
        // Execute search
        let top_docs = searcher.search(
            &search_query,
            &TopDocs::with_limit(options.limit),
        )?;
        
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
    fn build_fuzzy_query(&self, query: &str, options: &FuzzySearchOptions) -> Result<Box<dyn Query>> {
        let mut boolean_query = BooleanQuery::new();
        
        // Split query into terms
        let terms: Vec<&str> = query.split_whitespace().collect();
        
        for term in terms {
            let mut term_query = BooleanQuery::new();
            
            // Add fuzzy queries for searchable fields
            for field in &[self.fields.name, self.fields.docs, self.fields.path] {
                let fuzzy_query = FuzzyTermQuery::new(
                    Term::from_field_text(*field, term),
                    options.fuzzy_distance,
                    true, // transpose_cost_one
                );
                term_query.add_clause(fuzzy_query.into(), Occur::Should);
            }
            
            boolean_query.add_clause(term_query.into(), Occur::Should);
        }
        
        // Add crate filter if specified
        if let Some(crate_name) = &options.crate_filter {
            let crate_term = Term::from_field_text(self.fields.crate_name, crate_name);
            let crate_query = TermQuery::new(crate_term, tantivy::schema::IndexRecordOption::Basic);
            boolean_query.add_clause(crate_query.into(), Occur::Must);
        }
        
        Ok(Box::new(boolean_query))
    }
    
    /// Build standard query without fuzzy matching
    fn build_standard_query(&self, query: &str, options: &FuzzySearchOptions) -> Result<Box<dyn Query>> {
        let mut boolean_query = BooleanQuery::new();
        
        // Parse the query using the query parser
        let parsed_query = self.query_parser.parse_query(query)
            .with_context(|| format!("Failed to parse query: {}", query))?;
        boolean_query.add_clause(parsed_query, Occur::Must);
        
        // Add crate filter if specified
        if let Some(crate_name) = &options.crate_filter {
            let crate_term = Term::from_field_text(self.fields.crate_name, crate_name);
            let crate_query = TermQuery::new(crate_term, tantivy::schema::IndexRecordOption::Basic);
            boolean_query.add_clause(crate_query.into(), Occur::Must);
        }
        
        Ok(Box::new(boolean_query))
    }
    
    /// Convert Tantivy document to SearchResult
    fn doc_to_search_result(&self, doc: &TantivyDocument, score: f32) -> Result<Option<SearchResult>> {
        let get_text_field = |field: Field| -> Option<String> {
            doc.get_first(field)?.as_text().map(|s| s.to_string())
        };
        
        let get_u64_field = |field: Field| -> Option<u64> {
            doc.get_first(field)?.as_u64()
        };
        
        let item_id = get_u64_field(self.fields.item_id)? as u32;
        let name = get_text_field(self.fields.name)?;
        let path = get_text_field(self.fields.path)?;
        let kind = get_text_field(self.fields.kind)?;
        let crate_name = get_text_field(self.fields.crate_name)?;
        let version = get_text_field(self.fields.version)?;
        let visibility = get_text_field(self.fields.visibility).unwrap_or_default();
        
        Ok(Some(SearchResult {
            score,
            item_id,
            name,
            path,
            kind,
            crate_name,
            version,
            visibility,
        }))
    }
    
    /// Check if result matches additional filters
    fn matches_filters(&self, result: &SearchResult, options: &FuzzySearchOptions) -> bool {
        if let Some(kind_filter) = &options.kind_filter {
            if result.kind != *kind_filter {
                return false;
            }
        }
        
        true
    }
    
    /// Get search suggestions based on a partial query
    pub fn get_suggestions(&self, partial_query: &str, limit: usize) -> Result<Vec<String>> {
        let reader = self.index.reader()?;
        let searcher = reader.searcher();
        
        // Use fuzzy search to find similar terms
        let mut suggestions = Vec::new();
        let fuzzy_query = FuzzyTermQuery::new(
            Term::from_field_text(self.fields.name, partial_query),
            1, // Low edit distance for suggestions
            true,
        );
        
        let top_docs = searcher.search(&fuzzy_query, &TopDocs::with_limit(limit * 2))?;
        
        for (_, doc_address) in top_docs {
            let doc = searcher.doc(doc_address)?;
            if let Some(name) = doc.get_first(self.fields.name).and_then(|v| v.as_text()) {
                if !suggestions.contains(&name.to_string()) {
                    suggestions.push(name.to_string());
                }
                if suggestions.len() >= limit {
                    break;
                }
            }
        }
        
        Ok(suggestions)
    }
    
    /// Get search statistics
    pub fn get_search_stats(&self) -> Result<HashMap<String, serde_json::Value>> {
        let reader = self.index.reader()?;
        let searcher = reader.searcher();
        
        let segment_readers = searcher.segment_readers();
        let total_docs: u64 = segment_readers.iter().map(|r| r.num_docs() as u64).sum();
        
        let mut stats = HashMap::new();
        stats.insert("total_indexed_items".to_string(), serde_json::Value::Number(total_docs.into()));
        stats.insert("segment_count".to_string(), serde_json::Value::Number(segment_readers.len().into()));
        
        Ok(stats)
    }
}