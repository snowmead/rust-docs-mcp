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
use crate::docs::query::{item_kind_str, item_name, reexport_paths, visibility_str_cow};
use crate::search::config::{DEFAULT_BUFFER_SIZE, MAX_ITEMS_PER_CRATE};
use crate::search::index_types::{IndexCrate, IndexItem};
use anyhow::{Context, Result};
use rustdoc_types::{Crate, Id, Item};
use std::path::{Path, PathBuf};
use tantivy::{
    Index, IndexWriter, TantivyDocument,
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

#[derive(Debug, Clone, Copy)]
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

/// Build a single [`TantivyDocument`] directly from a rustdoc `Item` without
/// materializing an intermediate [`crate::docs::query::ItemInfo`].
///
/// All text fields are passed as `&str` borrows into tantivy — which is
/// optimal because tantivy's `add_text` writes bytes straight into its
/// internal `node_data: Vec<u8>` regardless of ownership, so there's no
/// benefit to handing it an owned `String`. Passing `String` would just
/// add an allocation that gets dropped immediately after tantivy copies
/// the bytes out.
///
/// Returns `None` for items that don't produce an indexable document —
/// typically anonymous items (impl blocks, etc.) that have neither a
/// direct name nor a path entry in `crate.paths`.
fn create_document_from_item(
    fields: &IndexFields,
    member: Option<&str>,
    crate_name: &str,
    version: &str,
    path: &[String],
    id: &Id,
    item: &Item,
) -> Option<TantivyDocument> {
    let name = item_name(item, path)?;
    let path_str = path.join("::");

    let docs: &str = item.docs.as_deref().unwrap_or("");
    let kind: &'static str = item_kind_str(&item.inner);
    let visibility = visibility_str_cow(&item.visibility);
    let item_id: u64 = id.0 as u64;

    let mut doc = TantivyDocument::default();
    doc.add_text(fields.name, name);
    doc.add_text(fields.docs, docs);
    doc.add_text(fields.path, &path_str);
    doc.add_text(fields.kind, kind);
    doc.add_text(fields.crate_name, crate_name);
    doc.add_text(fields.version, version);
    doc.add_u64(fields.item_id, item_id);
    doc.add_text(fields.visibility, visibility.as_ref());
    if let Some(member_name) = member {
        doc.add_text(fields.member, member_name);
    }

    Some(doc)
}

/// Build a [`TantivyDocument`] from an [`IndexItem`] (the trimmed
/// indexing-only representation) without materialising the full
/// [`rustdoc_types::Item`].
///
/// Mirrors [`create_document_from_item`] but reads `item.kind_tag`
/// directly instead of calling [`item_kind_str`].
fn create_document_from_index_item(
    fields: &IndexFields,
    member: Option<&str>,
    crate_name: &str,
    version: &str,
    path: &[String],
    id: &Id,
    item: &IndexItem,
) -> Option<TantivyDocument> {
    let name = item
        .name
        .as_deref()
        .or_else(|| path.last().map(String::as_str))?;
    let path_str = path.join("::");

    let docs: &str = item.docs.as_deref().unwrap_or("");
    let kind: &str = &item.kind_tag;
    let visibility = visibility_str_cow(&item.visibility);
    let item_id: u64 = id.0 as u64;

    let mut doc = TantivyDocument::default();
    doc.add_text(fields.name, name);
    doc.add_text(fields.docs, docs);
    doc.add_text(fields.path, &path_str);
    doc.add_text(fields.kind, kind);
    doc.add_text(fields.crate_name, crate_name);
    doc.add_text(fields.version, version);
    doc.add_u64(fields.item_id, item_id);
    doc.add_text(fields.visibility, visibility.as_ref());
    if let Some(member_name) = member {
        doc.add_text(fields.member, member_name);
    }

    Some(doc)
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

    /// Get or create an IndexWriter with the configured buffer size.
    ///
    /// The buffer is split across up to 8 tantivy indexing threads, with a
    /// per-thread floor of ~15MB — so [`DEFAULT_BUFFER_SIZE`] needs to be
    /// large enough to feed all cores or we silently lose parallelism.
    fn get_writer(&mut self) -> Result<&mut IndexWriter> {
        if self.writer.is_none() {
            let writer = self.index.writer(DEFAULT_BUFFER_SIZE)?;
            self.writer = Some(writer);
        }
        self.writer
            .as_mut()
            .ok_or_else(|| anyhow::anyhow!("IndexWriter not initialized"))
    }

    /// Add crate items to the search index by streaming them from
    /// `crate.index` directly into the tantivy writer.
    ///
    /// Each item is indexed in place: borrow from `&Item` → build a
    /// [`TantivyDocument`] with all text fields passed as `&str` → hand
    /// it off to the writer → drop. No intermediate `ItemInfo`,
    /// `Vec<ItemInfo>`, or `Vec<TantivyDocument>` is materialized, and
    /// every field is sourced by reference from the parsed [`Crate`].
    /// This is what lets us index crates with hundreds of thousands of
    /// items without peak memory ballooning or per-item allocator churn.
    pub fn add_crate_items(
        &mut self,
        crate_name: &str,
        version: &str,
        crate_data: &Crate,
        progress_callback: Option<crate::cache::downloader::ProgressCallback>,
    ) -> Result<()> {
        // Upper-bound check against the raw index size. The real indexed
        // count will typically be slightly lower (some items, e.g.
        // anonymous impl blocks, don't produce a document), but if even
        // the upper bound exceeds the cap we bail without doing any work.
        let upper_bound = crate_data.index.len();
        if upper_bound > MAX_ITEMS_PER_CRATE {
            return Err(anyhow::anyhow!(
                "Crate has too many items ({upper_bound}), max allowed: {MAX_ITEMS_PER_CRATE}"
            ));
        }

        // Capture `self` state before taking the `&mut` borrow for the
        // writer. `IndexFields` is `Copy`, so this is free. `member` is
        // borrowed by its `as_deref`ed lifetime; we need the owned
        // clone so the reference lives across the writer borrow.
        let fields = self.fields;
        let member_name_owned = self.member.clone();
        let member_name = member_name_owned.as_deref();
        let writer = self.get_writer()?;

        let import_paths = reexport_paths(crate_data);
        let mut indexed = 0usize;
        for (id, item) in crate_data.index.iter() {
            let Some(doc) = create_document_from_item(
                &fields,
                member_name,
                crate_name,
                version,
                import_paths
                    .get(id)
                    .or_else(|| crate_data.paths.get(id).map(|p| &p.path))
                    .map(Vec::as_slice)
                    .unwrap_or_default(),
                id,
                item,
            ) else {
                continue;
            };

            writer.add_document(doc)?;

            indexed += 1;

            // Cheap heartbeat so long-running indexing runs aren't silent.
            if indexed.is_multiple_of(10_000) {
                tracing::info!("Indexed {indexed}/{upper_bound} items for {crate_name}-{version}");
            }

            if let Some(ref callback) = progress_callback
                && indexed.is_multiple_of(50)
            {
                // Reserve the final 5% for the commit step.
                let percent = ((indexed * 95) / upper_bound.max(1)).min(95) as u8;
                callback(percent);
            }
        }

        writer.commit()?;

        tracing::info!(
            "Committed search index for {crate_name}-{version}: {indexed} items indexed \
             (of {upper_bound} in crate.index)"
        );

        if let Some(callback) = progress_callback {
            callback(100);
        }

        Ok(())
    }

    /// Add items from a trimmed [`IndexCrate`] to the search index.
    ///
    /// This is the optimised indexing path: [`IndexCrate`] only carries the
    /// fields the indexer reads, so the deeply recursive `ItemEnum`
    /// subtrees are never materialised. The iteration logic is identical
    /// to [`add_crate_items`](Self::add_crate_items).
    pub fn add_index_crate_items(
        &mut self,
        crate_name: &str,
        version: &str,
        crate_data: &IndexCrate,
        progress_callback: Option<crate::cache::downloader::ProgressCallback>,
    ) -> Result<()> {
        let upper_bound = crate_data.index.len();
        if upper_bound > MAX_ITEMS_PER_CRATE {
            return Err(anyhow::anyhow!(
                "Crate has too many items ({upper_bound}), max allowed: {MAX_ITEMS_PER_CRATE}"
            ));
        }

        let fields = self.fields;
        let member_name_owned = self.member.clone();
        let member_name = member_name_owned.as_deref();
        let writer = self.get_writer()?;

        let import_paths = crate_data.reexport_paths();
        let mut indexed = 0usize;
        for (id, item) in crate_data.index.iter() {
            let Some(doc) = create_document_from_index_item(
                &fields,
                member_name,
                crate_name,
                version,
                import_paths
                    .get(id)
                    .or_else(|| crate_data.paths.get(id).map(|p| &p.path))
                    .map(Vec::as_slice)
                    .unwrap_or_default(),
                id,
                item,
            ) else {
                continue;
            };

            writer.add_document(doc)?;
            indexed += 1;

            if indexed.is_multiple_of(10_000) {
                tracing::info!("Indexed {indexed}/{upper_bound} items for {crate_name}-{version}");
            }

            if let Some(ref callback) = progress_callback
                && indexed.is_multiple_of(50)
            {
                let percent = ((indexed * 95) / upper_bound.max(1)).min(95) as u8;
                callback(percent);
            }
        }

        writer.commit()?;

        tracing::info!(
            "Committed search index for {crate_name}-{version}: {indexed} items indexed \
             (of {upper_bound} in crate.index)"
        );

        if let Some(callback) = progress_callback {
            callback(100);
        }

        Ok(())
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
