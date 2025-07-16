use serde::{de, Deserializer};
use std::fmt;
use std::path::Path;
use std::fs;
use anyhow::{Context, Result};
use toml::{Table, Value};

/// Custom deserializer that can handle boolean values from strings, booleans, or numbers
pub fn deserialize_bool_from_anything<'de, D>(deserializer: D) -> Result<bool, D::Error>
where
    D: Deserializer<'de>,
{
    use serde::de::Visitor;

    struct BoolVisitor;

    impl<'de> Visitor<'de> for BoolVisitor {
        type Value = bool;

        fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
            formatter.write_str("a boolean, string, or number")
        }

        fn visit_bool<E>(self, value: bool) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            Ok(value)
        }

        fn visit_str<E>(self, value: &str) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            match value.to_lowercase().as_str() {
                "true" | "1" | "yes" | "on" => Ok(true),
                "false" | "0" | "no" | "off" | "" => Ok(false),
                _ => Err(E::custom(format!("cannot parse '{}' as boolean", value))),
            }
        }

        fn visit_i64<E>(self, value: i64) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            Ok(value != 0)
        }

        fn visit_u64<E>(self, value: u64) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            Ok(value != 0)
        }

        fn visit_f64<E>(self, value: f64) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            Ok(value != 0.0)
        }
    }

    deserializer.deserialize_any(BoolVisitor)
}

/// Ensures a crate is isolated from parent workspaces by adding an empty [workspace] section
/// to its Cargo.toml if it doesn't already have one.
pub fn ensure_crate_isolation(crate_path: &Path) -> Result<()> {
    let cargo_toml_path = crate_path.join("Cargo.toml");
    
    if !cargo_toml_path.exists() {
        return Ok(()); // No Cargo.toml, nothing to do
    }
    
    // Read the current Cargo.toml content
    let content = fs::read_to_string(&cargo_toml_path)
        .context("Failed to read Cargo.toml")?;
    
    // Check if it already has a [workspace] section
    let has_workspace = content.contains("[workspace]") || content.contains("[ workspace ]");
    
    if !has_workspace {
        // Append an empty workspace section to isolate from parent workspace
        let isolated_content = format!("{}\n\n# Added by rust-docs MCP to isolate from parent workspace\n[workspace]\n", content.trim_end());
        
        fs::write(&cargo_toml_path, isolated_content)
            .context("Failed to write isolated Cargo.toml")?;
        
        tracing::debug!(
            "Added empty [workspace] section to {} to isolate from parent workspace",
            cargo_toml_path.display()
        );
    }
    
    Ok(())
}

/// Information about a crate's features
#[derive(Debug, Clone)]
pub struct FeatureAnalysis {
    /// Whether the crate has any features defined
    pub has_features: bool,
    /// Total number of features
    pub feature_count: usize,
    /// Whether there are likely mutually exclusive features
    pub has_mutually_exclusive: bool,
    /// List of potentially conflicting feature groups
    pub conflict_groups: Vec<Vec<String>>,
}

/// Analyze a crate's features to determine if it's safe to use --all-features
pub fn analyze_crate_features(crate_path: &Path) -> Result<FeatureAnalysis> {
    let cargo_toml_path = crate_path.join("Cargo.toml");
    
    if !cargo_toml_path.exists() {
        return Ok(FeatureAnalysis {
            has_features: false,
            feature_count: 0,
            has_mutually_exclusive: false,
            conflict_groups: vec![],
        });
    }
    
    let content = fs::read_to_string(&cargo_toml_path)
        .context("Failed to read Cargo.toml")?;
    
    let toml_value: Table = toml::from_str(&content)
        .context("Failed to parse Cargo.toml")?;
    
    // Check if features section exists
    let features = match toml_value.get("features") {
        Some(Value::Table(features)) => features,
        _ => {
            return Ok(FeatureAnalysis {
                has_features: false,
                feature_count: 0,
                has_mutually_exclusive: false,
                conflict_groups: vec![],
            });
        }
    };
    
    let feature_count = features.len();
    if feature_count == 0 {
        return Ok(FeatureAnalysis {
            has_features: false,
            feature_count: 0,
            has_mutually_exclusive: false,
            conflict_groups: vec![],
        });
    }
    
    // Detect mutually exclusive features
    let mut conflict_groups = Vec::new();
    let feature_names: Vec<String> = features.keys().cloned().collect();
    
    // Common patterns for mutually exclusive features
    let exclusive_patterns = vec![
        // Size variants (e.g., rkyv)
        vec!["size_16", "size_32", "size_64"],
        vec!["16", "32", "64"],
        // Backend choices
        vec!["openssl", "rustls", "native-tls"],
        vec!["sync", "async", "tokio", "async-std"],
        // Platform specific
        vec!["wasm", "native", "web"],
        // Allocation strategies
        vec!["alloc", "no_alloc", "std", "no_std"],
    ];
    
    for pattern_group in exclusive_patterns {
        let mut found_features = Vec::new();
        for feature in &feature_names {
            for pattern in &pattern_group {
                if feature.contains(pattern) {
                    found_features.push(feature.clone());
                    break;
                }
            }
        }
        
        if found_features.len() > 1 {
            conflict_groups.push(found_features);
        }
    }
    
    // Check for explicit conflicts in feature definitions
    // Features that enable conflicting dependencies might also be mutually exclusive
    for (feature_name, feature_value) in features {
        if let Value::Array(deps) = feature_value {
            for dep in deps {
                if let Value::String(dep_str) = dep {
                    // Check if this feature explicitly conflicts with others
                    // This is a heuristic - features with "no_" prefix often conflict
                    if feature_name.starts_with("no_") || dep_str.starts_with("no_") {
                        let potential_conflict = feature_name.trim_start_matches("no_");
                        if features.contains_key(potential_conflict) {
                            conflict_groups.push(vec![
                                feature_name.clone(),
                                potential_conflict.to_string(),
                            ]);
                        }
                    }
                }
            }
        }
    }
    
    // Remove duplicates from conflict groups
    conflict_groups.sort();
    conflict_groups.dedup();
    
    let has_mutually_exclusive = !conflict_groups.is_empty();
    
    Ok(FeatureAnalysis {
        has_features: true,
        feature_count,
        has_mutually_exclusive,
        conflict_groups,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn test_analyze_crate_features_no_features() {
        let temp_dir = TempDir::new().unwrap();
        let cargo_toml = temp_dir.path().join("Cargo.toml");
        
        fs::write(&cargo_toml, r#"
[package]
name = "test"
version = "0.1.0"
"#).unwrap();
        
        let analysis = analyze_crate_features(temp_dir.path()).unwrap();
        assert!(!analysis.has_features);
        assert_eq!(analysis.feature_count, 0);
        assert!(!analysis.has_mutually_exclusive);
    }

    #[test]
    fn test_analyze_crate_features_with_safe_features() {
        let temp_dir = TempDir::new().unwrap();
        let cargo_toml = temp_dir.path().join("Cargo.toml");
        
        fs::write(&cargo_toml, r#"
[package]
name = "test"
version = "0.1.0"

[features]
serde = ["dep:serde"]
json = ["serde", "serde_json"]
"#).unwrap();
        
        let analysis = analyze_crate_features(temp_dir.path()).unwrap();
        assert!(analysis.has_features);
        assert_eq!(analysis.feature_count, 2);
        assert!(!analysis.has_mutually_exclusive);
    }

    #[test]
    fn test_analyze_crate_features_with_mutually_exclusive() {
        let temp_dir = TempDir::new().unwrap();
        let cargo_toml = temp_dir.path().join("Cargo.toml");
        
        fs::write(&cargo_toml, r#"
[package]
name = "test"
version = "0.1.0"

[features]
size_16 = []
size_32 = []
size_64 = []
"#).unwrap();
        
        let analysis = analyze_crate_features(temp_dir.path()).unwrap();
        assert!(analysis.has_features);
        assert_eq!(analysis.feature_count, 3);
        assert!(analysis.has_mutually_exclusive);
        assert!(!analysis.conflict_groups.is_empty());
    }
}