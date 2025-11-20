//! Output types for cache operations
//!
//! These types are used as the return values from cache tool methods.
//! They are serialized to JSON strings for the MCP protocol, and can be
//! deserialized in tests for type-safe validation.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Output from async cache_crate operations - returns task ID for monitoring
#[derive(Debug, Serialize, Deserialize, PartialEq, Clone)]
pub struct CacheTaskStartedOutput {
    pub task_id: String,
    #[serde(rename = "crate")]
    pub crate_name: String,
    pub version: String,
    pub source_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_details: Option<String>,
    pub status: String,
    pub message: String,
}

impl CacheTaskStartedOutput {
    /// Convert to JSON string for MCP response
    pub fn to_json(&self) -> String {
        serde_json::to_string(self)
            .unwrap_or_else(|_| r#"{"error":"Failed to serialize response"}"#.to_string())
    }
}

/// Output from cache_crate operations (crates.io, GitHub, local)
#[derive(Debug, Serialize, Deserialize, PartialEq, Clone)]
#[serde(tag = "status")]
pub enum CacheCrateOutput {
    /// Successful caching operation
    #[serde(rename = "success")]
    Success {
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
    /// Partial success when caching workspace members
    #[serde(rename = "partial_success")]
    PartialSuccess {
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
    /// Workspace detected, needs member specification
    #[serde(rename = "workspace_detected")]
    WorkspaceDetected {
        message: String,
        #[serde(rename = "crate")]
        crate_name: String,
        version: String,
        workspace_members: Vec<String>,
        example_usage: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        updated: Option<bool>,
    },
    /// Error occurred during operation
    #[serde(rename = "error")]
    Error { error: String },
}

impl CacheCrateOutput {
    /// Convert to JSON string for MCP response
    pub fn to_json(&self) -> String {
        serde_json::to_string(self)
            .unwrap_or_else(|_| r#"{"error":"Failed to serialize response"}"#.to_string())
    }

    /// Check if this is a success response
    pub fn is_success(&self) -> bool {
        matches!(self, CacheCrateOutput::Success { .. })
    }

    /// Check if this is an error response
    pub fn is_error(&self) -> bool {
        matches!(self, CacheCrateOutput::Error { .. })
    }

    /// Check if this is a workspace detection response
    pub fn is_workspace_detected(&self) -> bool {
        matches!(self, CacheCrateOutput::WorkspaceDetected { .. })
    }
}

/// Output from remove_crate operation
#[derive(Debug, Serialize, Deserialize, PartialEq)]
pub struct RemoveCrateOutput {
    pub status: String,
    pub message: String,
    #[serde(rename = "crate")]
    pub crate_name: String,
    pub version: String,
}

impl RemoveCrateOutput {
    /// Convert to JSON string for MCP response
    pub fn to_json(&self) -> String {
        serde_json::to_string(self)
            .unwrap_or_else(|_| r#"{"error":"Failed to serialize response"}"#.to_string())
    }
}

/// Version information for a cached crate
#[derive(Debug, Serialize, Deserialize, PartialEq, Clone)]
pub struct VersionInfo {
    pub version: String,
    pub cached_at: String,
    pub doc_generated: bool,
    pub size_bytes: u64,
    pub size_human: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub members: Option<Vec<String>>,
}

/// Size information with human-readable format
#[derive(Debug, Serialize, Deserialize, PartialEq)]
pub struct SizeInfo {
    pub bytes: u64,
    pub human: String,
}

/// Output from list_cached_crates operation
#[derive(Debug, Serialize, Deserialize, PartialEq)]
pub struct ListCachedCratesOutput {
    pub crates: HashMap<String, Vec<VersionInfo>>,
    pub total_crates: usize,
    pub total_versions: usize,
    pub total_size: SizeInfo,
}

impl ListCachedCratesOutput {
    /// Convert to JSON string for MCP response
    pub fn to_json(&self) -> String {
        serde_json::to_string(self)
            .unwrap_or_else(|_| r#"{"error":"Failed to serialize response"}"#.to_string())
    }
}

/// Output from list_crate_versions operation
#[derive(Debug, Serialize, Deserialize, PartialEq)]
pub struct ListCrateVersionsOutput {
    #[serde(rename = "crate")]
    pub crate_name: String,
    pub versions: Vec<VersionInfo>,
    pub count: usize,
}

impl ListCrateVersionsOutput {
    /// Convert to JSON string for MCP response
    pub fn to_json(&self) -> String {
        serde_json::to_string(self)
            .unwrap_or_else(|_| r#"{"error":"Failed to serialize response"}"#.to_string())
    }
}

/// Metadata for a single crate
#[derive(Debug, Serialize, Deserialize, PartialEq, Clone)]
pub struct CrateMetadata {
    pub crate_name: String,
    pub version: String,
    pub cached: bool,
    pub analyzed: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cache_size_bytes: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cache_size_human: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub member: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub workspace_members: Option<Vec<String>>,
}

/// Output from get_crates_metadata operation
#[derive(Debug, Serialize, Deserialize, PartialEq)]
pub struct GetCratesMetadataOutput {
    pub metadata: Vec<CrateMetadata>,
    pub total_queried: usize,
    pub total_cached: usize,
}

impl GetCratesMetadataOutput {
    /// Convert to JSON string for MCP response
    pub fn to_json(&self) -> String {
        serde_json::to_string(self)
            .unwrap_or_else(|_| r#"{"error":"Failed to serialize response"}"#.to_string())
    }
}

/// Generic error output that can be used by any tool
#[derive(Debug, Serialize, Deserialize, PartialEq)]
pub struct ErrorOutput {
    pub error: String,
}

impl ErrorOutput {
    /// Create a new error output
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            error: message.into(),
        }
    }

    /// Convert to JSON string for MCP response
    pub fn to_json(&self) -> String {
        serde_json::to_string(self)
            .unwrap_or_else(|_| r#"{"error":"Failed to serialize error"}"#.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cache_crate_output_serialization() {
        let output = CacheCrateOutput::Success {
            message: "Successfully cached test-crate-1.0.0".to_string(),
            crate_name: "test-crate".to_string(),
            version: "1.0.0".to_string(),
            members: None,
            results: None,
            updated: None,
        };

        let json = output.to_json();
        let deserialized: CacheCrateOutput = serde_json::from_str(&json).unwrap();
        assert_eq!(output, deserialized);
        assert!(deserialized.is_success());
    }

    #[test]
    fn test_workspace_detected_output() {
        let output = CacheCrateOutput::WorkspaceDetected {
            message: "This is a workspace crate".to_string(),
            crate_name: "workspace".to_string(),
            version: "1.0.0".to_string(),
            workspace_members: vec!["member1".to_string(), "member2".to_string()],
            example_usage: "example".to_string(),
            updated: None,
        };

        assert!(output.is_workspace_detected());
        assert!(!output.is_success());
        assert!(!output.is_error());
    }

    #[test]
    fn test_error_output() {
        let output = ErrorOutput::new("Something went wrong");
        let json = output.to_json();
        let deserialized: ErrorOutput = serde_json::from_str(&json).unwrap();
        assert_eq!(output, deserialized);
    }
}
