use std::path::{Path, PathBuf};
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::fs;

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
            .join(".mcp-rust-docs")
            .join("cache");
        
        fs::create_dir_all(&cache_dir)
            .context("Failed to create cache directory")?;
        
        Ok(Self { cache_dir })
    }
    
    /// Get the path for a specific crate version
    pub fn crate_path(&self, name: &str, version: &str) -> PathBuf {
        self.cache_dir
            .join("crates")
            .join(name)
            .join(version)
    }
    
    /// Get the source directory path for a crate
    pub fn source_path(&self, name: &str, version: &str) -> PathBuf {
        self.crate_path(name, version).join("source")
    }
    
    /// Get the documentation JSON path for a crate
    pub fn docs_path(&self, name: &str, version: &str) -> PathBuf {
        self.crate_path(name, version).join("docs.json")
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
                        let metadata = CrateMetadata {
                            name: crate_name.clone(),
                            version,
                            cached_at: chrono::Utc::now(), // TODO: Store actual cache time
                            doc_generated: self.has_docs(&crate_name, &version_entry.file_name().to_string_lossy()),
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