use anyhow::Result;
use rust_docs_mcp::cache::storage::CacheStorage;
use serde_json::Value;
use std::process::{Command, Output};

fn run(cache: &std::path::Path, args: &[&str]) -> Result<Output> {
    Ok(Command::new(env!("CARGO_BIN_EXE_rust-docs-mcp"))
        .args(args)
        .arg("--cache-dir")
        .arg(cache)
        .output()?)
}

#[test]
fn list_and_batch_validation_work_without_network() -> Result<()> {
    let cache = tempfile::tempdir()?;
    let output = run(cache.path(), &["cache", "list"])?;
    assert!(output.status.success());
    let listed: Value = serde_json::from_slice(&output.stdout)?;
    assert_eq!(listed["total_crates"], 0);

    let output = run(
        cache.path(),
        &["cache", "add", "bad@not-a-version", "../bad"],
    )?;
    assert_eq!(output.status.code(), Some(1));
    let batch: Value = serde_json::from_slice(&output.stdout)?;
    assert_eq!(batch["results"].as_array().unwrap().len(), 2);
    assert!(
        batch["results"]
            .as_array()
            .unwrap()
            .iter()
            .all(|r| r["status"] == "error")
    );
    assert_eq!(batch["failed"], true);

    for args in [
        vec!["cache", "add"],
        vec!["cache", "update"],
        vec!["cache", "update", "--all", "serde"],
    ] {
        assert_eq!(run(cache.path(), &args)?.status.code(), Some(2));
    }
    Ok(())
}

#[test]
fn update_skips_local_and_git_and_reports_unknown_names() -> Result<()> {
    let cache = tempfile::tempdir()?;
    let storage = CacheStorage::new(Some(cache.path().to_owned()))?;
    for (name, source) in [("local-example", "local"), ("git-example", "github")] {
        storage.ensure_dir(&storage.source_path(name, "0.1.0")?)?;
        storage.save_metadata_with_source(name, "0.1.0", source, None, None)?;
    }
    let output = run(cache.path(), &["cache", "update", "--all"])?;
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let batch: Value = serde_json::from_slice(&output.stdout)?;
    assert_eq!(batch["results"].as_array().unwrap().len(), 2);
    assert!(
        batch["results"]
            .as_array()
            .unwrap()
            .iter()
            .all(|r| r["status"] == "skipped")
    );
    assert_eq!(storage.list_cached_crates()?.len(), 2);

    let output = run(
        cache.path(),
        &["cache", "update", "unknown", "local-example"],
    )?;
    assert_eq!(output.status.code(), Some(1));
    let batch: Value = serde_json::from_slice(&output.stdout)?;
    assert!(
        batch["results"]
            .as_array()
            .unwrap()
            .iter()
            .any(|r| r["status"] == "error")
    );
    assert!(
        batch["results"]
            .as_array()
            .unwrap()
            .iter()
            .any(|r| r["status"] == "skipped")
    );
    Ok(())
}
