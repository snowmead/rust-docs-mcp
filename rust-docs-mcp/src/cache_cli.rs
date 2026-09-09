//! Blocking cache commands for shell use. Every batch reports each result.

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use anyhow::{Context, Result, bail};
use clap::{Args, Subcommand};
use semver::{Version, VersionReq};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::cache::features::FeatureOptions;
use crate::cache::storage::CacheStorage;
use crate::cache::tools::CacheCrateParams;
use crate::cache::types::CrateIdentifier;
use crate::cli::CliCallResult;
use crate::runtime::RustDocsRuntime;

#[derive(Debug, Subcommand)]
pub enum CacheCommand {
    /// List cached crates, versions, members, and feature variants as JSON
    List,
    /// Cache one or more crates. Full versions pin exactly; partial versions are requirements.
    Add {
        #[arg(required = true, num_args = 1..)]
        crates: Vec<String>,
        #[command(flatten)]
        features: CacheFeatureArgs,
    },
    /// Cache the latest stable crates.io releases, retaining old versions and feature selections
    Update {
        #[arg(long, conflicts_with = "crates")]
        all: bool,
        #[arg(required_unless_present = "all", num_args = 1..)]
        crates: Vec<String>,
    },
}

#[derive(Debug, Default, Args)]
pub struct CacheFeatureArgs {
    /// Comma-separated features. Disables defaults unless --no-default-features=false.
    #[arg(long, value_delimiter = ',')]
    pub features: Option<Vec<String>>,
    /// Disable default features; pass =false to add features to Cargo defaults
    #[arg(long, num_args = 0..=1, require_equals = true, default_missing_value = "true")]
    pub no_default_features: Option<bool>,
    /// Enable all features with no fallback; pass =false for Cargo defaults
    #[arg(long, num_args = 0..=1, require_equals = true, default_missing_value = "true")]
    pub all_features: Option<bool>,
}

#[derive(Debug)]
struct CrateSpec {
    name: String,
    requirement: VersionReq,
    exact: Option<Version>,
}

impl CrateSpec {
    fn parse(input: &str) -> Result<Self> {
        let (name, requested) = input
            .split_once('@')
            .map_or((input, None), |(name, version)| (name, Some(version)));
        CrateIdentifier::new(name, "0.0.0")?;
        let requirement = VersionReq::parse(requested.unwrap_or("*"))
            .with_context(|| format!("Invalid version requirement in '{input}'"))?;
        Ok(Self {
            name: name.to_owned(),
            requirement,
            exact: requested.and_then(|v| Version::parse(v).ok()),
        })
    }
}

#[derive(Deserialize)]
struct RegistryVersions {
    versions: Vec<RegistryVersion>,
}

#[derive(Deserialize)]
struct RegistryVersion {
    num: Version,
    yanked: bool,
}

fn select_version(spec: &CrateSpec, versions: &[RegistryVersion]) -> Result<String> {
    versions
        .iter()
        .filter(|v| !v.yanked)
        .filter(|v| {
            spec.exact
                .as_ref()
                .map_or_else(|| spec.requirement.matches(&v.num), |exact| exact == &v.num)
        })
        .map(|v| &v.num)
        .max()
        .map(ToString::to_string)
        .with_context(|| {
            format!(
                "No non-yanked version of {} matches {}",
                spec.name, spec.requirement
            )
        })
}

async fn resolve_version(client: &reqwest::Client, spec: &CrateSpec) -> Result<String> {
    let response: RegistryVersions = client
        .get(format!("https://crates.io/api/v1/crates/{}", spec.name))
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    select_version(spec, &response.versions)
}

fn cache_params(
    name: &str,
    version: &str,
    member: Option<String>,
    options: &FeatureOptions,
) -> CacheCrateParams {
    let (features, no_default_features, all_features) = match options {
        FeatureOptions::Auto => (None, None, None),
        FeatureOptions::All => (None, None, Some(true)),
        FeatureOptions::Selected {
            features,
            no_default_features,
        } => (Some(features.clone()), Some(*no_default_features), None),
    };
    CacheCrateParams {
        crate_name: name.to_owned(),
        source_type: "cratesio".into(),
        version: Some(version.to_owned()),
        github_url: None,
        branch: None,
        tag: None,
        path: None,
        members: member.map(|m| vec![m]),
        update: None,
        features,
        no_default_features,
        all_features,
    }
}

async fn cache_one(
    runtime: &RustDocsRuntime,
    storage: &CacheStorage,
    name: &str,
    version: &str,
    member: Option<String>,
    options: &FeatureOptions,
) -> Result<Value> {
    if let Ok(metadata) = storage.load_metadata(name, version, None)
        && metadata.source != "crates.io"
    {
        bail!(
            "{name}@{version} is cached from {}; refusing to treat it as a crates.io release",
            metadata.source
        );
    }
    let result = runtime
        .cache_crate_blocking(cache_params(name, version, member, options))
        .await;
    serde_json::from_str(&result).context("Invalid cache response")
}

fn batch_result(results: Vec<Value>) -> Result<CliCallResult> {
    let failed = results.iter().any(|r| {
        !matches!(
            r["status"].as_str(),
            Some("success" | "skipped" | "up_to_date")
        )
    });
    Ok(CliCallResult {
        output: serde_json::to_string(&json!({"results": results, "failed": failed}))?,
        failed,
    })
}

pub async fn run(cache_dir: Option<PathBuf>, command: CacheCommand) -> Result<CliCallResult> {
    let runtime = RustDocsRuntime::new(cache_dir.clone())?;
    if matches!(command, CacheCommand::List) {
        let output = runtime.list_cached_crates().await;
        return Ok(CliCallResult {
            failed: crate::cli::output_indicates_error(&output),
            output,
        });
    }
    let storage = CacheStorage::new(cache_dir)?;
    let client = reqwest::Client::builder()
        .user_agent(format!(
            "rust-docs-mcp/{} ({})",
            env!("CARGO_PKG_VERSION"),
            env!("CARGO_PKG_REPOSITORY")
        ))
        .timeout(std::time::Duration::from_secs(60))
        .build()?;
    let mut results = Vec::new();
    match command {
        CacheCommand::List => unreachable!(),
        CacheCommand::Add { crates, features } => {
            let options = FeatureOptions::new(
                features.features,
                features.no_default_features,
                features.all_features,
            )?;
            for input in crates {
                let result = async {
                    let spec = CrateSpec::parse(&input)?;
                    let version = resolve_version(&client, &spec).await?;
                    eprintln!("Caching {}@{}", spec.name, version);
                    cache_one(&runtime, &storage, &spec.name, &version, None, &options).await
                }
                .await;
                results.push(result.unwrap_or_else(
                    |e| json!({"crate": input, "status": "error", "error": format!("{e:#}")}),
                ));
            }
        }
        CacheCommand::Update { all, crates } => {
            let mut cached = BTreeMap::<String, Vec<_>>::new();
            for entry in storage.list_cached_crates()? {
                cached.entry(entry.name.clone()).or_default().push(entry);
            }
            let names: BTreeSet<String> = if all {
                cached.keys().cloned().collect()
            } else {
                crates.into_iter().collect()
            };
            for name in names {
                let Some(entries) = cached.get(&name) else {
                    results.push(
                        json!({"crate": name, "status": "error", "error": "Crate is not cached"}),
                    );
                    continue;
                };
                let registry_entries: Vec<_> =
                    entries.iter().filter(|e| e.source == "crates.io").collect();
                if registry_entries.is_empty() {
                    results.push(json!({"crate": name, "status": "skipped", "reason": "Only local or Git sources are cached"}));
                    continue;
                }
                let result = async {
                    let version = resolve_version(&client, &CrateSpec::parse(&name)?).await?;
                    let mut variants = BTreeMap::new();
                    for entry in registry_entries {
                        for variant in storage.list_variants(&name, &entry.version)? {
                            variants.insert((variant.member, variant.build.requested.fingerprint()), variant.build.requested);
                        }
                    }
                    if variants.is_empty() { variants.insert((None, FeatureOptions::Auto.fingerprint()), FeatureOptions::Auto); }
                    let mut outputs = Vec::new();
                    for ((member, _), options) in variants {
                        let scoped = storage.with_options(options.clone());
                        if scoped.has_docs(&name, &version, member.as_deref()) {
                            outputs.push(json!({"crate": name, "version": version, "member": member, "features": options, "status": "up_to_date"}));
                            continue;
                        }
                        eprintln!("Caching {name}@{version}");
                        outputs.push(cache_one(&runtime, &storage, &name, &version, member, &options).await
                            .unwrap_or_else(|e| json!({"crate": name, "version": version, "status": "error", "error": format!("{e:#}")})));
                    }
                    Ok::<_, anyhow::Error>(outputs)
                }.await;
                match result {
                    Ok(outputs) => results.extend(outputs),
                    Err(e) => results
                        .push(json!({"crate": name, "status": "error", "error": format!("{e:#}")})),
                }
            }
        }
    }
    batch_result(results)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_resolution_excludes_yanked_and_unrequested_prereleases() {
        let versions: RegistryVersions = serde_json::from_value(json!({"versions": [
            {"num": "1.0.0", "yanked": false}, {"num": "1.8.0", "yanked": false},
            {"num": "1.9.0", "yanked": true}, {"num": "2.0.0-beta.1", "yanked": false}
        ]}))
        .unwrap();
        for spec in ["example", "example@1", "example@>=1, <2"] {
            assert_eq!(
                select_version(&CrateSpec::parse(spec).unwrap(), &versions.versions).unwrap(),
                "1.8.0"
            );
        }
        assert_eq!(
            select_version(
                &CrateSpec::parse("example@1.0.0").unwrap(),
                &versions.versions
            )
            .unwrap(),
            "1.0.0"
        );
        assert_eq!(
            select_version(
                &CrateSpec::parse("example@2.0.0-beta.1").unwrap(),
                &versions.versions
            )
            .unwrap(),
            "2.0.0-beta.1"
        );
        assert!(CrateSpec::parse("../example@1").is_err());
        assert!(CrateSpec::parse("example@").is_err());
        assert!(
            select_version(&CrateSpec::parse("example@3").unwrap(), &versions.versions).is_err()
        );
    }

    #[test]
    fn partial_or_workspace_results_fail_batches() {
        assert!(
            batch_result(vec![
                json!({"status": "success"}),
                json!({"status": "partial_success"})
            ])
            .unwrap()
            .failed
        );
        assert!(
            batch_result(vec![json!({"status": "workspace_detected"})])
                .unwrap()
                .failed
        );
        assert!(
            !batch_result(vec![
                json!({"status": "skipped"}),
                json!({"status": "up_to_date"})
            ])
            .unwrap()
            .failed
        );
    }
}
