//! Utilities for member path conversion
//!
//! This module provides functions to safely convert member paths containing
//! slashes to dash-separated formats, preventing path traversal attacks
//! while maintaining compatibility with workspace member paths.

use anyhow::{bail, Result};

/// Convert a member path with slashes to a safe dash-separated format
/// 
/// # Examples
/// ```
/// assert_eq!(normalize_member_path("crates/rmcp"), "crates-rmcp");
/// assert_eq!(normalize_member_path("simple"), "simple");
/// ```
pub fn normalize_member_path(member_path: &str) -> String {
    member_path.replace('/', "-")
}

/// Check if a path needs normalization
pub fn needs_normalization(member_path: &str) -> bool {
    member_path.contains('/')
}

/// Validate that a member path doesn't contain dangerous sequences
/// 
/// This function ensures the member path is safe to use in file operations
/// by rejecting absolute paths, path traversal attempts, and backslashes.
pub fn validate_member_path(member_path: &str) -> Result<()> {
    // Reject empty paths
    if member_path.is_empty() {
        bail!("Invalid member path: empty path not allowed");
    }
    
    // Reject absolute paths
    if member_path.starts_with('/') || member_path.starts_with('\\') {
        bail!("Invalid member path '{}': absolute paths not allowed", member_path);
    }
    
    // Check for Windows absolute paths (e.g., C:\)
    if member_path.len() > 2 && member_path.chars().nth(1) == Some(':') {
        bail!("Invalid member path '{}': absolute paths not allowed", member_path);
    }
    
    // Reject path traversal
    if member_path.contains("..") {
        bail!("Invalid member path '{}': path traversal not allowed", member_path);
    }
    
    // Reject backslashes (Windows paths)
    if member_path.contains('\\') {
        bail!("Invalid member path '{}': backslashes not allowed", member_path);
    }
    
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_normalize_member_path() {
        assert_eq!(normalize_member_path("crates/rmcp"), "crates-rmcp");
        assert_eq!(normalize_member_path("crates/rmcp/submodule"), "crates-rmcp-submodule");
        assert_eq!(normalize_member_path("simple"), "simple");
        assert_eq!(normalize_member_path("already-dashed"), "already-dashed");
    }

    #[test]
    fn test_needs_normalization() {
        assert!(needs_normalization("crates/rmcp"));
        assert!(needs_normalization("path/to/member"));
        assert!(!needs_normalization("simple"));
        assert!(!needs_normalization("already-dashed"));
    }

    #[test]
    fn test_validate_member_path() {
        // Valid paths
        assert!(validate_member_path("crates/rmcp").is_ok());
        assert!(validate_member_path("simple").is_ok());
        assert!(validate_member_path("path/to/member").is_ok());
        
        // Invalid paths
        assert!(validate_member_path("").is_err());
        assert!(validate_member_path("/absolute/path").is_err());
        assert!(validate_member_path("\\windows\\path").is_err());
        assert!(validate_member_path("C:\\Windows").is_err());
        assert!(validate_member_path("../parent").is_err());
        assert!(validate_member_path("path/../traversal").is_err());
        assert!(validate_member_path("path\\with\\backslash").is_err());
    }
}