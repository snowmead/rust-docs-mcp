//! Output types for documentation tools
//!
//! These types are used as the return values from docs tool methods.
//! They are serialized to JSON strings for the MCP protocol, and can be
//! deserialized in tests for type-safe validation.

use serde::{Deserialize, Serialize};

/// Simplified item information for API responses
#[derive(Debug, Serialize, Deserialize, PartialEq, Clone)]
pub struct ItemInfo {
    pub id: String,
    pub name: String,
    pub kind: String,
    pub path: Vec<String>,
    pub docs: Option<String>,
    pub visibility: String,
}

/// Preview item info for lightweight responses
#[derive(Debug, Serialize, Deserialize, PartialEq, Clone)]
pub struct ItemPreview {
    pub id: String,
    pub name: String,
    pub kind: String,
    pub path: Vec<String>,
}

/// Pagination information
#[derive(Debug, Serialize, Deserialize, PartialEq, Clone)]
pub struct PaginationInfo {
    pub total: usize,
    pub limit: usize,
    pub offset: usize,
    pub has_more: bool,
}

/// Output from list_crate_items operation
#[derive(Debug, Serialize, Deserialize, PartialEq)]
pub struct ListCrateItemsOutput {
    pub items: Vec<ItemInfo>,
    pub pagination: PaginationInfo,
}

impl ListCrateItemsOutput {
    /// Convert to JSON string for MCP response
    pub fn to_json(&self) -> String {
        serde_json::to_string(self)
            .unwrap_or_else(|_| r#"{"error":"Failed to serialize response"}"#.to_string())
    }
}

/// Output from search_items operation (full details)
#[derive(Debug, Serialize, Deserialize, PartialEq)]
pub struct SearchItemsOutput {
    pub items: Vec<ItemInfo>,
    pub pagination: PaginationInfo,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub warning: Option<String>,
}

impl SearchItemsOutput {
    /// Convert to JSON string for MCP response
    pub fn to_json(&self) -> String {
        serde_json::to_string(self)
            .unwrap_or_else(|_| r#"{"error":"Failed to serialize response"}"#.to_string())
    }
}

/// Output from search_items_preview operation (lightweight)
#[derive(Debug, Serialize, Deserialize, PartialEq)]
pub struct SearchItemsPreviewOutput {
    pub items: Vec<ItemPreview>,
    pub pagination: PaginationInfo,
}

impl SearchItemsPreviewOutput {
    /// Convert to JSON string for MCP response
    pub fn to_json(&self) -> String {
        serde_json::to_string(self)
            .unwrap_or_else(|_| r#"{"error":"Failed to serialize response"}"#.to_string())
    }
}

/// Source location information
#[derive(Debug, Serialize, Deserialize, PartialEq, Clone)]
pub struct SourceLocation {
    pub filename: String,
    pub line_start: usize,
    pub column_start: usize,
    pub line_end: usize,
    pub column_end: usize,
}

/// Detailed item information including signatures
#[derive(Debug, Serialize, Deserialize, PartialEq)]
pub struct DetailedItem {
    pub info: ItemInfo,
    pub signature: Option<String>,
    pub generics: Option<serde_json::Value>,
    pub fields: Option<Vec<ItemInfo>>,
    pub variants: Option<Vec<ItemInfo>>,
    pub methods: Option<Vec<ItemInfo>>,
    pub source_location: Option<SourceLocation>,
}

/// Output from get_item_details operation
#[derive(Debug, Serialize, Deserialize, PartialEq)]
#[serde(untagged)]
pub enum GetItemDetailsOutput {
    Success(DetailedItem),
    Error { error: String },
}

impl GetItemDetailsOutput {
    /// Convert to JSON string for MCP response
    pub fn to_json(&self) -> String {
        serde_json::to_string(self)
            .unwrap_or_else(|_| r#"{"error":"Failed to serialize response"}"#.to_string())
    }
    
    /// Check if this is a success response
    pub fn is_success(&self) -> bool {
        matches!(self, GetItemDetailsOutput::Success(_))
    }
    
    /// Check if this is an error response
    pub fn is_error(&self) -> bool {
        matches!(self, GetItemDetailsOutput::Error { .. })
    }
}

/// Output from get_item_docs operation
#[derive(Debug, Serialize, Deserialize, PartialEq)]
pub struct GetItemDocsOutput {
    pub documentation: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

impl GetItemDocsOutput {
    /// Convert to JSON string for MCP response
    pub fn to_json(&self) -> String {
        serde_json::to_string(self)
            .unwrap_or_else(|_| r#"{"error":"Failed to serialize response"}"#.to_string())
    }
}

/// Source code information for an item
#[derive(Debug, Serialize, Deserialize, PartialEq)]
pub struct SourceInfo {
    pub location: SourceLocation,
    pub code: String,
    pub context_lines: Option<usize>,
}

/// Output from get_item_source operation
#[derive(Debug, Serialize, Deserialize, PartialEq)]
#[serde(untagged)]
pub enum GetItemSourceOutput {
    Success(SourceInfo),
    Error { error: String },
}

impl GetItemSourceOutput {
    /// Convert to JSON string for MCP response
    pub fn to_json(&self) -> String {
        serde_json::to_string(self)
            .unwrap_or_else(|_| r#"{"error":"Failed to serialize response"}"#.to_string())
    }
    
    /// Check if this is a success response
    pub fn is_success(&self) -> bool {
        matches!(self, GetItemSourceOutput::Success(_))
    }
    
    /// Check if this is an error response
    pub fn is_error(&self) -> bool {
        matches!(self, GetItemSourceOutput::Error { .. })
    }
}

/// Generic error output for docs tools
#[derive(Debug, Serialize, Deserialize, PartialEq)]
pub struct DocsErrorOutput {
    pub error: String,
}

impl DocsErrorOutput {
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
    fn test_list_items_output_serialization() {
        let output = ListCrateItemsOutput {
            items: vec![
                ItemInfo {
                    id: "1".to_string(),
                    name: "test_fn".to_string(),
                    kind: "function".to_string(),
                    path: vec!["test".to_string()],
                    docs: Some("Test function".to_string()),
                    visibility: "public".to_string(),
                }
            ],
            pagination: PaginationInfo {
                total: 1,
                limit: 100,
                offset: 0,
                has_more: false,
            },
        };
        
        let json = output.to_json();
        let deserialized: ListCrateItemsOutput = serde_json::from_str(&json).unwrap();
        assert_eq!(output, deserialized);
    }
    
    #[test]
    fn test_search_preview_output() {
        let output = SearchItemsPreviewOutput {
            items: vec![
                ItemPreview {
                    id: "42".to_string(),
                    name: "MyStruct".to_string(),
                    kind: "struct".to_string(),
                    path: vec!["my_mod".to_string()],
                }
            ],
            pagination: PaginationInfo {
                total: 1,
                limit: 10,
                offset: 0,
                has_more: false,
            },
        };
        
        let json = output.to_json();
        let deserialized: SearchItemsPreviewOutput = serde_json::from_str(&json).unwrap();
        assert_eq!(output, deserialized);
    }
    
    #[test]
    fn test_item_details_output() {
        let success = GetItemDetailsOutput::Success(DetailedItem {
            info: ItemInfo {
                id: "1".to_string(),
                name: "test".to_string(),
                kind: "function".to_string(),
                path: vec![],
                docs: None,
                visibility: "public".to_string(),
            },
            signature: Some("fn test()".to_string()),
            generics: None,
            fields: None,
            variants: None,
            methods: None,
            source_location: None,
        });
        
        assert!(success.is_success());
        assert!(!success.is_error());
        
        let error = GetItemDetailsOutput::Error {
            error: "Not found".to_string(),
        };
        
        assert!(!error.is_success());
        assert!(error.is_error());
    }
}