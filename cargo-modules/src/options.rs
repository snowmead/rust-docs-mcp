// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use std::path::PathBuf;
use clap::Args;

#[derive(Args, Clone, PartialEq, Eq, Debug)]
pub struct GeneralOptions {
    /// Use verbose output.
    #[arg(long, short)]
    pub verbose: bool,
}

#[derive(Args, Clone, PartialEq, Eq, Debug)]
pub struct ProjectOptions {
    /// Process only this package's library.
    #[arg(long)]
    pub lib: bool,

    /// Process only the specified binary.
    #[arg(long, value_name = "NAME")]
    pub bin: Option<String>,

    /// Package to process (see `cargo help pkgid`).
    #[arg(long, short, value_name = "SPEC")]
    pub package: Option<String>,

    /// Do not activate the `default` feature.
    #[arg(long)]
    pub no_default_features: bool,

    /// Activate all available features.
    #[arg(long)]
    pub all_features: bool,

    /// List of features to activate.
    /// This will be ignored if `--cargo-all-features` is provided.
    #[arg(long, value_name = "FEATURES")]
    pub features: Vec<String>,

    /// Analyze for target triple.
    #[arg(long, value_name = "TARGET")]
    pub target: Option<String>,

    /// Path to Cargo.toml.
    #[arg(long, value_name = "PATH", default_value = ".")]
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
