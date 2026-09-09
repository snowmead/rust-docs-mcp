//! Documentation generation for Rust crates
//!
//! This module handles running `cargo rustdoc` to generate JSON documentation
//! for both regular crates and workspace members.

use crate::cache::constants::*;
use crate::cache::downloader::ProgressCallback;
use crate::cache::storage::CacheStorage;
use crate::cache::workspace::WorkspaceHandler;
use crate::rustdoc;
use crate::search::index_types::IndexCrate;
use crate::search::indexer::SearchIndexer;
use anyhow::{Context, Result, bail};
use fs4::fs_std::FileExt;
use sha2::{Digest, Sha256};
use std::fs::File;
use std::path::{Path, PathBuf};

/// Read and parse a rustdoc JSON file into a [`rustdoc_types::Crate`].
///
/// Uses [`memmap2::Mmap`] + [`serde_json::from_slice`] so that:
/// - the raw JSON bytes live in the page cache, NOT the Rust heap — so
///   `peak_alloc`-style heap trackers don't count them against us, and
///   the process doesn't hold a second copy of a multi-GB file just
///   to feed the parser;
/// - the parser uses the slice-based code path, which is ~1.5-2x faster
///   than `from_reader` because it can backtrack in place instead of
///   pulling one byte at a time through `io::Bytes`.
///
/// Synchronous — wrap in [`tokio::task::spawn_blocking`] when calling
/// from async code, since serde_json's parser is sync and can block for
/// seconds to minutes on large inputs.
///
/// # Safety
///
/// [`memmap2::Mmap::map`] is `unsafe` because the kernel mapping is
/// undefined behaviour if the file is mutated or truncated by another
/// process while the mapping is live. This is sound here because the
/// rust-docs-mcp cache owns the docs.json file for the lifetime of
/// indexing: it's written once by `cargo rustdoc` under the cache lock
/// and nothing else touches it until the index build completes.
fn read_crate_from_json(path: &Path) -> Result<rustdoc_types::Crate> {
    let file = File::open(path)
        .with_context(|| format!("Failed to open documentation file: {}", path.display()))?;
    // SAFETY: see the function-level `# Safety` note — docs.json is
    // cache-owned and not mutated concurrently during indexing.
    let mmap = unsafe {
        memmap2::Mmap::map(&file)
            .with_context(|| format!("Failed to mmap documentation file: {}", path.display()))?
    };
    serde_json::from_slice(&mmap)
        .with_context(|| format!("Failed to parse documentation JSON: {}", path.display()))
}

/// Parse a rustdoc JSON file into an [`IndexCrate`], which only
/// deserialises the fields needed by the search indexer.
///
/// Uses the same mmap approach as [`read_crate_from_json`] but skips ~90%
/// of per-item allocation by not materialising `ItemEnum` subtrees.
pub fn read_crate_for_indexing(path: &Path) -> Result<IndexCrate> {
    let file = File::open(path)
        .with_context(|| format!("Failed to open documentation file: {}", path.display()))?;
    // SAFETY: same rationale as `read_crate_from_json` — docs.json is
    // cache-owned and not mutated concurrently during indexing.
    let mmap = unsafe {
        memmap2::Mmap::map(&file)
            .with_context(|| format!("Failed to mmap documentation file: {}", path.display()))?
    };
    serde_json::from_slice(&mmap).with_context(|| {
        format!(
            "Failed to parse documentation JSON for indexing: {}",
            path.display()
        )
    })
}

/// Make `read_crate_from_json` accessible for benchmarks.
pub fn read_crate_from_json_pub(path: &Path) -> Result<rustdoc_types::Crate> {
    read_crate_from_json(path)
}

/// Service for generating documentation from Rust crates
#[derive(Debug, Clone)]
pub struct DocGenerator {
    storage: CacheStorage,
}

impl DocGenerator {
    /// Create a new documentation generator
    pub fn new(storage: CacheStorage) -> Self {
        Self { storage }
    }

    fn with_legacy_features(&self, features: Option<Vec<String>>) -> Result<Self> {
        let options = self.storage.features.with_feature_list(features)?;
        Ok(Self::new(self.storage.with_options(options)))
    }

    pub async fn generate_docs(
        &self,
        name: &str,
        version: &str,
        progress_callback: Option<ProgressCallback>,
        features: Option<Vec<String>>,
    ) -> Result<PathBuf> {
        self.with_legacy_features(features)?
            .generate(name, version, None, progress_callback)
            .await
    }

    pub async fn generate_workspace_member_docs(
        &self,
        name: &str,
        version: &str,
        member_path: &str,
        progress_callback: Option<ProgressCallback>,
        features: Option<Vec<String>>,
    ) -> Result<PathBuf> {
        self.with_legacy_features(features)?
            .generate(name, version, Some(member_path), progress_callback)
            .await
    }

    async fn generate(
        &self,
        name: &str,
        version: &str,
        member: Option<&str>,
        progress: Option<ProgressCallback>,
    ) -> Result<PathBuf> {
        let docs_path = self.storage.docs_path(name, version, member)?;
        // Separate processes can build the same variant. Hold an OS lock until publication.
        let lock_dir = self.storage.cache_dir().join("locks");
        self.storage.ensure_dir(&lock_dir)?;
        let lock_key = serde_json::to_vec(&(name, version, member, &self.storage.features))?;
        let lock = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(lock_dir.join(format!("{:x}.lock", Sha256::digest(lock_key))))?;
        tokio::time::timeout(std::time::Duration::from_secs(3600), async {
            while !lock.try_lock_exclusive()? {
                tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            }
            Ok::<_, std::io::Error>(())
        })
        .await
        .context("Timed out waiting for documentation build lock")??;
        if docs_path.exists() {
            return Ok(docs_path);
        }
        let source_path = self.storage.source_path(name, version)?;
        let manifest = match member {
            Some(member) => source_path.join(member).join(CARGO_TOML),
            None => source_path.join(CARGO_TOML),
        };
        let package = WorkspaceHandler::get_package_name(&manifest)?;
        let destination = self.storage.artifact_path(name, version, member)?;
        let parent = destination.parent().context("Invalid artifact path")?;
        self.storage.ensure_dir(parent)?;
        // Work in an isolated directory. Failed builds never become cache hits.
        let staging = tempfile::tempdir_in(parent)?;
        let target = tempfile::tempdir_in(self.storage.cache_dir())?;
        if let Some(callback) = &progress {
            callback(10);
        }
        let effective = rustdoc::run_cargo_rustdoc_json_with_options(
            manifest
                .parent()
                .context("Manifest has no parent directory")?,
            None,
            Some(target.path()),
            &self.storage.features,
        )
        .await?;
        let json_file = self.find_json_doc(&target.path().join(DOC_DIR), &package)?;
        let staged_storage = self.storage.staging_at(staging.path().to_path_buf());
        std::fs::copy(json_file, staged_storage.docs_path(name, version, member)?)?;
        let staged = DocGenerator::new(staged_storage);
        staged
            .generate_dependencies(name, version, member, &effective)
            .await?;
        staged
            .create_search_index(name, version, member, progress)
            .await?;
        std::fs::write(
            staging.path().join("build.json"),
            serde_json::to_vec_pretty(&serde_json::json!({
                "requested": self.storage.features,
                "effective": effective,
                "toolchain": rustdoc::resolve_toolchain()?,
                "format_version": rustdoc_types::FORMAT_VERSION,
            }))?,
        )?;
        if destination.exists() {
            std::fs::remove_dir_all(&destination)?;
        }
        std::fs::rename(staging.path(), &destination)
            .context("Failed to publish documentation variant")?;
        self.storage.save_metadata(name, version)?;
        Ok(docs_path)
    }

    /// Find the JSON documentation file for a crate in the target/doc directory
    fn find_json_doc(&self, doc_dir: &Path, crate_name: &str) -> Result<PathBuf> {
        // The JSON file is typically named after the crate, with hyphens replaced by underscores
        let json_name = crate_name.replace('-', "_");
        let json_file = doc_dir.join(format!("{json_name}.json"));

        if json_file.exists() {
            return Ok(json_file);
        }

        // If not found, try to find any .json file in the directory
        let entries = std::fs::read_dir(doc_dir)
            .with_context(|| format!("Failed to read doc directory: {}", doc_dir.display()))?;

        for entry in entries {
            let entry = entry?;
            let path = entry.path();
            if path.extension().and_then(|s| s.to_str()) == Some("json") {
                return Ok(path);
            }
        }

        bail!(
            "No JSON documentation file found for crate '{}' in {}",
            crate_name,
            doc_dir.display()
        );
    }

    async fn generate_dependencies(
        &self,
        name: &str,
        version: &str,
        member: Option<&str>,
        effective: &crate::cache::features::FeatureOptions,
    ) -> Result<()> {
        let source = self.storage.source_path(name, version)?;
        let manifest = member
            .map(|m| source.join(m))
            .unwrap_or_else(|| source.clone())
            .join(CARGO_TOML);
        let output = tokio::process::Command::new("cargo")
            .arg(format!("+{}", rustdoc::resolve_toolchain()?))
            .args(["metadata", "--format-version", "1", "--manifest-path"])
            .arg(manifest)
            .args(effective.args())
            .current_dir(source)
            .kill_on_drop(true)
            .output()
            .await
            .context("Failed to run cargo metadata")?;
        if !output.status.success() {
            bail!(
                "Failed to generate dependency metadata: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }
        tokio::fs::write(
            self.storage.dependencies_path(name, version, member)?,
            output.stdout,
        )
        .await?;
        Ok(())
    }

    /// Load dependency information from cache
    pub async fn load_dependencies(&self, name: &str, version: &str) -> Result<serde_json::Value> {
        let deps_path = self.storage.dependencies_path(name, version, None)?;

        if !deps_path.exists() {
            bail!("Dependencies not found for {name}-{version}");
        }

        let json_string = tokio::fs::read_to_string(&deps_path)
            .await
            .context("Failed to read dependencies file")?;

        let deps: serde_json::Value =
            serde_json::from_str(&json_string).context("Failed to parse dependencies JSON")?;

        Ok(deps)
    }

    /// Load documentation from cache for a crate or workspace member.
    ///
    /// Parses straight into [`rustdoc_types::Crate`] via a memory-mapped
    /// file + `serde_json::from_slice`, on the blocking pool. The
    /// previous implementation went `String → serde_json::Value →
    /// rustdoc_types::Crate`, which held multiple copies of ~1GB in memory
    /// for large crates and was the runtime-query half of issue #43.
    pub async fn load_docs(
        &self,
        name: &str,
        version: &str,
        member_name: Option<&str>,
    ) -> Result<rustdoc_types::Crate> {
        let docs_path = self.storage.docs_path(name, version, member_name)?;

        if !docs_path.exists() {
            if let Some(member) = member_name {
                bail!("Documentation not found for workspace member {member} in {name}-{version}");
            } else {
                bail!("Documentation not found for {name}-{version}");
            }
        }

        tokio::task::spawn_blocking(move || read_crate_from_json(&docs_path))
            .await
            .context("documentation loading task panicked")?
    }

    /// Create search index for a crate or workspace member
    pub async fn create_search_index(
        &self,
        name: &str,
        version: &str,
        member_name: Option<&str>,
        progress_callback: Option<ProgressCallback>,
    ) -> Result<()> {
        let target_label = match member_name {
            Some(member) => format!("workspace member {member} in {name}-{version}"),
            None => format!("{name}-{version}"),
        };

        tracing::info!("Creating search index for {target_label}");

        let docs_path = self.storage.docs_path(name, version, member_name)?;

        // Report file size up front so slow indexing runs can be correlated
        // with input size in the logs.
        if let Ok(meta) = std::fs::metadata(&docs_path) {
            let mb = (meta.len() as f64) / (1024.0 * 1024.0);
            tracing::info!("Indexing {target_label}: docs.json size = {mb:.1} MB");
        }

        // Both the JSON parse and the tantivy indexing are fully synchronous
        // and can block for minutes on large crates. Run the entire pipeline
        // inside a single `spawn_blocking` so the async runtime stays free
        // (and so we never hop runtimes mid-operation, which would require
        // extra synchronization across the parsed `Crate` struct).
        let storage = self.storage.clone();
        let name_owned = name.to_string();
        let version_owned = version.to_string();
        let member_owned = member_name.map(|m| m.to_string());
        let docs_path_owned = docs_path.clone();
        let label_for_task = target_label.clone();

        tokio::task::spawn_blocking(move || -> Result<()> {
            let parse_start = std::time::Instant::now();
            let crate_data = read_crate_for_indexing(&docs_path_owned)?;
            let item_count = crate_data.index.len();
            tracing::info!(
                "Parsed (trimmed) {label_for_task} in {:.2}s ({item_count} items)",
                parse_start.elapsed().as_secs_f64()
            );

            let index_start = std::time::Instant::now();
            let mut indexer = SearchIndexer::new_for_crate(
                &name_owned,
                &version_owned,
                &storage,
                member_owned.as_deref(),
            )?;
            indexer.add_index_crate_items(
                &name_owned,
                &version_owned,
                &crate_data,
                progress_callback,
            )?;
            tracing::info!(
                "Indexed {label_for_task} in {:.2}s ({item_count} items)",
                index_start.elapsed().as_secs_f64()
            );
            Ok(())
        })
        .await
        .context("search indexing task panicked")??;

        tracing::info!("Successfully created search index for {target_label}");
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn test_docgen_creation() {
        let temp_dir = TempDir::new().unwrap();
        let storage = CacheStorage::new(Some(temp_dir.path().to_path_buf())).unwrap();
        let docgen = DocGenerator::new(storage);

        // Just verify it was created successfully
        assert!(format!("{docgen:?}").contains("DocGenerator"));
    }

    #[test]
    fn test_find_json_doc_not_found() {
        let temp_dir = TempDir::new().unwrap();
        let storage = CacheStorage::new(Some(temp_dir.path().to_path_buf())).unwrap();
        let docgen = DocGenerator::new(storage);

        let doc_dir = temp_dir.path().join(DOC_DIR);
        fs::create_dir_all(&doc_dir).unwrap();

        let result = docgen.find_json_doc(&doc_dir, "nonexistent");
        assert!(result.is_err());
    }

    #[test]
    fn test_find_json_doc_found() {
        let temp_dir = TempDir::new().unwrap();
        let storage = CacheStorage::new(Some(temp_dir.path().to_path_buf())).unwrap();
        let docgen = DocGenerator::new(storage);

        let doc_dir = temp_dir.path().join(DOC_DIR);
        fs::create_dir_all(&doc_dir).unwrap();

        // Create a JSON file
        let json_file = doc_dir.join("test_crate.json");
        fs::write(&json_file, "{}").unwrap();

        let result = docgen.find_json_doc(&doc_dir, "test_crate").unwrap();
        assert_eq!(result, json_file);
    }

    #[test]
    fn test_find_json_doc_with_underscore_conversion() {
        let temp_dir = TempDir::new().unwrap();
        let storage = CacheStorage::new(Some(temp_dir.path().to_path_buf())).unwrap();
        let docgen = DocGenerator::new(storage);

        let doc_dir = temp_dir.path().join(DOC_DIR);
        fs::create_dir_all(&doc_dir).unwrap();

        // Create a JSON file with underscores (converted from hyphens)
        let json_file = doc_dir.join("test_crate.json");
        fs::write(&json_file, "{}").unwrap();

        let result = docgen.find_json_doc(&doc_dir, "test-crate").unwrap();
        assert_eq!(result, json_file);
    }
}
