use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

/// Manages the file system storage for cached crates and their documentation
#[derive(Debug, Clone)]
pub struct CacheStorage {
    cache_dir: PathBuf,
}

/// Metadata about a cached crate
#[derive(Debug, Serialize, Deserialize)]
pub struct CrateMetadata {
    pub name: String,
    pub version: String,
    pub cached_at: chrono::DateTime<chrono::Utc>,
    pub doc_generated: bool,
}

impl CacheStorage {
    /// Create a new cache storage instance
    pub fn new() -> Result<Self> {
        let cache_dir = dirs::home_dir()
            .context("Failed to get home directory")?
            .join(".rust-docs-mcp")
            .join("cache");

        fs::create_dir_all(&cache_dir).context("Failed to create cache directory")?;

        Ok(Self { cache_dir })
    }

    /// Get the path for a specific crate version
    pub fn crate_path(&self, name: &str, version: &str) -> PathBuf {
        self.cache_dir.join("crates").join(name).join(version)
    }

    /// Get the source directory path for a crate
    pub fn source_path(&self, name: &str, version: &str) -> PathBuf {
        self.crate_path(name, version).join("source")
    }

    /// Get the documentation JSON path for a crate
    pub fn docs_path(&self, name: &str, version: &str) -> PathBuf {
        self.crate_path(name, version).join("docs.json")
    }

    /// Get the metadata path for a crate
    pub fn metadata_path(&self, name: &str, version: &str) -> PathBuf {
        self.crate_path(name, version).join("metadata.json")
    }

    /// Check if a crate version is cached
    pub fn is_cached(&self, name: &str, version: &str) -> bool {
        self.crate_path(name, version).exists()
    }

    /// Check if documentation is generated for a crate
    pub fn has_docs(&self, name: &str, version: &str) -> bool {
        self.docs_path(name, version).exists()
    }

    /// Ensure a directory exists
    pub fn ensure_dir(&self, path: &Path) -> Result<()> {
        fs::create_dir_all(path)
            .with_context(|| format!("Failed to create directory: {}", path.display()))?;
        Ok(())
    }

    /// Save metadata for a crate
    pub fn save_metadata(&self, name: &str, version: &str) -> Result<()> {
        let metadata = CrateMetadata {
            name: name.to_string(),
            version: version.to_string(),
            cached_at: chrono::Utc::now(),
            doc_generated: self.has_docs(name, version),
        };
        
        let metadata_path = self.metadata_path(name, version);
        let json = serde_json::to_string_pretty(&metadata)?;
        fs::write(metadata_path, json)?;
        Ok(())
    }

    /// Load metadata for a crate
    pub fn load_metadata(&self, name: &str, version: &str) -> Result<CrateMetadata> {
        let metadata_path = self.metadata_path(name, version);
        let json = fs::read_to_string(metadata_path)?;
        let metadata: CrateMetadata = serde_json::from_str(&json)?;
        Ok(metadata)
    }

    /// Get all cached crate versions
    pub fn list_cached_crates(&self) -> Result<Vec<CrateMetadata>> {
        let crates_dir = self.cache_dir.join("crates");
        let mut cached_crates = Vec::new();

        if !crates_dir.exists() {
            return Ok(cached_crates);
        }

        for crate_entry in fs::read_dir(&crates_dir)? {
            let crate_entry = crate_entry?;
            let crate_name = crate_entry.file_name().to_string_lossy().to_string();

            if crate_entry.file_type()?.is_dir() {
                for version_entry in fs::read_dir(crate_entry.path())? {
                    let version_entry = version_entry?;
                    let version = version_entry.file_name().to_string_lossy().to_string();

                    if version_entry.file_type()?.is_dir() {
                        // Try to load metadata, fall back to creating new metadata if not found
                        let metadata = match self.load_metadata(&crate_name, &version) {
                            Ok(meta) => meta,
                            Err(_) => {
                                // If metadata doesn't exist, create it based on file modification time
                                let cached_at = version_entry
                                    .metadata()
                                    .and_then(|m| m.modified())
                                    .map(|t| chrono::DateTime::<chrono::Utc>::from(t))
                                    .unwrap_or_else(|_| chrono::Utc::now());
                                
                                CrateMetadata {
                                    name: crate_name.clone(),
                                    version,
                                    cached_at,
                                    doc_generated: self.has_docs(
                                        &crate_name,
                                        &version_entry.file_name().to_string_lossy(),
                                    ),
                                }
                            }
                        };
                        cached_crates.push(metadata);
                    }
                }
            }
        }

        Ok(cached_crates)
    }

    /// Remove a cached crate version
    pub fn remove_crate(&self, name: &str, version: &str) -> Result<()> {
        let path = self.crate_path(name, version);
        if path.exists() {
            fs::remove_dir_all(&path)
                .with_context(|| format!("Failed to remove crate cache: {}/{}", name, version))?;
        }
        Ok(())
    }
}
