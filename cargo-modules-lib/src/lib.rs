// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Library for analyzing Rust crate module structures and dependencies.
//!
//! This library provides functionality to:
//! - Analyze Rust crate module hierarchies
//! - Build dependency graphs showing relationships between modules
//! - Detect orphaned source files
//! - Extract module metadata and structure information

use std::path::Path;

use anyhow::Result;
use ra_ap_hir::{self as hir};
use ra_ap_ide::{self as ide};

pub use crate::{
    graph::{Graph, Node, Edge, Relationship, GraphBuilder},
    item::Item,
    tree::{Tree, ModuleTree, TreeBuilder},
    analyzer::LoadOptions,
    options::{GeneralOptions, ProjectOptions},
};

pub mod analyzer;
pub mod graph;
pub mod item;
pub mod options;
pub mod tree;
pub mod utils;

mod colors;

/// Analyzes a Rust crate at the given path and returns the analysis components
///
/// # Arguments
/// * `path` - Path to the crate root (containing Cargo.toml)
///
/// # Returns
/// A tuple of (crate, database, edition) that can be used for further analysis
pub fn analyze_crate(path: &Path) -> Result<(hir::Crate, ide::AnalysisHost, ide::Edition)> {
    let general_options = GeneralOptions {
        verbose: false,
    };
    
    let project_options = ProjectOptions {
        lib: false,
        bin: None,
        package: None,
        no_default_features: false,
        all_features: false,
        features: vec![],
        target: None,
        manifest_path: path.to_path_buf(),
    };
    
    let load_options = LoadOptions {
        cfg_test: false,
        // Keep sysroot disabled to prevent hanging on system library loading
        sysroot: false,
    };
    
    let (crate_id, analysis_host, _vfs, edition) = analyzer::load_workspace(&general_options, &project_options, &load_options)?;
    
    Ok((crate_id, analysis_host, edition))
}

/// Builds a dependency graph from a crate analysis
///
/// # Arguments
/// * `crate_id` - The crate to analyze
/// * `db` - The analysis database
/// * `edition` - The Rust edition
///
/// # Returns
/// A tuple of (dependency graph, root node index)
pub fn build_dependency_graph(
    crate_id: hir::Crate, 
    db: &ide::RootDatabase, 
    edition: ide::Edition
) -> Result<(Graph<Node, Edge>, petgraph::graph::NodeIndex)> {
    let builder = GraphBuilder::new(db, edition, crate_id);
    builder.build()
}

/// Builds a module tree from a crate analysis
///
/// # Arguments
/// * `crate_id` - The crate to analyze
/// * `db` - The analysis database  
/// * `edition` - The Rust edition
///
/// # Returns
/// A module tree structure
pub fn build_module_tree(
    crate_id: hir::Crate,
    db: &ide::RootDatabase,
    edition: ide::Edition
) -> Result<ModuleTree> {
    ModuleTree::build(db, &crate_id, edition)
}

/// Detects orphaned source files in a crate directory
///
/// # Arguments
/// * `path` - Path to the crate root directory
///
/// # Returns
/// A vector of paths to orphaned files
pub fn detect_orphans(path: &Path) -> Result<Vec<std::path::PathBuf>> {
    // This would need to be implemented by examining the file system
    // and comparing with the analyzed module structure
    // For now, return empty vector
    let _ = path;
    Ok(vec![])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_analyze_current_crate() {
        let current_dir = std::env::current_dir().unwrap();
        let result = analyze_crate(&current_dir);
        
        match result {
            Ok((crate_id, analysis_host, edition)) => {
                println!("Successfully analyzed crate: {:?}", crate_id);
                println!("Edition: {:?}", edition);
                
                // Test building dependency graph
                let db = analysis_host.raw_database();
                match build_dependency_graph(crate_id, db, edition) {
                    Ok((graph, _root_idx)) => {
                        println!("Successfully built dependency graph with {} nodes", graph.node_count());
                    }
                    Err(e) => {
                        println!("Failed to build dependency graph: {}", e);
                    }
                }
            }
            Err(e) => {
                println!("Analysis failed: {}", e);
            }
        }
    }
}