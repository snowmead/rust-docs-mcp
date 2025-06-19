//! Utility functions for the cache module
//!
//! This module contains shared utilities used across the cache implementation,
//! including file operations, error handling, and response formatting.

use anyhow::{Context, Result};
use std::fs;
use std::path::Path;

/// Recursively copy directory contents from source to destination
///
/// This function copies all files and subdirectories from the source path to the destination,
/// excluding version control directories like .git, .svn, and .hg.
pub fn copy_directory_contents(src: &Path, dest: &Path) -> Result<()> {
    if !dest.exists() {
        fs::create_dir_all(dest)
            .with_context(|| format!("Failed to create directory: {}", dest.display()))?;
    }

    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let path = entry.path();
        let name = entry.file_name();
        let dest_path = dest.join(&name);

        if path.is_dir() {
            // Skip version control directories
            if name == ".git" || name == ".svn" || name == ".hg" {
                continue;
            }
            copy_directory_contents(&path, &dest_path)?;
        } else {
            fs::copy(&path, &dest_path).with_context(|| {
                format!(
                    "Failed to copy file from {} to {}",
                    path.display(),
                    dest_path.display()
                )
            })?;
        }
    }

    Ok(())
}

/// Format bytes into human-readable string
pub fn format_bytes(bytes: u64) -> String {
    const UNITS: &[&str] = &["B", "KB", "MB", "GB", "TB"];
    if bytes == 0 {
        return "0 B".to_string();
    }

    let base = 1024_f64;
    let exponent = (bytes as f64).ln() / base.ln();
    let exponent = exponent.floor() as usize;

    let unit = UNITS.get(exponent).unwrap_or(&"TB");
    let size = bytes as f64 / base.powi(exponent as i32);

    if size.fract() == 0.0 {
        format!("{size:.0} {unit}")
    } else {
        format!("{size:.2} {unit}")
    }
}

/// Response types for cache operations
#[derive(Debug, serde::Serialize)]
#[serde(untagged)]
pub enum CacheResponse {
    Success {
        status: &'static str,
        message: String,
        #[serde(rename = "crate")]
        crate_name: String,
        version: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        members: Option<Vec<String>>,
        #[serde(skip_serializing_if = "Option::is_none")]
        results: Option<Vec<String>>,
        #[serde(skip_serializing_if = "Option::is_none")]
        updated: Option<bool>,
    },
    PartialSuccess {
        status: &'static str,
        message: String,
        #[serde(rename = "crate")]
        crate_name: String,
        version: String,
        members: Vec<String>,
        results: Vec<String>,
        errors: Vec<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        updated: Option<bool>,
    },
    WorkspaceDetected {
        status: &'static str,
        message: &'static str,
        #[serde(rename = "crate")]
        crate_name: String,
        version: String,
        workspace_members: Vec<String>,
        example_usage: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        updated: Option<bool>,
    },
    Error {
        error: String,
    },
}

impl CacheResponse {
    /// Create a success response
    pub fn success(crate_name: impl Into<String>, version: impl Into<String>) -> Self {
        let crate_name = crate_name.into();
        let version = version.into();
        Self::Success {
            status: "success",
            message: format!("Successfully cached {crate_name}-{version}"),
            crate_name,
            version,
            members: None,
            results: None,
            updated: None,
        }
    }

    /// Create a success response with update flag
    pub fn success_updated(crate_name: impl Into<String>, version: impl Into<String>) -> Self {
        let crate_name = crate_name.into();
        let version = version.into();
        Self::Success {
            status: "success",
            message: format!("Successfully updated {crate_name}-{version}"),
            crate_name,
            version,
            members: None,
            results: None,
            updated: Some(true),
        }
    }

    /// Create a workspace members success response
    pub fn members_success(
        crate_name: impl Into<String>,
        version: impl Into<String>,
        members: Vec<String>,
        results: Vec<String>,
        updated: bool,
    ) -> Self {
        let count = results.len();
        let message = if updated {
            format!("Successfully updated {count} workspace members")
        } else {
            format!("Successfully cached {count} workspace members")
        };

        Self::Success {
            status: "success",
            message,
            crate_name: crate_name.into(),
            version: version.into(),
            members: Some(members),
            results: Some(results),
            updated: if updated { Some(true) } else { None },
        }
    }

    /// Create a partial success response for workspace members
    pub fn members_partial(
        crate_name: impl Into<String>,
        version: impl Into<String>,
        members: Vec<String>,
        results: Vec<String>,
        errors: Vec<String>,
        updated: bool,
    ) -> Self {
        let message = if updated {
            format!(
                "Updated {} members with {} errors",
                results.len(),
                errors.len()
            )
        } else {
            format!(
                "Cached {} members with {} errors",
                results.len(),
                errors.len()
            )
        };

        Self::PartialSuccess {
            status: "partial_success",
            message,
            crate_name: crate_name.into(),
            version: version.into(),
            members,
            results,
            errors,
            updated: if updated { Some(true) } else { None },
        }
    }

    /// Create a workspace detected response
    pub fn workspace_detected(
        crate_name: impl Into<String>,
        version: impl Into<String>,
        members: Vec<String>,
        source_type: &str,
        updated: bool,
    ) -> Self {
        let crate_name = crate_name.into();
        let version = version.into();
        let example_members = members.get(0..2.min(members.len())).unwrap_or(&[]).to_vec();

        Self::WorkspaceDetected {
            status: "workspace_detected",
            message: "This is a workspace crate. Please specify which members to cache using the 'members' parameter.",
            crate_name: crate_name.clone(),
            version: version.clone(),
            workspace_members: members,
            example_usage: format!(
                "cache_crate_from_{source_type}(crate_name=\"{crate_name}\", version=\"{version}\", members={example_members:?})"
            ),
            updated: if updated { Some(true) } else { None },
        }
    }

    /// Create an error response
    pub fn error(message: impl Into<String>) -> Self {
        Self::Error {
            error: message.into(),
        }
    }

    /// Convert to JSON string
    pub fn to_json(&self) -> String {
        serde_json::to_string(self)
            .unwrap_or_else(|_| r#"{"error":"Failed to serialize response"}"#.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_format_bytes() {
        assert_eq!(format_bytes(0), "0 B");
        assert_eq!(format_bytes(512), "512 B");
        assert_eq!(format_bytes(1024), "1 KB");
        assert_eq!(format_bytes(1536), "1.50 KB");
        assert_eq!(format_bytes(1048576), "1 MB");
        assert_eq!(format_bytes(1073741824), "1 GB");
    }

    #[test]
    fn test_copy_directory_contents() -> Result<()> {
        let temp_dir = TempDir::new()?;
        let src_dir = temp_dir.path().join("src");
        let dest_dir = temp_dir.path().join("dest");

        // Create source structure
        fs::create_dir_all(&src_dir)?;
        fs::write(src_dir.join("file1.txt"), "content1")?;

        let sub_dir = src_dir.join("subdir");
        fs::create_dir_all(&sub_dir)?;
        fs::write(sub_dir.join("file2.txt"), "content2")?;

        // Create .git directory that should be skipped
        let git_dir = src_dir.join(".git");
        fs::create_dir_all(&git_dir)?;
        fs::write(git_dir.join("config"), "git config")?;

        // Copy contents
        copy_directory_contents(&src_dir, &dest_dir)?;

        // Verify
        assert!(dest_dir.join("file1.txt").exists());
        assert!(dest_dir.join("subdir").exists());
        assert!(dest_dir.join("subdir/file2.txt").exists());
        assert!(!dest_dir.join(".git").exists());

        Ok(())
    }

    #[test]
    fn test_cache_response() {
        // Test success response
        let response = CacheResponse::success("test-crate", "1.0.0");
        let json_str = response.to_json();
        let json: serde_json::Value = serde_json::from_str(&json_str).unwrap();
        assert_eq!(json["status"], "success");
        assert_eq!(json["message"], "Successfully cached test-crate-1.0.0");
        assert_eq!(json["crate"], "test-crate");
        assert_eq!(json["version"], "1.0.0");

        // Test error response
        let error = CacheResponse::error("Something went wrong");
        let json_str = error.to_json();
        let json: serde_json::Value = serde_json::from_str(&json_str).unwrap();
        assert_eq!(json["error"], "Something went wrong");
        assert!(json.get("status").is_none());

        // Test workspace detected
        let workspace = CacheResponse::workspace_detected(
            "test-crate",
            "1.0.0",
            vec!["crate-a".to_string(), "crate-b".to_string()],
            "cratesio",
            false,
        );
        let json_str = workspace.to_json();
        let json: serde_json::Value = serde_json::from_str(&json_str).unwrap();
        assert_eq!(json["status"], "workspace_detected");
        assert_eq!(json["crate"], "test-crate");
        assert_eq!(
            json["workspace_members"],
            serde_json::json!(["crate-a", "crate-b"])
        );

        // Test members success
        let members = CacheResponse::members_success(
            "test-crate",
            "1.0.0",
            vec!["member1".to_string()],
            vec!["Successfully cached member: member1".to_string()],
            false,
        );
        let json_str = members.to_json();
        let json: serde_json::Value = serde_json::from_str(&json_str).unwrap();
        assert_eq!(json["status"], "success");
        assert!(json.get("updated").is_none());

        // Test members success with update
        let members_updated = CacheResponse::members_success(
            "test-crate",
            "1.0.0",
            vec!["member1".to_string()],
            vec!["Successfully cached member: member1".to_string()],
            true,
        );
        let json_str = members_updated.to_json();
        let json: serde_json::Value = serde_json::from_str(&json_str).unwrap();
        assert!(json["updated"].as_bool().unwrap());
    }
}
