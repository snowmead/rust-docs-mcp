//! Output types for search tools
//!
//! These types are used as the return values from search tool methods.
//! They are serialized to JSON strings for the MCP protocol, and can be
//! deserialized in tests for type-safe validation.

use serde::{Deserialize, Serialize};

/// Individual search result item
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SearchResult {
    /// Relevance score
    pub score: f32,
    /// Item ID
    pub item_id: u32,
    /// Item name
    pub name: String,
    /// Item path
    pub path: String,
    /// Item kind
    pub kind: String,
    /// Crate name
    pub crate_name: String,
    /// Crate version
    pub version: String,
    /// Item visibility
    pub visibility: String,
    /// Documentation preview (optional)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub doc_preview: Option<String>,
    /// Workspace member (optional)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub member: Option<String>,
}

/// Output from search_items_fuzzy operation
#[derive(Debug, Serialize, Deserialize, PartialEq)]
pub struct SearchItemsFuzzyOutput {
    pub results: Vec<SearchResult>,
    pub query: String,
    pub total_results: usize,
    pub fuzzy_enabled: bool,
    pub crate_name: String,
    pub version: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub member: Option<String>,
}

impl SearchItemsFuzzyOutput {
    /// Convert to JSON string for MCP response
    pub fn to_json(&self) -> String {
        serde_json::to_string(self)
            .unwrap_or_else(|_| r#"{"error":"Failed to serialize response"}"#.to_string())
    }
    
    /// Check if there are any results
    pub fn has_results(&self) -> bool {
        !self.results.is_empty()
    }
}

/// Error output for search tools
#[derive(Debug, Serialize, Deserialize, PartialEq)]
pub struct SearchErrorOutput {
    pub error: String,
}

impl SearchErrorOutput {
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
    fn test_search_fuzzy_output_serialization() {
        let output = SearchItemsFuzzyOutput {
            results: vec![
                SearchResult {
                    score: 0.95,
                    item_id: 42,
                    name: "deserialize".to_string(),
                    path: "serde::de".to_string(),
                    kind: "function".to_string(),
                    crate_name: "serde".to_string(),
                    version: "1.0.0".to_string(),
                    visibility: "public".to_string(),
                    doc_preview: Some("Deserialize a value".to_string()),
                    member: None,
                }
            ],
            query: "deserialize".to_string(),
            total_results: 1,
            fuzzy_enabled: true,
            crate_name: "serde".to_string(),
            version: "1.0.0".to_string(),
            member: None,
        };
        
        assert!(output.has_results());
        
        let json = output.to_json();
        let deserialized: SearchItemsFuzzyOutput = serde_json::from_str(&json).unwrap();
        assert_eq!(output, deserialized);
    }
    
    #[test]
    fn test_search_error_output() {
        let output = SearchErrorOutput::new("Search failed");
        let json = output.to_json();
        let deserialized: SearchErrorOutput = serde_json::from_str(&json).unwrap();
        assert_eq!(output, deserialized);
    }
}