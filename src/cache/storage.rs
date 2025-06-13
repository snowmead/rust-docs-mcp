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
    pub size_bytes: u64,
}

impl CacheStorage {
    /// Create a new cache storage instance
    pub fn new(custom_cache_dir: Option<PathBuf>) -> Result<Self> {
        let cache_dir = match custom_cache_dir {
            Some(dir) => dir,
            None => dirs::home_dir()
                .context("Failed to get home directory")?
                .join(".rust-docs-mcp")
                .join("cache"),
        };

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

    /// Get the dependencies path for a crate
    pub fn dependencies_path(&self, name: &str, version: &str) -> PathBuf {
        self.crate_path(name, version).join("dependencies.json")
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

    /// Calculate the total size of a directory in bytes
    pub fn calculate_dir_size(&self, path: &Path) -> Result<u64> {
        let mut total_size = 0u64;

        if !path.exists() {
            return Ok(0);
        }

        for entry in fs::read_dir(path)? {
            let entry = entry?;
            let metadata = entry.metadata()?;

            if metadata.is_dir() {
                total_size += self.calculate_dir_size(&entry.path())?;
            } else {
                total_size += metadata.len();
            }
        }

        Ok(total_size)
    }

    /// Save metadata for a crate
    pub fn save_metadata(&self, name: &str, version: &str) -> Result<()> {
        let crate_path = self.crate_path(name, version);
        let size_bytes = self.calculate_dir_size(&crate_path)?;

        let metadata = CrateMetadata {
            name: name.to_string(),
            version: version.to_string(),
            cached_at: chrono::Utc::now(),
            doc_generated: self.has_docs(name, version),
            size_bytes,
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
                                    version: version.clone(),
                                    cached_at,
                                    doc_generated: self.has_docs(
                                        &crate_name,
                                        &version_entry.file_name().to_string_lossy(),
                                    ),
                                    size_bytes: 0,
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
