//! # Search Indexer Module
//!
//! Provides Tantivy-based indexing for Rust documentation search.
//!
//! ## Key Components
//! - [`SearchIndexer`] - Main indexer for creating and managing search indices
//! - [`IndexFields`] - Schema definition for indexed fields
//!
//! ## Example
//! ```no_run
//! # use std::path::Path;
//! # use anyhow::Result;
//! # use rust_docs_mcp::search::indexer::SearchIndexer;
//! # use rust_docs_mcp::cache::storage::CacheStorage;
//! # fn main() -> Result<()> {
//! let storage = CacheStorage::new(None)?;
//! let mut indexer = SearchIndexer::new_for_crate("tokio", "1.35.0", &storage, None)?;
//! // Add crate items to index
//! # Ok(())
//! # }
//! ```

use crate::cache::storage::CacheStorage;
use crate::docs::query::{DocQuery, ItemInfo};
use crate::search::config::{DEFAULT_BUFFER_SIZE, MAX_BUFFER_SIZE, MAX_ITEMS_PER_CRATE};
use anyhow::{Context, Result};
use rustdoc_types::Crate;
use std::path::{Path, PathBuf};
use tantivy::{
    Index, IndexWriter, TantivyDocument, doc,
    schema::{FAST, Field, STORED, STRING, Schema, TEXT},
};

/// Tantivy-based search indexer for Rust documentation
pub struct SearchIndexer {
    index: Index,
    fields: IndexFields,
    writer: Option<IndexWriter>,
    index_path: PathBuf,
    member: Option<String>,
}

#[derive(Debug, Clone)]
pub struct IndexFields {
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

impl SearchIndexer {
    /// Create a new search indexer instance for a specific crate
    pub fn new_for_crate(
        crate_name: &str,
        version: &str,
        storage: &CacheStorage,
        member: Option<&str>,
    ) -> Result<Self> {
        let index_path = storage.search_index_path(crate_name, version, member)?;

        let mut indexer = Self::new_at_path(&index_path)?;
        indexer.member = member.map(|s| s.to_string());
        Ok(indexer)
    }

    /// Create a new search indexer instance at a specific path
    pub fn new_at_path(index_path: &Path) -> Result<Self> {
        let mut schema_builder = Schema::builder();

        // Searchable fields
        let name_field = schema_builder.add_text_field("name", TEXT | STORED);
        let docs_field = schema_builder.add_text_field("docs", TEXT);
        let path_field = schema_builder.add_text_field("path", TEXT | STORED);
        let kind_field = schema_builder.add_text_field("kind", STRING | STORED);

        // Metadata fields
        let crate_field = schema_builder.add_text_field("crate", STRING | STORED);
        let version_field = schema_builder.add_text_field("version", STRING | STORED);
        let item_id_field = schema_builder.add_u64_field("item_id", FAST | STORED);
        let visibility_field = schema_builder.add_text_field("visibility", TEXT | STORED);
        let member_field = schema_builder.add_text_field("member", STRING | STORED);

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
            member: member_field,
        };

        // Create index directory
        std::fs::create_dir_all(index_path).with_context(|| {
            format!(
                "Failed to create search index directory: {}",
                index_path.display()
            )
        })?;

        let index = match Index::open_in_dir(index_path) {
            Ok(index) => index,
            Err(_) => Index::create_in_dir(index_path, schema.clone()).with_context(|| {
                format!("Failed to create search index at: {}", index_path.display())
            })?,
        };

        Ok(Self {
            index,
            fields,
            writer: None,
            index_path: index_path.to_path_buf(),
            member: None,
        })
    }

    /// Get or create an IndexWriter with proper buffer size
    fn get_writer(&mut self) -> Result<&mut IndexWriter> {
        if self.writer.is_none() {
            let buffer_size = std::cmp::min(DEFAULT_BUFFER_SIZE, MAX_BUFFER_SIZE);
            let writer = self.index.writer(buffer_size)?;
            self.writer = Some(writer);
        }
        self.writer
            .as_mut()
            .ok_or_else(|| anyhow::anyhow!("IndexWriter not initialized"))
    }

    /// Add crate items to the search index
    pub fn add_crate_items(
        &mut self,
        crate_name: &str,
        version: &str,
        crate_data: &Crate,
        progress_callback: Option<crate::cache::downloader::ProgressCallback>,
    ) -> Result<()> {
        let query = DocQuery::new(crate_data.clone());
        let items = query.list_items(None); // Get all items without filtering

        // Limit number of items to prevent resource exhaustion
        if items.len() > MAX_ITEMS_PER_CRATE {
            return Err(anyhow::anyhow!(
                "Crate has too many items ({}), max allowed: {}",
                items.len(),
                MAX_ITEMS_PER_CRATE
            ));
        }

        self.add_items_to_index(crate_name, version, &items, progress_callback)?;
        Ok(())
    }

    /// Add items to the search index
    fn add_items_to_index(
        &mut self,
        crate_name: &str,
        version: &str,
        items: &[ItemInfo],
        progress_callback: Option<crate::cache::downloader::ProgressCallback>,
    ) -> Result<()> {
        let total_items = items.len();

        // Create all documents first
        let mut documents = Vec::new();
        for (i, item) in items.iter().enumerate() {
            let doc = self.create_document_from_item(crate_name, version, item)?;
            documents.push(doc);

            // Report progress every 50 items during document creation (0-70%)
            if let Some(ref callback) = progress_callback
                && (i % 50 == 0 || i == total_items - 1) {
                    let percent = ((i * 70) / total_items.max(1)).min(70) as u8;
                    callback(percent);
                }
        }

        // Then add all documents to the writer
        let writer = self.get_writer()?;
        for (i, doc) in documents.iter().enumerate() {
            writer.add_document(doc.clone())?;

            // Report progress during writing (70-95%)
            if let Some(ref callback) = progress_callback
                && (i % 50 == 0 || i == documents.len() - 1) {
                    let percent = (70 + ((i * 25) / documents.len().max(1))).min(95) as u8;
                    callback(percent);
                }
        }

        writer.commit()?;

        // Report 100% complete
        if let Some(callback) = progress_callback {
            callback(100);
        }

        Ok(())
    }

    /// Create a Tantivy document from an ItemInfo
    fn create_document_from_item(
        &self,
        crate_name: &str,
        version: &str,
        item: &ItemInfo,
    ) -> Result<TantivyDocument> {
        let item_id: u64 = item
            .id
            .parse()
            .with_context(|| format!("Failed to parse item ID: {}", item.id))?;

        let path_str = item.path.join("::");
        let docs_str = item.docs.clone().unwrap_or_default();

        let mut doc = doc!(
            self.fields.name => item.name.clone(),
            self.fields.docs => docs_str,
            self.fields.path => path_str,
            self.fields.kind => item.kind.clone(),
            self.fields.crate_name => crate_name.to_string(),
            self.fields.version => version.to_string(),
            self.fields.item_id => item_id,
            self.fields.visibility => item.visibility.clone(),
        );

        // Add member field if present
        if let Some(member_name) = &self.member {
            doc.add_text(self.fields.member, member_name.clone());
        }

        Ok(doc)
    }

    /// Check if the index has any documents
    pub fn has_documents(&self) -> Result<bool> {
        let reader = self.index.reader()?;
        let searcher = reader.searcher();
        let count = searcher.num_docs();
        Ok(count > 0)
    }

    /// Get the underlying Tantivy index
    pub fn get_index(&self) -> &Index {
        &self.index
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

    pub fn get_member_field(&self) -> Field {
        self.fields.member
    }
}

impl std::fmt::Debug for SearchIndexer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SearchIndexer")
            .field("index", &"<Index>")
            .field("fields", &self.fields)
            .field("writer", &self.writer.is_some())
            .field("index_path", &self.index_path)
            .field("member", &self.member)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_create_indexer() {
        let temp_dir = TempDir::new().expect("Failed to create temporary directory for test");
        let index_path = temp_dir.path().join("test_index");
        let indexer = SearchIndexer::new_at_path(&index_path)
            .expect("Failed to create search indexer for test");
        assert!(
            indexer
                .get_index()
                .searchable_segment_ids()
                .expect("Failed to get searchable segment IDs")
                .is_empty()
        );
    }

    #[test]
    fn test_crate_name_validation() {
        let temp_dir = TempDir::new().expect("Failed to create temporary directory for test");
        let storage = CacheStorage::new(Some(temp_dir.path().to_path_buf()))
            .expect("Failed to create storage");
        let indexer = SearchIndexer::new_for_crate("test-crate", "1.0.0", &storage, None)
            .expect("Failed to create search indexer for test");

        // The add_crate_items method is tested integration-wise since it requires a real Crate
        // Here we just test that the indexer can be created successfully
        assert!(
            indexer
                .get_index()
                .searchable_segment_ids()
                .expect("Failed to get searchable segment IDs")
                .is_empty()
        );
    }
}
