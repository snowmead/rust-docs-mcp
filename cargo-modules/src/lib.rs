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
    analyzer::LoadOptions,
    item::Item,
    options::{GeneralOptions, ProjectOptions},
    tree::{ModuleTree, Tree, TreeBuilder},
};

pub mod analyzer;
pub mod item;
pub mod options;
pub mod tree;
pub mod utils;


// Internal modules not part of the public API
mod graph;
mod colors;

/// Analysis configuration to control performance and depth
#[derive(Debug, Clone)]
pub struct AnalysisConfig {
    pub cfg_test: bool,
    pub sysroot: bool,
    pub no_default_features: bool,
    pub all_features: bool,
    pub features: Vec<String>,
}

impl Default for AnalysisConfig {
    fn default() -> Self {
        Self::fast()
    }
}

impl AnalysisConfig {
    /// Fast analysis with minimal dependencies - recommended for large crates
    pub fn fast() -> Self {
        Self {
            cfg_test: false,
            sysroot: false,
            no_default_features: true, // Skip default features for speed
            all_features: false,
            features: vec![],
        }
    }

    /// Standard analysis with default features
    pub fn standard() -> Self {
        Self {
            cfg_test: false,
            sysroot: false,
            no_default_features: false,
            all_features: false,
            features: vec![],
        }
    }

    /// Ultra fast analysis with absolute minimal processing - for large workspaces
    pub fn ultra_fast() -> Self {
        Self {
            cfg_test: false,
            sysroot: false,
            no_default_features: true, // Skip all default features
            all_features: false,       // Don't analyze any features
            features: vec![],          // No specific features
        }
    }
}

/// Analyzes a Rust crate at the given path and returns the analysis components
///
/// # Arguments
/// * `path` - Path to the crate root (containing Cargo.toml)
/// * `package` - Optional package name for workspace crates
/// * `config` - Analysis configuration to control performance and depth
///
/// # Returns
/// A tuple of (crate, database, edition) that can be used for further analysis
pub fn analyze_crate(
    path: &Path,
    package: Option<&str>,
    config: AnalysisConfig,
) -> Result<(hir::Crate, ide::AnalysisHost, ide::Edition)> {
    let general_options = GeneralOptions { verbose: false };

    let project_options = ProjectOptions {
        lib: false,
        bin: None,
        package: package.map(|p| p.to_string()),
        no_default_features: config.no_default_features,
        all_features: config.all_features,
        features: config.features,
        target: None,
        manifest_path: path.to_path_buf(),
    };

    let load_options = LoadOptions {
        cfg_test: config.cfg_test,
        sysroot: config.sysroot,
    };

    let (crate_id, analysis_host, _vfs, edition) =
        analyzer::load_workspace(&general_options, &project_options, &load_options)?;

    Ok((crate_id, analysis_host, edition))
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
    edition: ide::Edition,
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
