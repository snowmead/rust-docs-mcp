use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// A normalized build request. `Auto` preserves the historical fallback policy.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "mode", rename_all = "snake_case")]
pub enum FeatureOptions {
    #[default]
    Auto,
    All,
    Selected {
        features: Vec<String>,
        no_default_features: bool,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BuildMetadata {
    pub requested: FeatureOptions,
    pub effective: FeatureOptions,
    pub toolchain: String,
    pub format_version: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CachedVariant {
    pub fingerprint: String,
    pub member: Option<String>,
    #[serde(flatten)]
    pub build: BuildMetadata,
}

impl FeatureOptions {
    pub fn new(
        features: Option<Vec<String>>,
        no_default: Option<bool>,
        all: Option<bool>,
    ) -> Result<Self> {
        if all == Some(true) {
            if features.is_some() || no_default == Some(true) {
                bail!(
                    "all_features=true cannot be combined with features or no_default_features=true"
                );
            }
            return Ok(Self::All);
        }
        if features.is_none() && no_default.is_none() && all.is_none() {
            return Ok(Self::Auto);
        }
        let disable_defaults = no_default.unwrap_or(features.is_some());
        let mut features: Vec<String> = features
            .unwrap_or_default()
            .into_iter()
            .flat_map(|f| {
                f.split([',', ' '])
                    .filter(|s| !s.is_empty())
                    .map(str::to_owned)
                    .collect::<Vec<_>>()
            })
            .collect();
        if features
            .iter()
            .any(|f| f.starts_with('-') || f.chars().any(char::is_control))
        {
            bail!("Feature names must not start with '-' or contain control characters");
        }
        features.sort();
        features.dedup();
        Ok(Self::Selected {
            features,
            no_default_features: disable_defaults,
        })
    }

    /// Apply the legacy feature-list API while retaining an explicit defaults setting.
    pub(crate) fn with_feature_list(&self, features: Option<Vec<String>>) -> Result<Self> {
        let Some(features) = features else {
            return Ok(self.clone());
        };
        let no_default = match self {
            Self::Selected {
                no_default_features,
                ..
            } => Some(*no_default_features),
            _ => None,
        };
        Self::new(Some(features), no_default, None)
    }

    pub fn fingerprint(&self) -> String {
        // The layout revision also invalidates indexes that omitted re-exports.
        format!(
            "v1-{:x}",
            Sha256::digest(serde_json::to_vec(self).expect("feature options serialize"))
        )
    }

    pub fn args(&self) -> Vec<String> {
        match self {
            Self::Auto => unreachable!("resolve the fallback strategy before building"),
            Self::All => vec!["--all-features".into()],
            Self::Selected {
                features,
                no_default_features,
            } => {
                let mut args = Vec::new();
                if *no_default_features {
                    args.push("--no-default-features".into());
                }
                if !features.is_empty() {
                    args.extend(["--features".into(), features.join(",")]);
                }
                args
            }
        }
    }

    pub fn strategies(&self) -> Vec<Self> {
        match self {
            Self::Auto => vec![
                Self::All,
                Self::Selected {
                    features: vec![],
                    no_default_features: false,
                },
                Self::Selected {
                    features: vec![],
                    no_default_features: true,
                },
            ],
            explicit => vec![explicit.clone()],
        }
    }
}

impl std::fmt::Display for FeatureOptions {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Auto => f.write_str("automatic feature fallback"),
            Self::All => f.write_str("all features enabled"),
            Self::Selected {
                features,
                no_default_features: false,
            } if features.is_empty() => f.write_str("default features only"),
            _ => write!(f, "{}", self.args().join(" ")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalized_identity_and_explicit_defaults() {
        let a = FeatureOptions::new(Some(vec!["b,a".into(), "a".into()]), None, None).unwrap();
        let b = FeatureOptions::new(Some(vec!["a".into(), "b".into()]), Some(true), None).unwrap();
        assert_eq!(a.fingerprint(), b.fingerprint());
        assert_ne!(a.fingerprint(), FeatureOptions::Auto.fingerprint());
        assert_eq!(a.args(), ["--no-default-features", "--features", "a,b"]);
        assert!(
            FeatureOptions::new(None, None, Some(false))
                .unwrap()
                .args()
                .is_empty()
        );
        assert!(FeatureOptions::new(Some(vec![]), None, Some(true)).is_err());
        assert_eq!(FeatureOptions::Auto.strategies().len(), 3);
        assert_eq!(a.strategies().len(), 1);
    }
}
