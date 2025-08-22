//! Output types for analysis tools
//!
//! These types are used as the return values from analysis tool methods.
//! They are serialized to JSON strings for the MCP protocol, and can be
//! deserialized in tests for type-safe validation.

use serde::{Deserialize, Serialize};

/// Enhanced node structure for crate structure analysis
#[derive(Debug, Serialize, Deserialize, PartialEq, Clone)]
pub struct StructureNode {
    pub kind: String,
    pub name: String,
    pub path: String,
    pub visibility: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub children: Option<Vec<StructureNode>>,
}

/// Output from structure (analyze_crate_structure) operation
#[derive(Debug, Serialize, Deserialize, PartialEq)]
pub struct StructureOutput {
    pub status: String,
    pub message: String,
    pub tree: StructureNode,
    pub usage_hint: String,
}

impl StructureOutput {
    /// Convert to JSON string for MCP response
    pub fn to_json(&self) -> String {
        serde_json::to_string(self)
            .unwrap_or_else(|_| r#"{"error":"Failed to serialize response"}"#.to_string())
    }

    /// Check if this is a success response
    pub fn is_success(&self) -> bool {
        self.status == "success"
    }
}

/// Error output for analysis tools
#[derive(Debug, Serialize, Deserialize, PartialEq)]
pub struct AnalysisErrorOutput {
    pub error: String,
}

impl AnalysisErrorOutput {
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
    fn test_structure_output_serialization() {
        let output = StructureOutput {
            status: "success".to_string(),
            message: "Module structure analysis completed".to_string(),
            tree: StructureNode {
                kind: "module".to_string(),
                name: "root".to_string(),
                path: "".to_string(),
                visibility: "public".to_string(),
                children: Some(vec![StructureNode {
                    kind: "struct".to_string(),
                    name: "MyStruct".to_string(),
                    path: "my_mod".to_string(),
                    visibility: "public".to_string(),
                    children: None,
                }]),
            },
            usage_hint: "Use the 'path' and 'name' fields to search for items".to_string(),
        };

        assert!(output.is_success());

        let json = output.to_json();
        let deserialized: StructureOutput = serde_json::from_str(&json).unwrap();
        assert_eq!(output, deserialized);
    }

    #[test]
    fn test_analysis_error_output() {
        let output = AnalysisErrorOutput::new("Failed to analyze crate");
        let json = output.to_json();
        let deserialized: AnalysisErrorOutput = serde_json::from_str(&json).unwrap();
        assert_eq!(output, deserialized);
    }
}
