//! End-to-end test for the Cargo registry source reuse path.
//!
//! This file lives in its own integration-test binary (separate from
//! `tests/integration_tests.rs`) because it mutates the process-global
//! `CARGO_HOME` env var, which would interfere with the other 22 tests that
//! rely on the ambient toolchain configuration.

use anyhow::{Context, Result};
use rmcp::handler::server::wrapper::Parameters;
use rust_docs_mcp::RustDocsService;
use rust_docs_mcp::cache::constants::CARGO_TOML;
use rust_docs_mcp::cache::outputs::CacheTaskStartedOutput;
use rust_docs_mcp::cache::storage::CacheStorage;
use rust_docs_mcp::cache::tools::{CacheCrateParams, CacheOperationsParams};
use std::path::Path;
use std::sync::{Mutex, OnceLock};
use std::time::Duration;
use tempfile::TempDir;

const TEST_TIMEOUT: Duration = Duration::from_secs(60);

/// Serializes `CARGO_HOME` mutations between tests in this binary. Only tests
/// in this file contend for this lock; other integration test binaries run in
/// separate processes and don't touch it.
fn cargo_home_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

/// RAII drop guard that removes `CARGO_HOME` when the test ends, so a panic
/// in the middle of the test doesn't leak env state into later tests if any
/// are added to this binary.
struct CargoHomeGuard;

impl Drop for CargoHomeGuard {
    fn drop(&mut self) {
        unsafe {
            std::env::remove_var("CARGO_HOME");
        }
    }
}

fn set_cargo_home(path: &Path) -> CargoHomeGuard {
    unsafe {
        std::env::set_var("CARGO_HOME", path);
    }
    CargoHomeGuard
}

// Minimal copies of the helpers in `tests/integration_tests.rs`. Duplicating
// them here (rather than extracting to a shared module) keeps the scope of
// this change small — no refactor of the existing test harness.
fn parse_cache_task_started(response: &str) -> Result<CacheTaskStartedOutput> {
    serde_json::from_str(response).map_err(|e| {
        anyhow::anyhow!("Failed to parse task started response: {e}\nResponse: {response}")
    })
}

#[derive(Debug)]
#[allow(dead_code)] // Fields are used for diagnostic output via Debug
enum TaskTerminal {
    Success,
    Failed(String),
    Other(String),
}

async fn wait_for_task_terminal(
    service: &RustDocsService,
    task_id: &str,
    timeout: Duration,
) -> Result<TaskTerminal> {
    let start = std::time::Instant::now();
    let poll_interval = Duration::from_millis(500);

    loop {
        if start.elapsed() > timeout {
            return Err(anyhow::anyhow!(
                "Timeout waiting for task {task_id} to reach a terminal state after {timeout:?}"
            ));
        }

        let params = CacheOperationsParams {
            task_id: Some(task_id.to_string()),
            status_filter: None,
            cancel: false,
            clear: false,
        };

        let response = service.cache_operations(Parameters(params)).await;

        if response.contains("COMPLETED ✓") {
            return Ok(TaskTerminal::Success);
        } else if response.contains("FAILED ✗") {
            return Ok(TaskTerminal::Failed(response));
        } else if response.contains("CANCELLED") {
            return Ok(TaskTerminal::Other(response));
        }

        tokio::time::sleep(poll_interval).await;
    }
}

// `#[tokio::test]` defaults to a `current_thread` runtime, so holding the
// sync `MutexGuard` across `.await` is safe — only one thread ever runs the
// future. We hold the guard for the whole test because `CARGO_HOME` is
// process-global and must not be mutated concurrently.
#[allow(clippy::await_holding_lock)]
#[tokio::test]
async fn test_cache_crate_reuses_cargo_registry_source() -> Result<()> {
    let _lock = cargo_home_lock().lock().unwrap();

    // Use a UUID-suffixed crate name so the HTTP fallback would 404 against
    // crates.io if the reuse path silently breaks. A literal name like
    // "fake-crate" could be squatted in the registry; a UUID cannot.
    let crate_name = format!("rust-docs-mcp-testonly-{}", uuid::Uuid::new_v4().simple());
    let crate_version = "0.0.1";

    // Pre-populate a fake cargo registry source layout:
    //   $CARGO_HOME/registry/src/index.crates.io-test/<crate>-<version>/
    let cargo_home_dir = TempDir::new()?;
    let cache_dir = TempDir::new()?;

    let cached_source = cargo_home_dir
        .path()
        .join("registry")
        .join("src")
        .join("index.crates.io-test")
        .join(format!("{crate_name}-{crate_version}"));
    std::fs::create_dir_all(cached_source.join("src"))?;
    std::fs::write(
        cached_source.join(CARGO_TOML),
        format!(
            "[package]\nname = \"{crate_name}\"\nversion = \"{crate_version}\"\nedition = \"2021\"\n"
        ),
    )?;
    std::fs::write(
        cached_source.join("src").join("lib.rs"),
        "pub fn answer() -> u32 { 42 }\n",
    )?;

    // Install the fake CARGO_HOME. The drop guard removes it on test exit so
    // a panic here doesn't leak state into other tests added to this binary.
    let _cargo_home_guard = set_cargo_home(cargo_home_dir.path());

    let service = RustDocsService::new(Some(cache_dir.path().to_path_buf()))?;

    let params = CacheCrateParams {
        crate_name: crate_name.clone(),
        source_type: "cratesio".to_string(),
        version: Some(crate_version.to_string()),
        github_url: None,
        branch: None,
        tag: None,
        path: None,
        members: None,
        update: None,
        features: None,
    };

    let response = service.cache_crate(Parameters(params)).await;
    let task_output = parse_cache_task_started(&response)?;
    assert_eq!(task_output.crate_name, crate_name);
    assert_eq!(task_output.version, crate_version);

    let terminal = wait_for_task_terminal(&service, &task_output.task_id, TEST_TIMEOUT).await?;

    // Assert the reuse metadata BEFORE asserting the task result. The
    // metadata is written at downloader.rs::try_copy_from_cargo_registry
    // *before* `generate_docs` runs nightly rustdoc — so even if rustdoc
    // fails on the minimal fake lib, the reuse evidence is on disk and is
    // what we actually want to verify.
    let storage = CacheStorage::new(Some(cache_dir.path().to_path_buf()))?;

    let metadata = storage
        .load_metadata(&crate_name, crate_version, None)
        .with_context(|| {
            format!(
                "metadata for {crate_name}-{crate_version} was not written — the reuse path did not run (terminal: {terminal:?})"
            )
        })?;

    assert_eq!(
        metadata.source, "crates.io",
        "reuse path should record source as crates.io"
    );
    let recorded_source_path = metadata
        .source_path
        .as_deref()
        .expect("reuse path should record source_path pointing into CARGO_HOME");
    assert!(
        Path::new(recorded_source_path).starts_with(cargo_home_dir.path()),
        "recorded source_path {recorded_source_path} should live inside the fake CARGO_HOME {}",
        cargo_home_dir.path().display()
    );

    let cached_cargo_toml = storage
        .source_path(&crate_name, crate_version)?
        .join(CARGO_TOML);
    assert!(
        cached_cargo_toml.is_file(),
        "cached source should contain a Cargo.toml"
    );
    let cargo_toml_contents = std::fs::read_to_string(&cached_cargo_toml)?;
    assert!(
        cargo_toml_contents.contains(crate_version),
        "cached Cargo.toml should be our fake manifest (contents: {cargo_toml_contents})"
    );

    // The task itself may report Success (reuse + rustdoc both worked) or
    // Failed (reuse worked but rustdoc failed on the minimal fake lib). Both
    // are acceptable — what we care about is that the reuse metadata is on
    // disk. A `Cancelled` terminal would indicate something odd, though.
    match terminal {
        TaskTerminal::Success | TaskTerminal::Failed(_) => Ok(()),
        TaskTerminal::Other(msg) => Err(anyhow::anyhow!(
            "unexpected terminal state (neither success nor failed): {msg}"
        )),
    }
}
