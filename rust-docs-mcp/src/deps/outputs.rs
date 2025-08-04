//! Output types for dependency tools
//!
//! These types are used as the return values from dependency tool methods.
//! They are serialized to JSON strings for the MCP protocol, and can be
//! deserialized in tests for type-safe validation.

use serde::{Deserialize, Serialize};

/// Identifies a crate with name and version
#[derive(Debug, Serialize, Deserialize, PartialEq, Clone)]
pub struct CrateIdentifier {
    pub name: String,
    pub version: String,
}

/// Information about a single dependency
#[derive(Debug, Serialize, Deserialize, PartialEq, Clone)]
pub struct Dependency {
    /// Name of the dependency
    pub name: String,
    
    /// Version requirement specified in Cargo.toml
    pub version_req: String,
    
    /// Actual resolved version
    pub resolved_version: Option<String>,
    
    /// Kind of dependency (normal, dev, build)
    pub kind: String,
    
    /// Whether this is an optional dependency
    pub optional: bool,
    
    /// Features enabled for this dependency
    pub features: Vec<String>,
    
    /// Target platform (if dependency is platform-specific)
    pub target: Option<String>,
}

/// Output from get_dependencies operation
#[derive(Debug, Serialize, Deserialize, PartialEq)]
pub struct GetDependenciesOutput {
    /// The crate name and version being queried
    pub crate_info: CrateIdentifier,
    
    /// Direct dependencies of the crate
    pub direct_dependencies: Vec<Dependency>,
    
    /// Full dependency tree (only included if requested)
    pub dependency_tree: Option<serde_json::Value>,
    
    /// Total number of dependencies (direct + transitive)
    pub total_dependencies: usize,
}

impl GetDependenciesOutput {
    /// Convert to JSON string for MCP response
    pub fn to_json(&self) -> String {
        serde_json::to_string(self)
            .unwrap_or_else(|_| r#"{"error":"Failed to serialize response"}"#.to_string())
    }
}

/// Error output for dependency tools
#[derive(Debug, Serialize, Deserialize, PartialEq)]
pub struct DepsErrorOutput {
    pub error: String,
}

impl DepsErrorOutput {
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
    fn test_get_dependencies_output_serialization() {
        let output = GetDependenciesOutput {
            crate_info: CrateIdentifier {
                name: "test-crate".to_string(),
                version: "1.0.0".to_string(),
            },
            direct_dependencies: vec![
                Dependency {
                    name: "serde".to_string(),
                    version_req: "^1.0".to_string(),
                    resolved_version: Some("1.0.193".to_string()),
                    kind: "normal".to_string(),
                    optional: false,
                    features: vec!["derive".to_string()],
                    target: None,
                }
            ],
            dependency_tree: None,
            total_dependencies: 1,
        };
        
        let json = output.to_json();
        let deserialized: GetDependenciesOutput = serde_json::from_str(&json).unwrap();
        assert_eq!(output, deserialized);
    }
    
    #[test]
    fn test_deps_error_output() {
        let output = DepsErrorOutput::new("Dependencies not available");
        let json = output.to_json();
        let deserialized: DepsErrorOutput = serde_json::from_str(&json).unwrap();
        assert_eq!(output, deserialized);
    }
}