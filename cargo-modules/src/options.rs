// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use std::path::PathBuf;

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct GeneralOptions {
    /// Use verbose output.
    pub verbose: bool,
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct ProjectOptions {
    /// Process only this package's library.
    pub lib: bool,

    /// Process only the specified binary.
    pub bin: Option<String>,

    /// Package to process (see `cargo help pkgid`).
    pub package: Option<String>,

    /// Do not activate the `default` feature.
    pub no_default_features: bool,

    /// Activate all available features.
    pub all_features: bool,

    /// List of features to activate.
    /// This will be ignored if `--cargo-all-features` is provided.
    pub features: Vec<String>,

    /// Analyze for target triple.
    pub target: Option<String>,

    /// Path to Cargo.toml.
    pub manifest_path: PathBuf,
}

impl Default for GeneralOptions {
    fn default() -> Self {
        Self { verbose: false }
    }
}

impl Default for ProjectOptions {
    fn default() -> Self {
        Self {
            lib: false,
            bin: None,
            package: None,
            no_default_features: false,
            all_features: false,
            features: vec![],
            target: None,
            manifest_path: PathBuf::from("."),
        }
    }
}
