//! Transaction-like operations for crate caching with automatic rollback
//!
//! This module provides utilities for safely updating cached crates with
//! automatic backup and restore capabilities.

use anyhow::{Context, Result};
use std::path::PathBuf;

use crate::cache::storage::CacheStorage;

/// Represents a cache update transaction with automatic rollback on failure
pub struct CacheTransaction<'a> {
    storage: &'a CacheStorage,
    crate_name: String,
    version: String,
    backup_path: Option<PathBuf>,
}

impl<'a> CacheTransaction<'a> {
    /// Create a new cache transaction
    pub fn new(storage: &'a CacheStorage, crate_name: &str, version: &str) -> Self {
        Self {
            storage,
            crate_name: crate_name.to_string(),
            version: version.to_string(),
            backup_path: None,
        }
    }

    /// Begin the transaction by creating a backup if the crate exists
    pub fn begin(&mut self) -> Result<()> {
        if self.storage.is_cached(&self.crate_name, &self.version) {
            let backup_path = self
                .storage
                .backup_crate_to_temp(&self.crate_name, &self.version)
                .context("Failed to create backup")?;
            self.backup_path = Some(backup_path);

            // Remove the existing cache
            self.storage
                .remove_crate(&self.crate_name, &self.version)
                .context("Failed to remove existing cache")?;
        }
        Ok(())
    }

    /// Commit the transaction by cleaning up the backup
    pub fn commit(mut self) -> Result<()> {
        if let Some(backup_path) = self.backup_path.take() {
            self.storage
                .cleanup_backup(&backup_path)
                .context("Failed to cleanup backup")?;
        }
        Ok(())
    }

    /// Rollback the transaction by restoring from backup
    pub fn rollback(&mut self) -> Result<()> {
        if let Some(backup_path) = self.backup_path.take() {
            self.storage
                .restore_crate_from_backup(&self.crate_name, &self.version, &backup_path)
                .context("Failed to restore from backup")?;

            self.storage
                .cleanup_backup(&backup_path)
                .context("Failed to cleanup backup after restore")?;
        }
        Ok(())
    }
}

impl<'a> Drop for CacheTransaction<'a> {
    fn drop(&mut self) {
        // If transaction wasn't committed and there's a backup, try to rollback
        if self.backup_path.is_some() {
            let _ = self.rollback();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn test_transaction_commit() -> Result<()> {
        let temp_dir = TempDir::new()?;
        let storage = CacheStorage::new(Some(temp_dir.path().to_path_buf()))?;

        // Create a fake cached crate with proper structure
        let source_path = storage.source_path("test-crate", "1.0.0");
        storage.ensure_dir(&source_path)?;
        fs::write(source_path.join("file.txt"), "original content")?;
        storage.save_metadata("test-crate", "1.0.0")?;

        // Start transaction
        let mut transaction = CacheTransaction::new(&storage, "test-crate", "1.0.0");
        transaction.begin()?;

        // Verify crate was removed
        assert!(!storage.is_cached("test-crate", "1.0.0"));

        // Add new content
        let new_source_path = storage.source_path("test-crate", "1.0.0");
        storage.ensure_dir(&new_source_path)?;
        fs::write(new_source_path.join("file.txt"), "new content")?;
        storage.save_metadata("test-crate", "1.0.0")?;

        // Commit transaction
        transaction.commit()?;

        // Verify new content exists
        assert!(storage.is_cached("test-crate", "1.0.0"));
        let content = fs::read_to_string(new_source_path.join("file.txt"))?;
        assert_eq!(content, "new content");

        Ok(())
    }

    #[test]
    fn test_transaction_rollback() -> Result<()> {
        let temp_dir = TempDir::new()?;
        let storage = CacheStorage::new(Some(temp_dir.path().to_path_buf()))?;

        // Create a fake cached crate with proper structure
        let source_path = storage.source_path("test-crate", "1.0.0");
        storage.ensure_dir(&source_path)?;
        fs::write(source_path.join("file.txt"), "original content")?;

        // Save metadata to make it a valid cached crate
        storage.save_metadata("test-crate", "1.0.0")?;

        // Start transaction
        let mut transaction = CacheTransaction::new(&storage, "test-crate", "1.0.0");
        transaction.begin()?;

        // Verify crate was removed
        assert!(!storage.is_cached("test-crate", "1.0.0"));

        // Simulate failure - rollback
        transaction.rollback()?;

        // Verify original content was restored
        assert!(storage.is_cached("test-crate", "1.0.0"));
        let restored_source_path = storage.source_path("test-crate", "1.0.0");
        let content = fs::read_to_string(restored_source_path.join("file.txt"))?;
        assert_eq!(content, "original content");

        Ok(())
    }
}
