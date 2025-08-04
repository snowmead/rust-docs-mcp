pub mod outputs;
pub mod tools;

use rmcp::schemars;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Response for dependency information
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct DependencyInfo {
    /// The crate name and version being queried
    pub crate_info: CrateIdentifier,

    /// Direct dependencies of the crate
    pub direct_dependencies: Vec<Dependency>,

    /// Full dependency tree (only included if requested)
    pub dependency_tree: Option<serde_json::Value>,

    /// Total number of dependencies (direct + transitive)
    pub total_dependencies: usize,
}

/// Identifies a crate with name and version
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct CrateIdentifier {
    pub name: String,
    pub version: String,
}

/// Information about a single dependency
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
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

/// Process cargo metadata output to extract dependency information
pub fn process_cargo_metadata(
    metadata: &serde_json::Value,
    crate_name: &str,
    crate_version: &str,
    include_tree: bool,
    filter: Option<&str>,
) -> anyhow::Result<DependencyInfo> {
    // Find the package in the metadata
    let packages = metadata["packages"]
        .as_array()
        .ok_or_else(|| anyhow::anyhow!("No packages found in metadata"))?;

    let package = packages
        .iter()
        .find(|p| {
            p["name"].as_str() == Some(crate_name) && p["version"].as_str() == Some(crate_version)
        })
        .ok_or_else(|| {
            anyhow::anyhow!(
                "Package {}-{} not found in metadata",
                crate_name,
                crate_version
            )
        })?;

    // Extract direct dependencies
    let mut direct_dependencies = Vec::new();

    if let Some(deps) = package["dependencies"].as_array() {
        for dep in deps {
            let name = dep["name"].as_str().unwrap_or_default();

            // Apply filter if provided (case-insensitive)
            if let Some(filter_str) = filter
                && !name.to_lowercase().contains(&filter_str.to_lowercase())
            {
                continue;
            }

            // Find resolved version from the resolve section
            let resolved_version = find_resolved_version(metadata, crate_name, crate_version, name);

            direct_dependencies.push(Dependency {
                name: name.to_string(),
                version_req: dep["req"].as_str().unwrap_or_default().to_string(),
                resolved_version,
                kind: dep["kind"].as_str().unwrap_or("normal").to_string(),
                optional: dep["optional"].as_bool().unwrap_or(false),
                features: dep["features"]
                    .as_array()
                    .map(|arr| {
                        arr.iter()
                            .filter_map(|v| v.as_str().map(String::from))
                            .collect()
                    })
                    .unwrap_or_default(),
                target: dep["target"].as_str().map(String::from),
            });
        }
    }

    // Count total dependencies
    let total_dependencies = if let Some(resolve) = metadata["resolve"].as_object() {
        if let Some(nodes) = resolve["nodes"].as_array() {
            // Find the node for our package and count its dependencies
            nodes
                .iter()
                .find(|n| {
                    n["id"]
                        .as_str()
                        .map(|id| id.starts_with(&format!("{crate_name} {crate_version}")))
                        .unwrap_or(false)
                })
                .and_then(|n| n["dependencies"].as_array())
                .map(|deps| deps.len())
                .unwrap_or(0)
        } else {
            direct_dependencies.len()
        }
    } else {
        direct_dependencies.len()
    };

    Ok(DependencyInfo {
        crate_info: CrateIdentifier {
            name: crate_name.to_string(),
            version: crate_version.to_string(),
        },
        direct_dependencies,
        dependency_tree: if include_tree {
            Some(metadata["resolve"].clone())
        } else {
            None
        },
        total_dependencies,
    })
}

/// Find the resolved version of a dependency from the resolve section
fn find_resolved_version(
    metadata: &serde_json::Value,
    parent_name: &str,
    parent_version: &str,
    dep_name: &str,
) -> Option<String> {
    let resolve = metadata["resolve"].as_object()?;
    let nodes = resolve["nodes"].as_array()?;

    // Find the parent node
    let parent_node = nodes.iter().find(|n| {
        n["id"]
            .as_str()
            .map(|id| id.starts_with(&format!("{parent_name} {parent_version}")))
            .unwrap_or(false)
    })?;

    // Find the dependency in the parent's deps
    let deps = parent_node["deps"].as_array()?;
    for dep in deps {
        if dep["name"].as_str() == Some(dep_name) {
            // Extract version from the pkg field
            if let Some(pkg) = dep["pkg"].as_str() {
                // pkg format is "name version (source)"
                let parts: Vec<&str> = pkg.split(' ').collect();
                if parts.len() >= 2 {
                    return Some(parts[1].to_string());
                }
            }
        }
    }

    None
}
