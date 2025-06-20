use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

use crate::cache::types::CrateIdentifier;
use crate::cache::utils::copy_directory_contents;

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
    #[serde(default = "default_source")]
    pub source: String,
    #[serde(default)]
    pub source_path: Option<String>,
}

/// Default source for backward compatibility
fn default_source() -> String {
    "crates.io".to_string()
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
        self.crate_path_for_id(&CrateIdentifier::new(name, version).unwrap())
    }

    /// Get the path for a specific crate using CrateIdentifier
    pub fn crate_path_for_id(&self, crate_id: &CrateIdentifier) -> PathBuf {
        self.cache_dir
            .join("crates")
            .join(crate_id.name())
            .join(crate_id.version())
    }

    /// Get the path for a specific workspace member
    pub fn member_path(&self, name: &str, version: &str, member_name: &str) -> PathBuf {
        self.crate_path(name, version)
            .join("members")
            .join(member_name)
    }

    /// Get the source directory path for a crate
    pub fn source_path(&self, name: &str, version: &str) -> PathBuf {
        self.crate_path(name, version).join("source")
    }

    /// Get the documentation JSON path for a crate
    pub fn docs_path(&self, name: &str, version: &str) -> PathBuf {
        self.crate_path(name, version).join("docs.json")
    }

    /// Get the documentation JSON path for a workspace member
    pub fn member_docs_path(&self, name: &str, version: &str, member_name: &str) -> PathBuf {
        self.member_path(name, version, member_name)
            .join("docs.json")
    }

    /// Get the metadata path for a crate
    pub fn metadata_path(&self, name: &str, version: &str) -> PathBuf {
        self.crate_path(name, version).join("metadata.json")
    }

    /// Get the metadata path for a workspace member
    pub fn member_metadata_path(&self, name: &str, version: &str, member_name: &str) -> PathBuf {
        self.member_path(name, version, member_name)
            .join("metadata.json")
    }

    /// Get the dependencies path for a crate
    pub fn dependencies_path(&self, name: &str, version: &str) -> PathBuf {
        self.crate_path(name, version).join("dependencies.json")
    }

    /// Get the dependencies path for a workspace member
    pub fn member_dependencies_path(
        &self,
        name: &str,
        version: &str,
        member_name: &str,
    ) -> PathBuf {
        self.member_path(name, version, member_name)
            .join("dependencies.json")
    }

    /// Check if a crate version is cached
    pub fn is_cached(&self, name: &str, version: &str) -> bool {
        self.crate_path(name, version).exists()
    }

    /// Check if documentation is generated for a crate
    pub fn has_docs(&self, name: &str, version: &str) -> bool {
        self.docs_path(name, version).exists()
    }

    /// Check if a workspace member is cached
    pub fn is_member_cached(&self, name: &str, version: &str, member_name: &str) -> bool {
        self.member_path(name, version, member_name).exists()
    }

    /// Check if documentation is generated for a workspace member
    pub fn has_member_docs(&self, name: &str, version: &str, member_name: &str) -> bool {
        self.member_docs_path(name, version, member_name).exists()
    }

    /// Ensure a directory exists
    pub fn ensure_dir(&self, path: &Path) -> Result<()> {
        fs::create_dir_all(path)
            .with_context(|| format!("Failed to create directory: {}", path.display()))?;
        Ok(())
    }

    /// Calculate the total size of a directory in bytes
    #[allow(clippy::only_used_in_recursion)]
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
        self.save_metadata_with_source(name, version, "crates.io", None)
    }

    /// Save metadata for a crate with source information
    pub fn save_metadata_with_source(
        &self,
        name: &str,
        version: &str,
        source: &str,
        source_path: Option<&str>,
    ) -> Result<()> {
        let crate_path = self.crate_path(name, version);
        let size_bytes = self.calculate_dir_size(&crate_path)?;

        let metadata = CrateMetadata {
            name: name.to_string(),
            version: version.to_string(),
            cached_at: chrono::Utc::now(),
            doc_generated: self.has_docs(name, version),
            size_bytes,
            source: source.to_string(),
            source_path: source_path.map(String::from),
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

    /// Load metadata for a workspace member
    pub fn load_member_metadata(
        &self,
        name: &str,
        version: &str,
        member_name: &str,
    ) -> Result<CrateMetadata> {
        let metadata_path = self.member_metadata_path(name, version, member_name);
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
                                    .map(chrono::DateTime::<chrono::Utc>::from)
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
                                    source: default_source(),
                                    source_path: None,
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

    /// Get all workspace members for a cached crate
    pub fn list_workspace_members(&self, name: &str, version: &str) -> Result<Vec<String>> {
        let members_dir = self.crate_path(name, version).join("members");
        let mut members = Vec::new();

        if !members_dir.exists() {
            return Ok(members);
        }

        for member_entry in fs::read_dir(&members_dir)? {
            let member_entry = member_entry?;
            if member_entry.file_type()?.is_dir() {
                let member_name = member_entry.file_name().to_string_lossy().to_string();
                members.push(member_name);
            }
        }

        Ok(members)
    }

    /// Remove a cached crate version
    pub fn remove_crate(&self, name: &str, version: &str) -> Result<()> {
        let path = self.crate_path(name, version);
        if path.exists() {
            fs::remove_dir_all(&path)
                .with_context(|| format!("Failed to remove crate cache: {name}/{version}"))?;
        }
        Ok(())
    }

    /// Copy a crate to a temporary backup location
    pub fn backup_crate_to_temp(&self, name: &str, version: &str) -> Result<PathBuf> {
        let source = self.crate_path(name, version);
        if !source.exists() {
            bail!("Crate {name}-{version} not found in cache");
        }

        let temp_dir = std::env::temp_dir()
            .join("rust-docs-mcp-backup")
            .join(format!(
                "{name}-{version}-{}-{}",
                chrono::Utc::now()
                    .timestamp_nanos_opt()
                    .unwrap_or_else(|| chrono::Utc::now().timestamp_micros()),
                std::process::id()
            ));

        self.ensure_dir(&temp_dir)?;
        copy_directory_contents(&source, &temp_dir)
            .with_context(|| format!("Failed to backup crate {name}-{version}"))?;

        Ok(temp_dir)
    }

    /// Restore a crate from temporary backup
    pub fn restore_crate_from_backup(
        &self,
        name: &str,
        version: &str,
        backup_path: &Path,
    ) -> Result<()> {
        if !backup_path.exists() {
            bail!("Backup path does not exist: {}", backup_path.display());
        }

        let target = self.crate_path(name, version);

        // Remove current version if it exists
        if target.exists() {
            fs::remove_dir_all(&target)
                .with_context(|| "Failed to remove existing crate before restore".to_string())?;
        }

        // Ensure parent directory exists
        if let Some(parent) = target.parent() {
            self.ensure_dir(parent)?;
        }

        // Create the target directory first
        self.ensure_dir(&target)?;

        // Restore from backup
        copy_directory_contents(backup_path, &target)
            .with_context(|| format!("Failed to restore crate {name}-{version} from backup"))?;

        Ok(())
    }

    /// Clean up temporary backup
    pub fn cleanup_backup(&self, backup_path: &Path) -> Result<()> {
        if backup_path.exists() {
            fs::remove_dir_all(backup_path).with_context(|| {
                format!("Failed to cleanup backup at {}", backup_path.display())
            })?;
        }
        Ok(())
    }
}
