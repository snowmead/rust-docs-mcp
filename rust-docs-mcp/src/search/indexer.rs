use std::path::{Path, PathBuf};
use std::collections::HashMap;
use anyhow::{Context, Result};
use tantivy::{
    schema::{Schema, Field, STORED, TEXT, FAST},
    Index, IndexWriter, Document, Term,
    doc
};
use rustdoc_types::Crate;
use crate::docs::query::{DocQuery, ItemInfo};

/// Tantivy-based search indexer for Rust documentation
pub struct SearchIndexer {
    index: Index,
    schema: Schema,
    fields: IndexFields,
    writer: Option<IndexWriter>,
    cache_dir: PathBuf,
}

#[derive(Debug, Clone)]
struct IndexFields {
    name: Field,
    docs: Field,
    path: Field,
    kind: Field,
    crate_name: Field,
    version: Field,
    item_id: Field,
    visibility: Field,
}

impl SearchIndexer {
    /// Create a new search indexer instance
    pub fn new(cache_dir: &Path) -> Result<Self> {
        let mut schema_builder = Schema::builder();
        
        // Searchable fields
        let name_field = schema_builder.add_text_field("name", TEXT | STORED);
        let docs_field = schema_builder.add_text_field("docs", TEXT);
        let path_field = schema_builder.add_text_field("path", TEXT | STORED);
        let kind_field = schema_builder.add_text_field("kind", TEXT | STORED);
        
        // Metadata fields
        let crate_field = schema_builder.add_text_field("crate", TEXT | STORED);
        let version_field = schema_builder.add_text_field("version", TEXT | STORED);
        let item_id_field = schema_builder.add_u64_field("item_id", FAST | STORED);
        let visibility_field = schema_builder.add_text_field("visibility", TEXT | STORED);
        
        let schema = schema_builder.build();
        
        let fields = IndexFields {
            name: name_field,
            docs: docs_field,
            path: path_field,
            kind: kind_field,
            crate_name: crate_field,
            version: version_field,
            item_id: item_id_field,
            visibility: visibility_field,
        };
        
        // Create index in cache directory
        let index_path = cache_dir.join("search_index");
        std::fs::create_dir_all(&index_path)
            .with_context(|| format!("Failed to create search index directory: {}", index_path.display()))?;
        
        let index = Index::create_in_dir(&index_path, schema.clone())
            .with_context(|| format!("Failed to create search index at: {}", index_path.display()))?;
        
        Ok(Self {
            index,
            schema,
            fields,
            writer: None,
            cache_dir: cache_dir.to_path_buf(),
        })
    }
    
    /// Get or create an IndexWriter with proper buffer size
    fn get_writer(&mut self) -> Result<&mut IndexWriter> {
        if self.writer.is_none() {
            // Use a configurable buffer size with a reasonable default
            const DEFAULT_BUFFER_SIZE: usize = 50_000_000; // 50MB
            const MAX_BUFFER_SIZE: usize = 200_000_000; // 200MB max
            
            let buffer_size = std::cmp::min(DEFAULT_BUFFER_SIZE, MAX_BUFFER_SIZE);
            let writer = self.index.writer(buffer_size)?;
            self.writer = Some(writer);
        }
        Ok(self.writer.as_mut().unwrap())
    }
    
    /// Add crate items to the search index
    pub fn add_crate_items(&mut self, crate_name: &str, version: &str, crate_data: &Crate) -> Result<()> {
        // Validate crate name to prevent path traversal
        if crate_name.contains(&['/', '\', '..'][..]) {
            return Err(anyhow::anyhow!("Invalid crate name"));
        }
        
        let query = DocQuery::new(crate_data.clone());
        let items = query.list_items(None); // Get all items without filtering
        
        // Limit number of items to prevent resource exhaustion
        const MAX_ITEMS_PER_CRATE: usize = 100_000;
        if items.len() > MAX_ITEMS_PER_CRATE {
            return Err(anyhow::anyhow!("Crate has too many items ({}), max allowed: {}", items.len(), MAX_ITEMS_PER_CRATE));
        }
        
        self.add_items_to_index(crate_name, version, &items)?;
        Ok(())
    }
    
    /// Add items to the search index
    fn add_items_to_index(&mut self, crate_name: &str, version: &str, items: &[ItemInfo]) -> Result<()> {
        let writer = self.get_writer()?;
        
        for item in items {
            let doc = self.create_document_from_item(crate_name, version, item)?;
            writer.add_document(doc)?;
        }
        
        writer.commit()?;
        Ok(())
    }
    
    /// Create a Tantivy document from an ItemInfo
    fn create_document_from_item(&self, crate_name: &str, version: &str, item: &ItemInfo) -> Result<Document> {
        let item_id: u64 = item.id.parse()
            .with_context(|| format!("Failed to parse item ID: {}", item.id))?;
        
        let path_str = item.path.join("::");
        let docs_str = item.docs.clone().unwrap_or_default();
        
        let doc = doc!(
            self.fields.name => item.name.clone(),
            self.fields.docs => docs_str,
            self.fields.path => path_str,
            self.fields.kind => item.kind.clone(),
            self.fields.crate_name => crate_name.to_string(),
            self.fields.version => version.to_string(),
            self.fields.item_id => item_id,
            self.fields.visibility => item.visibility.clone(),
        );
        
        Ok(doc)
    }
    
    /// Check if a crate is indexed
    pub fn is_crate_indexed(&self, crate_name: &str, version: &str) -> Result<bool> {
        let reader = self.index.reader()?;
        let searcher = reader.searcher();
        
        // Search for any document with this crate name and version
        let crate_term = Term::from_field_text(self.fields.crate_name, crate_name);
        let version_term = Term::from_field_text(self.fields.version, version);
        
        let crate_query = tantivy::query::TermQuery::new(crate_term, tantivy::schema::IndexRecordOption::Basic);
        let version_query = tantivy::query::TermQuery::new(version_term, tantivy::schema::IndexRecordOption::Basic);
        
        let mut boolean_query = tantivy::query::BooleanQuery::new();
        boolean_query.add_clause(crate_query.into(), tantivy::query::Occur::Must);
        boolean_query.add_clause(version_query.into(), tantivy::query::Occur::Must);
        
        let top_docs = searcher.search(&boolean_query, &tantivy::collector::TopDocs::with_limit(1))?;
        
        Ok(!top_docs.is_empty())
    }
    
    /// Remove a crate from the index
    pub fn remove_crate(&mut self, crate_name: &str, version: &str) -> Result<()> {
        let writer = self.get_writer()?;
        
        // Create a query that matches documents with both crate name AND version
        let crate_term = Term::from_field_text(self.fields.crate_name, crate_name);
        let version_term = Term::from_field_text(self.fields.version, version);
        
        let crate_query = tantivy::query::TermQuery::new(crate_term, tantivy::schema::IndexRecordOption::Basic);
        let version_query = tantivy::query::TermQuery::new(version_term, tantivy::schema::IndexRecordOption::Basic);
        
        let mut boolean_query = tantivy::query::BooleanQuery::new();
        boolean_query.add_clause(crate_query.into(), tantivy::query::Occur::Must);
        boolean_query.add_clause(version_query.into(), tantivy::query::Occur::Must);
        
        writer.delete_query(Box::new(boolean_query))?;
        writer.commit()?;
        
        Ok(())
    }
    
    /// Get the underlying Tantivy index
    pub fn get_index(&self) -> &Index {
        &self.index
    }
    
    /// Get the schema fields
    pub fn get_fields(&self) -> &IndexFields {
        &self.fields
    }
    
    /// Get a specific field by name for external access
    pub fn get_name_field(&self) -> Field {
        self.fields.name
    }
    
    pub fn get_docs_field(&self) -> Field {
        self.fields.docs
    }
    
    pub fn get_path_field(&self) -> Field {
        self.fields.path
    }
    
    pub fn get_kind_field(&self) -> Field {
        self.fields.kind
    }
    
    pub fn get_crate_name_field(&self) -> Field {
        self.fields.crate_name
    }
    
    pub fn get_version_field(&self) -> Field {
        self.fields.version
    }
    
    pub fn get_item_id_field(&self) -> Field {
        self.fields.item_id
    }
    
    pub fn get_visibility_field(&self) -> Field {
        self.fields.visibility
    }
    
    /// Get statistics about the index
    pub fn get_stats(&self) -> Result<HashMap<String, serde_json::Value>> {
        let reader = self.index.reader()?;
        let searcher = reader.searcher();
        
        let segment_readers = searcher.segment_readers();
        let total_docs: u64 = segment_readers.iter().map(|r| r.num_docs() as u64).sum();
        let total_segments = segment_readers.len();
        
        let mut stats = HashMap::new();
        stats.insert("total_docs".to_string(), serde_json::Value::Number(total_docs.into()));
        stats.insert("total_segments".to_string(), serde_json::Value::Number(total_segments.into()));
        
        Ok(stats)
    }
    
    /// Optimize the index by merging segments
    pub fn optimize(&mut self) -> Result<()> {
        let writer = self.get_writer()?;
        writer.wait_merging_threads()?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;
    
    #[test]
    fn test_create_indexer() {
        let temp_dir = TempDir::new().unwrap();
        let indexer = SearchIndexer::new(temp_dir.path()).unwrap();
        assert!(indexer.get_index().searchable_segment_ids().unwrap().is_empty());
    }
    
    #[test]
    fn test_crate_name_validation() {
        let temp_dir = TempDir::new().unwrap();
        let mut indexer = SearchIndexer::new(temp_dir.path()).unwrap();
        let crate_data = Crate::default();
        
        // Test invalid crate names
        assert!(indexer.add_crate_items("../evil", "1.0.0", &crate_data).is_err());
        assert!(indexer.add_crate_items("crate/name", "1.0.0", &crate_data).is_err());
        assert!(indexer.add_crate_items("crate\\name", "1.0.0", &crate_data).is_err());
        
        // Test valid crate name
        assert!(indexer.add_crate_items("valid-crate", "1.0.0", &crate_data).is_ok());
    }
    
    #[test]
    fn test_is_crate_indexed() {
        let temp_dir = TempDir::new().unwrap();
        let mut indexer = SearchIndexer::new(temp_dir.path()).unwrap();
        let crate_data = Crate::default();
        
        // Should not be indexed initially
        assert!(!indexer.is_crate_indexed("test-crate", "1.0.0").unwrap());
        
        // Add the crate
        indexer.add_crate_items("test-crate", "1.0.0", &crate_data).unwrap();
        
        // Should now be indexed
        assert!(indexer.is_crate_indexed("test-crate", "1.0.0").unwrap());
        
        // Different version should not be indexed
        assert!(!indexer.is_crate_indexed("test-crate", "2.0.0").unwrap());
    }
}