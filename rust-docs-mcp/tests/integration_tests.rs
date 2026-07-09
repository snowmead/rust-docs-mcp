//! Integration tests for rust-docs-mcp caching functionality
//!
//! These tests verify that caching works correctly for all three sources:
//! - crates.io
//! - GitHub
//! - Local paths

use anyhow::Result;
use rmcp::handler::server::wrapper::Parameters;
use rust_docs_mcp::RustDocsService;
use rust_docs_mcp::analysis::outputs::StructureOutput;
use rust_docs_mcp::analysis::tools::AnalyzeCrateStructureParams;
use rust_docs_mcp::cache::outputs::{
    CacheTaskStartedOutput, GetCratesMetadataOutput, ListCrateVersionsOutput,
};
use rust_docs_mcp::cache::tools::{
    CacheCrateParams, CacheOperationsParams, CrateMetadataQuery, GetCratesMetadataParams,
    ListCrateVersionsParams,
};
use rust_docs_mcp::deps::outputs::GetDependenciesOutput;
use rust_docs_mcp::deps::tools::GetDependenciesParams;
use rust_docs_mcp::docs::outputs::{
    GetItemDetailsOutput, GetItemDocsOutput, GetItemSourceOutput, ListCrateItemsOutput,
    SearchItemsOutput, SearchItemsPreviewOutput,
};
use rust_docs_mcp::docs::tools::{
    GetItemDetailsParams, GetItemDocsParams, GetItemSourceParams, ListItemsParams,
    SearchItemsParams, SearchItemsPreviewParams,
};
use rust_docs_mcp::search::outputs::SearchItemsFuzzyOutput;
use rust_docs_mcp::search::tools::SearchItemsFuzzyParams;
use std::time::Duration;
use tempfile::TempDir;

// Test constants
const TEST_TIMEOUT: Duration = Duration::from_secs(30);
const LARGE_CRATE_TEST_TIMEOUT: Duration = Duration::from_secs(120);
const HEAVY_NETWORK_TEST_TIMEOUT: Duration = Duration::from_secs(600);
const SEMVER_VERSION: &str = "1.0.0";
const SERDE_VERSION: &str = "v1.0.136";
const SERDE_GITHUB_URL: &str = "https://github.com/serde-rs/serde";
const CLIPPY_GITHUB_URL: &str = "https://github.com/rust-lang/rust-clippy";
const CLIPPY_BRANCH: &str = "master";

// Response validation helpers
fn parse_cache_task_started(response: &str) -> Result<CacheTaskStartedOutput> {
    serde_json::from_str(response).map_err(|e| {
        anyhow::anyhow!("Failed to parse task started response: {e}\nResponse: {response}")
    })
}

/// Helper to create a test service with temporary cache
fn create_test_service() -> Result<(RustDocsService, TempDir)> {
    let temp_dir = TempDir::new()?;
    let service = RustDocsService::new(Some(temp_dir.path().to_path_buf()))?;
    Ok((service, temp_dir))
}

/// Result of waiting for a caching task
#[derive(Debug)]
#[allow(dead_code)]
enum TaskResult {
    Success,
    WorkspaceDetected(String), // Contains workspace info
    BinaryOnly(String),        // Contains binary-only error
    Failed(String),            // Contains error message
    Cancelled,
}

/// Extract current step information from markdown output
/// Returns None if no step information is found
/// Returns (current_step, total_steps, description)
fn extract_step_from_response(response: &str) -> Option<(u8, u8, Option<String>)> {
    // Look for pattern: **Step**: X of Y
    // or: **Step**: X of Y: Description
    response
        .lines()
        .find(|line| line.contains("**Step**:"))
        .and_then(|line| {
            // Extract after "**Step**: "
            let step_part = line.split("**Step**:").nth(1)?.trim();

            // Parse "X of Y" or "X of Y: Description"
            let parts: Vec<&str> = step_part.splitn(2, " of ").collect();
            if parts.len() != 2 {
                return None;
            }

            let current: u8 = parts[0].trim().parse().ok()?;

            // Check if there's a description after total
            let remaining = parts[1];
            let (total_str, description) = if let Some(colon_pos) = remaining.find(':') {
                let (total, desc) = remaining.split_at(colon_pos);
                (total.trim(), Some(desc[1..].trim().to_string()))
            } else {
                (remaining.trim(), None)
            };

            let total: u8 = total_str.parse().ok()?;

            Some((current, total, description))
        })
}

/// Helper to wait for an async caching task to complete and return the final result.
/// This polls cache_operations every 500ms until the task reaches a terminal state.
async fn wait_for_task_completion(
    service: &RustDocsService,
    task_id: &str,
    timeout: Duration,
) -> Result<TaskResult> {
    let start = std::time::Instant::now();
    let poll_interval = Duration::from_millis(500);

    loop {
        if start.elapsed() > timeout {
            return Err(anyhow::anyhow!(
                "Timeout waiting for task {task_id} to complete after {timeout:?}"
            ));
        }

        // Check task status via cache_operations
        let params = CacheOperationsParams {
            task_id: Some(task_id.to_string()),
            status_filter: None,
            cancel: false,
            clear: false,
        };

        let response = service.cache_operations(Parameters(params)).await;

        // Parse the markdown response to check if task is complete
        if response.contains("COMPLETED ✓") {
            return Ok(TaskResult::Success);
        } else if response.contains("FAILED ✗") {
            // Check for specific failure types
            if response.contains("Workspace detected") || response.contains("specify member") {
                return Ok(TaskResult::WorkspaceDetected(response));
            } else if response.contains("binary-only") || response.contains("no library") {
                return Ok(TaskResult::BinaryOnly(response));
            } else {
                return Ok(TaskResult::Failed(response));
            }
        } else if response.contains("CANCELLED") {
            return Ok(TaskResult::Cancelled);
        }

        // Task still in progress, wait and retry
        tokio::time::sleep(poll_interval).await;
    }
}

/// Helper to setup and cache the semver test crate
async fn setup_test_crate(service: &RustDocsService) -> Result<()> {
    let params = CacheCrateParams {
        crate_name: "semver".to_string(),
        source_type: "cratesio".to_string(),
        version: Some(SEMVER_VERSION.to_string()),
        github_url: None,
        branch: None,
        tag: None,
        path: None,
        members: None,
        update: None,
        features: None,
    };

    // Start the async caching operation
    let response = service.cache_crate(Parameters(params)).await;

    // Parse the task started response
    let task_output = parse_cache_task_started(&response)?;

    // Wait for the task to complete
    let result = wait_for_task_completion(service, &task_output.task_id, TEST_TIMEOUT).await?;

    match result {
        TaskResult::Success => Ok(()),
        _ => Err(anyhow::anyhow!("Failed to cache test crate: {result:?}")),
    }
}

/// Helper to get a test item ID from the semver crate
async fn get_test_item_id(service: &RustDocsService) -> Result<i32> {
    let params = SearchItemsPreviewParams {
        crate_name: "semver".to_string(),
        version: SEMVER_VERSION.to_string(),
        pattern: "Version".to_string(),
        limit: Some(1),
        offset: None,
        kind_filter: Some("struct".to_string()),
        path_filter: None,
        member: None,
    };

    let response = service.search_items_preview(Parameters(params)).await;
    let output: SearchItemsPreviewOutput = serde_json::from_str(&response)?;

    if let Some(item) = output.items.first() {
        return Ok(item.id.parse::<i32>()?);
    }

    Err(anyhow::anyhow!("Could not find test item ID in response"))
}

#[tokio::test]
#[ignore = "requires network access"]
async fn test_cache_from_crates_io() -> Result<()> {
    let (service, _temp_dir) = create_test_service()?;

    // Cache a small, stable crate from crates.io
    let params = CacheCrateParams {
        crate_name: "semver".to_string(),
        source_type: "cratesio".to_string(),
        version: Some(SEMVER_VERSION.to_string()),
        github_url: None,
        branch: None,
        tag: None,
        path: None,
        members: None,
        update: None,
        features: None,
    };

    // Start async caching operation
    let response = service.cache_crate(Parameters(params)).await;
    let task_output = parse_cache_task_started(&response)?;

    assert_eq!(task_output.crate_name, "semver");
    assert_eq!(task_output.version, SEMVER_VERSION);

    // Wait for task to complete
    let result = wait_for_task_completion(&service, &task_output.task_id, TEST_TIMEOUT).await?;
    assert!(
        matches!(result, TaskResult::Success),
        "Expected success, got: {result:?}"
    );

    // Verify it's in the cache by listing versions
    let list_params = ListCrateVersionsParams {
        crate_name: "semver".to_string(),
    };

    let versions_response = service.list_crate_versions(Parameters(list_params)).await;
    let versions_output: ListCrateVersionsOutput = serde_json::from_str(&versions_response)?;
    assert_eq!(versions_output.crate_name, "semver");
    assert!(
        versions_output
            .versions
            .iter()
            .any(|v| v.version == SEMVER_VERSION),
        "Version not found in cache: {versions_output:?}"
    );

    Ok(())
}

#[tokio::test]
#[ignore = "requires network access"]
async fn test_cache_from_github() -> Result<()> {
    let (service, _temp_dir) = create_test_service()?;

    // Cache a crate from GitHub using a tag
    let params = CacheCrateParams {
        crate_name: "serde-test".to_string(),
        source_type: "github".to_string(),
        version: None,
        github_url: Some(SERDE_GITHUB_URL.to_string()),
        branch: None,
        tag: Some(SERDE_VERSION.to_string()),
        path: None,
        members: None,
        update: None,
        features: None,
    };

    let response = service.cache_crate(Parameters(params)).await;

    // Parse the async task response
    let task_output = parse_cache_task_started(&response)?;

    // Wait for completion - serde is a workspace so we expect workspace detection failure
    let result = wait_for_task_completion(&service, &task_output.task_id, TEST_TIMEOUT).await?;
    assert!(
        matches!(result, TaskResult::WorkspaceDetected(_)),
        "Expected workspace detection for serde, got: {result:?}"
    );

    // Verify cached (workspace metadata should be cached)
    let list_params = ListCrateVersionsParams {
        crate_name: "serde-test".to_string(),
    };

    let versions_response = service.list_crate_versions(Parameters(list_params)).await;
    let versions_output: ListCrateVersionsOutput = serde_json::from_str(&versions_response)?;
    assert!(
        versions_output
            .versions
            .iter()
            .any(|v| v.version == SERDE_VERSION),
        "Version not found: {versions_output:?}"
    );

    Ok(())
}

#[tokio::test]
#[ignore = "requires network access"]
async fn test_cache_from_github_branch() -> Result<()> {
    // Initialize tracing for this test
    let _ = tracing_subscriber::fmt()
        .with_env_filter("rust_docs_mcp=debug")
        .try_init();

    let (service, _temp_dir) = create_test_service()?;

    // Cache from GitHub using a branch
    let params = CacheCrateParams {
        crate_name: "clippy-test".to_string(),
        source_type: "github".to_string(),
        version: None,
        github_url: Some(CLIPPY_GITHUB_URL.to_string()),
        branch: Some(CLIPPY_BRANCH.to_string()),
        tag: None,
        path: None,
        members: None,
        update: None,
        features: None,
    };

    let response = service.cache_crate(Parameters(params)).await;

    // Print the response for debugging
    println!("Response: {response}");

    // Parse async task response
    let task_output = parse_cache_task_started(&response)?;

    // Wait for task completion - clippy is binary-only so should fail
    let result =
        wait_for_task_completion(&service, &task_output.task_id, LARGE_CRATE_TEST_TIMEOUT).await?;

    // Clippy is a binary-only package, so we should expect a binary-only error
    assert!(
        matches!(result, TaskResult::BinaryOnly(_)),
        "Expected binary-only package error, got: {result:?}"
    );

    Ok(())
}

#[tokio::test]
#[ignore = "requires network access"]
async fn test_cache_from_local_path() -> Result<()> {
    let (service, _temp_dir) = create_test_service()?;

    // Create a test crate in a temporary directory
    let test_crate_dir = TempDir::new()?;
    let cargo_toml_path = test_crate_dir.path().join("Cargo.toml");
    std::fs::write(
        &cargo_toml_path,
        r#"
[package]
name = "test-local"
version = "0.1.0"
edition = "2021"

[dependencies]
    "#,
    )?;

    // Create a minimal lib.rs
    let src_dir = test_crate_dir.path().join("src");
    std::fs::create_dir(&src_dir)?;
    std::fs::write(
        src_dir.join("lib.rs"),
        "//! Test local crate\npub fn test() {}",
    )?;

    // Cache from local path
    let params = CacheCrateParams {
        crate_name: "test-local".to_string(),
        source_type: "local".to_string(),
        version: Some("0.1.0".to_string()),
        github_url: None,
        branch: None,
        tag: None,
        path: Some(test_crate_dir.path().to_str().unwrap().to_string()),
        members: None,
        update: None,
        features: None,
    };

    let response = service.cache_crate(Parameters(params)).await;
    let task_output = parse_cache_task_started(&response)?;

    // Wait for task completion
    let result = wait_for_task_completion(&service, &task_output.task_id, TEST_TIMEOUT).await?;
    assert!(
        matches!(result, TaskResult::Success),
        "Failed to cache from local path: {result:?}"
    );

    // Verify cached
    let list_params = ListCrateVersionsParams {
        crate_name: "test-local".to_string(),
    };

    let versions_response = service.list_crate_versions(Parameters(list_params)).await;
    assert!(
        versions_response.contains("0.1.0"),
        "Version not found: {versions_response}"
    );

    Ok(())
}

#[tokio::test]
async fn test_workspace_crate_detection() -> Result<()> {
    let (service, _temp_dir) = create_test_service()?;

    // Create a workspace crate
    let workspace_dir = TempDir::new()?;
    let workspace_toml = workspace_dir.path().join("Cargo.toml");
    std::fs::write(
        &workspace_toml,
        r#"
[workspace]
members = ["crate-a", "crate-b"]
resolver = "2"

[workspace.dependencies]
serde = "1.0"
    "#,
    )?;

    // Create member crates
    for member in &["crate-a", "crate-b"] {
        let member_dir = workspace_dir.path().join(member);
        std::fs::create_dir_all(&member_dir)?;
        std::fs::write(
            member_dir.join("Cargo.toml"),
            format!(
                r#"
[package]
name = "{member}"
version = "0.1.0"
edition = "2021"

[dependencies]
serde = {{ workspace = true }}
        "#
            ),
        )?;

        let src_dir = member_dir.join("src");
        std::fs::create_dir(&src_dir)?;
        std::fs::write(src_dir.join("lib.rs"), format!("//! {member} crate"))?;
    }

    // Cache the workspace - should detect it's a workspace
    let params = CacheCrateParams {
        crate_name: "test-workspace".to_string(),
        source_type: "local".to_string(),
        version: Some("0.1.0".to_string()),
        github_url: None,
        branch: None,
        tag: None,
        path: Some(workspace_dir.path().to_str().unwrap().to_string()),
        members: None, // Should detect workspace and return member list
        update: None,
        features: None,
    };

    let response = service.cache_crate(Parameters(params)).await;

    // Parse async task response
    let task_output = parse_cache_task_started(&response)?;

    // Wait for completion - should fail with workspace detection
    let result = wait_for_task_completion(&service, &task_output.task_id, TEST_TIMEOUT).await?;

    // Response should indicate workspace detection
    assert!(
        matches!(result, TaskResult::WorkspaceDetected(_)),
        "Response should indicate workspace detection: {result:?}"
    );

    // Verify the response mentions workspace detection
    if let TaskResult::WorkspaceDetected(msg) = result {
        assert!(
            msg.contains("Workspace detected"),
            "Should mention workspace: {msg}"
        );
    }

    Ok(())
}

#[tokio::test]
#[ignore = "requires network access"]
async fn test_cache_update() -> Result<()> {
    let (service, _temp_dir) = create_test_service()?;

    // Cache initially
    let params1 = CacheCrateParams {
        crate_name: "once_cell".to_string(),
        source_type: "cratesio".to_string(),
        version: Some("1.17.0".to_string()),
        github_url: None,
        branch: None,
        tag: None,
        path: None,
        members: None,
        update: None,
        features: None,
    };

    let response1 = service.cache_crate(Parameters(params1)).await;
    let task1 = parse_cache_task_started(&response1)?;
    let result1 = wait_for_task_completion(&service, &task1.task_id, TEST_TIMEOUT).await?;
    assert!(
        matches!(result1, TaskResult::Success),
        "Initial cache failed: {result1:?}"
    );

    // Cache again with update flag
    let params2 = CacheCrateParams {
        crate_name: "once_cell".to_string(),
        source_type: "cratesio".to_string(),
        version: Some("1.17.0".to_string()),
        github_url: None,
        branch: None,
        tag: None,
        path: None,
        members: None,
        update: Some(true),
        features: None,
    };

    let response2 = service.cache_crate(Parameters(params2)).await;
    let task2 = parse_cache_task_started(&response2)?;
    let result2 = wait_for_task_completion(&service, &task2.task_id, TEST_TIMEOUT).await?;
    assert!(
        matches!(result2, TaskResult::Success),
        "Update cache failed: {result2:?}"
    );

    Ok(())
}

#[tokio::test]
async fn test_invalid_inputs() -> Result<()> {
    let (service, _temp_dir) = create_test_service()?;

    // Test non-existent crate from crates.io
    let params = CacheCrateParams {
        crate_name: "this-crate-definitely-does-not-exist-123456".to_string(),
        source_type: "cratesio".to_string(),
        version: Some("1.0.0".to_string()),
        github_url: None,
        branch: None,
        tag: None,
        path: None,
        members: None,
        update: None,
        features: None,
    };

    let response = service.cache_crate(Parameters(params)).await;

    // crates.io returns 403 Forbidden for non-existent crates - this will be async
    let task = parse_cache_task_started(&response)?;
    let result = wait_for_task_completion(&service, &task.task_id, TEST_TIMEOUT).await?;
    assert!(
        matches!(result, TaskResult::Failed(_)),
        "Expected error response, got: {result:?}"
    );

    // Test invalid GitHub URL - this might fail synchronously or asynchronously
    let params = CacheCrateParams {
        crate_name: "invalid".to_string(),
        source_type: "github".to_string(),
        version: None,
        github_url: Some("not-a-valid-url".to_string()),
        branch: None,
        tag: Some("v1.0.0".to_string()),
        path: None,
        members: None,
        update: None,
        features: None,
    };

    let response = service.cache_crate(Parameters(params)).await;
    // Try parsing as error first, then as async task
    if response.contains("# Error") {
        // Synchronous error
        assert!(
            response.contains("Error"),
            "Expected error in response: {response}"
        );
    } else {
        // Async task
        let task = parse_cache_task_started(&response)?;
        let result = wait_for_task_completion(&service, &task.task_id, TEST_TIMEOUT).await?;
        assert!(
            matches!(result, TaskResult::Failed(_)),
            "Expected error response, got: {result:?}"
        );
    }

    // Test non-existent local path - this will fail synchronously
    let params = CacheCrateParams {
        crate_name: "invalid".to_string(),
        source_type: "local".to_string(),
        version: Some("1.0.0".to_string()),
        github_url: None,
        branch: None,
        tag: None,
        path: Some("/this/path/does/not/exist".to_string()),
        members: None,
        update: None,
        features: None,
    };

    let response = service.cache_crate(Parameters(params)).await;
    // Local path validation happens synchronously before spawning
    assert!(
        response.contains("Error") || response.contains("does not exist"),
        "Expected error for non-existent path: {response}"
    );

    Ok(())
}

#[tokio::test]
#[ignore = "requires network access"]
async fn test_concurrent_caching() -> Result<()> {
    let (service, _temp_dir) = create_test_service()?;
    let service = std::sync::Arc::new(service);

    // Define test crates with expected metadata
    let test_crates = vec![
        ("semver", "1.0.0"),
        ("once_cell", "1.0.0"),
        ("regex", "1.11.1"), // Use the latest version compatible with nightly toolchain
    ];

    // Start caching multiple crates concurrently
    let mut task_ids = vec![];

    for (name, version) in &test_crates {
        let params = CacheCrateParams {
            crate_name: name.to_string(),
            source_type: "cratesio".to_string(),
            version: Some(version.to_string()),
            github_url: None,
            branch: None,
            tag: None,
            path: None,
            members: None,
            update: None,
            features: None,
        };
        let start = std::time::Instant::now();
        let response = service.cache_crate(Parameters(params)).await;
        let duration = start.elapsed();
        println!("Started caching {name} {version} in {duration:?}");

        let task = parse_cache_task_started(&response)?;
        task_ids.push((name.to_string(), version.to_string(), task.task_id));
    }

    // Wait for all tasks to complete
    let mut results = vec![];
    for (name, version, task_id) in task_ids {
        let result = wait_for_task_completion(&service, &task_id, TEST_TIMEOUT).await?;
        println!("Completed caching {name} {version}");

        if !matches!(result, TaskResult::Success) {
            eprintln!("Concurrent cache failed for {name}: {result:?}");
        }
        results.push((name.clone(), version.clone(), result));
    }

    // Verify all operations succeeded
    for (name, version, result) in &results {
        assert!(
            matches!(result, TaskResult::Success),
            "Failed to cache {name} {version}: {result:?}"
        );
    }

    // Verify cache consistency - all crates should be present
    let cached_crates_response = service.list_cached_crates().await;
    for (name, version) in &test_crates {
        assert!(
            cached_crates_response.contains(name),
            "{name} not found in cache listing"
        );

        // Also verify specific version is cached
        let list_params = ListCrateVersionsParams {
            crate_name: name.to_string(),
        };
        let versions_response = service.list_crate_versions(Parameters(list_params)).await;
        assert!(
            versions_response.contains(version),
            "Version {version} of {name} not found in cache"
        );
    }

    // Verify no corruption by attempting to re-cache (should be idempotent)
    // This would fail if the cache was corrupted during concurrent access
    for (name, version) in &test_crates {
        let params = CacheCrateParams {
            crate_name: name.to_string(),
            source_type: "cratesio".to_string(),
            version: Some(version.to_string()),
            github_url: None,
            branch: None,
            tag: None,
            path: None,
            members: None,
            update: Some(false), // Should not re-download if already cached
            features: None,
        };
        let response = service.cache_crate(Parameters(params)).await;
        let task = parse_cache_task_started(&response)?;
        let result = wait_for_task_completion(&service, &task.task_id, TEST_TIMEOUT).await?;

        // Should complete successfully even if already cached
        assert!(
            matches!(result, TaskResult::Success),
            "Cache integrity check failed for {name} {version}: {result:?}"
        );
    }

    Ok(())
}

#[tokio::test]
async fn test_workspace_member_caching() -> Result<()> {
    let (service, _temp_dir) = create_test_service()?;

    // Create a workspace with members
    let workspace_dir = TempDir::new()?;
    let workspace_toml = workspace_dir.path().join("Cargo.toml");
    std::fs::write(
        &workspace_toml,
        r#"
[workspace]
members = ["lib-a", "lib-b"]
resolver = "2"
    "#,
    )?;

    // Create member crates
    for (member, version) in &[("lib-a", "0.1.0"), ("lib-b", "0.2.0")] {
        let member_dir = workspace_dir.path().join(member);
        std::fs::create_dir_all(&member_dir)?;
        std::fs::write(
            member_dir.join("Cargo.toml"),
            format!(
                r#"
[package]
name = "{member}"
version = "{version}"
edition = "2021"
        "#
            ),
        )?;

        let src_dir = member_dir.join("src");
        std::fs::create_dir(&src_dir)?;
        std::fs::write(src_dir.join("lib.rs"), format!("//! {member} library"))?;
    }

    // First attempt without specifying members - should get workspace detection
    let params1 = CacheCrateParams {
        crate_name: "my-workspace".to_string(),
        source_type: "local".to_string(),
        version: Some("1.0.0".to_string()),
        github_url: None,
        branch: None,
        tag: None,
        path: Some(workspace_dir.path().to_str().unwrap().to_string()),
        members: None,
        update: None,
        features: None,
    };

    let response1 = service.cache_crate(Parameters(params1)).await;
    let task1 = parse_cache_task_started(&response1)?;
    let result1 = wait_for_task_completion(&service, &task1.task_id, TEST_TIMEOUT).await?;

    // Should detect workspace
    assert!(
        matches!(result1, TaskResult::WorkspaceDetected(_)),
        "Should detect workspace: {result1:?}"
    );

    if let TaskResult::WorkspaceDetected(msg) = result1 {
        assert!(
            msg.contains("Workspace detected"),
            "Should mention workspace: {msg}"
        );
    }

    // Now cache with specific members
    let params2 = CacheCrateParams {
        crate_name: "my-workspace".to_string(),
        source_type: "local".to_string(),
        version: Some("1.0.0".to_string()),
        github_url: None,
        branch: None,
        tag: None,
        path: Some(workspace_dir.path().to_str().unwrap().to_string()),
        members: Some(vec!["lib-a".to_string(), "lib-b".to_string()]),
        update: None,
        features: None,
    };

    let response2 = service.cache_crate(Parameters(params2)).await;
    let task2 = parse_cache_task_started(&response2)?;
    let result2 = wait_for_task_completion(&service, &task2.task_id, TEST_TIMEOUT).await?;

    assert!(
        matches!(result2, TaskResult::Success),
        "Should successfully cache workspace members: {result2:?}"
    );

    Ok(())
}

// ===== DOCUMENTATION TOOLS TESTS =====

#[tokio::test]
#[ignore = "requires network access"]
async fn test_list_crate_items() -> Result<()> {
    let (service, _temp_dir) = create_test_service()?;
    setup_test_crate(&service).await?;

    // Test basic listing
    let params = ListItemsParams {
        crate_name: "semver".to_string(),
        version: SEMVER_VERSION.to_string(),
        kind_filter: None,
        limit: Some(50),
        offset: Some(0),
        member: None,
    };

    let response = service.list_crate_items(Parameters(params)).await;
    let output: ListCrateItemsOutput = serde_json::from_str(&response)?;

    assert!(!output.items.is_empty(), "Should have items");
    assert_eq!(output.pagination.limit, 50, "Limit should match request");
    assert_eq!(output.pagination.offset, 0, "Offset should match request");

    // Test with kind filter
    let params = ListItemsParams {
        crate_name: "semver".to_string(),
        version: SEMVER_VERSION.to_string(),
        kind_filter: Some("struct".to_string()),
        limit: Some(10),
        offset: None,
        member: None,
    };

    let response = service.list_crate_items(Parameters(params)).await;
    let output: ListCrateItemsOutput = serde_json::from_str(&response)?;

    // Check all items are structs
    for item in &output.items {
        assert_eq!(item.kind, "struct", "All items should be structs");
    }

    Ok(())
}

#[tokio::test]
#[ignore = "requires network access"]
async fn test_search_items_preview() -> Result<()> {
    let (service, _temp_dir) = create_test_service()?;
    setup_test_crate(&service).await?;

    // Test basic preview search
    let params = SearchItemsPreviewParams {
        crate_name: "semver".to_string(),
        version: SEMVER_VERSION.to_string(),
        pattern: "Version".to_string(),
        limit: Some(10),
        offset: None,
        kind_filter: None,
        path_filter: None,
        member: None,
    };

    let response = service.search_items_preview(Parameters(params)).await;
    let output: SearchItemsPreviewOutput = serde_json::from_str(&response)?;

    assert!(
        !output.items.is_empty(),
        "Should find items matching 'Version'"
    );

    // Verify preview format (only id, name, kind, path)
    if let Some(item) = output.items.first() {
        assert!(!item.id.is_empty(), "Item should have id");
        assert!(!item.name.is_empty(), "Item should have name");
        assert!(!item.kind.is_empty(), "Item should have kind");
        assert!(!item.path.is_empty(), "Item should have path");
    }

    // Test with filters
    let params = SearchItemsPreviewParams {
        crate_name: "semver".to_string(),
        version: SEMVER_VERSION.to_string(),
        pattern: "new".to_string(),
        limit: Some(5),
        offset: None,
        kind_filter: Some("function".to_string()),
        path_filter: None,
        member: None,
    };

    let response = service.search_items_preview(Parameters(params)).await;
    let output: SearchItemsPreviewOutput = serde_json::from_str(&response)?;

    // Check all items are functions
    for item in &output.items {
        assert_eq!(item.kind, "function", "All items should be functions");
    }

    Ok(())
}

#[tokio::test]
#[ignore = "requires network access"]
async fn test_search_items_full() -> Result<()> {
    let (service, _temp_dir) = create_test_service()?;
    setup_test_crate(&service).await?;

    // Test full search with complete documentation
    let params = SearchItemsParams {
        crate_name: "semver".to_string(),
        version: SEMVER_VERSION.to_string(),
        pattern: "Version".to_string(),
        limit: Some(5),
        offset: None,
        kind_filter: Some("struct".to_string()),
        path_filter: None,
        member: None,
    };

    let response = service.search_items(Parameters(params)).await;
    let output: SearchItemsOutput = serde_json::from_str(&response)?;

    assert!(!output.items.is_empty(), "Should find items");

    // Verify full format includes documentation
    if let Some(item) = output.items.first() {
        assert!(!item.id.is_empty(), "Item should have id");
        assert!(!item.name.is_empty(), "Item should have name");
        assert_eq!(item.kind, "struct", "Item should be a struct");
        // Full search may include documentation
        // Note: docs field is Optional, so it's OK if it's None
    }

    Ok(())
}

#[tokio::test]
#[ignore = "requires network access"]
async fn test_get_item_details() -> Result<()> {
    let (service, _temp_dir) = create_test_service()?;
    setup_test_crate(&service).await?;

    let item_id = get_test_item_id(&service).await?;

    // Test getting complete item details
    let params = GetItemDetailsParams {
        crate_name: "semver".to_string(),
        version: SEMVER_VERSION.to_string(),
        item_id,
        member: None,
    };

    let response = service.get_item_details(Parameters(params)).await;
    let output: GetItemDetailsOutput = serde_json::from_str(&response)?;

    assert!(output.is_success(), "Should be a success response");

    if let GetItemDetailsOutput::Success(detailed_item) = output {
        // Should contain detailed information about the item
        assert!(!detailed_item.info.id.is_empty(), "Details should have id");
        assert!(
            !detailed_item.info.name.is_empty(),
            "Details should have name"
        );
        assert!(
            !detailed_item.info.kind.is_empty(),
            "Details should have kind"
        );
    }

    Ok(())
}

#[tokio::test]
#[ignore = "requires network access"]
async fn test_get_item_docs_and_source() -> Result<()> {
    let (service, _temp_dir) = create_test_service()?;
    setup_test_crate(&service).await?;

    let item_id = get_test_item_id(&service).await?;

    // Test getting just documentation
    let docs_params = GetItemDocsParams {
        crate_name: "semver".to_string(),
        version: SEMVER_VERSION.to_string(),
        item_id,
        member: None,
    };

    let docs_response = service.get_item_docs(Parameters(docs_params)).await;
    let docs_output: GetItemDocsOutput = serde_json::from_str(&docs_response)?;

    // Documentation is optional - the item may not have docs
    // If it has documentation, it should be in the documentation field
    if let Some(doc) = docs_output.documentation {
        assert!(
            !doc.is_empty(),
            "If documentation exists, it should not be empty"
        );
    }

    // Test getting source code
    let source_params = GetItemSourceParams {
        crate_name: "semver".to_string(),
        version: SEMVER_VERSION.to_string(),
        item_id,
        context_lines: Some(5),
        member: None,
    };

    let source_response = service.get_item_source(Parameters(source_params)).await;
    let source_output: GetItemSourceOutput = serde_json::from_str(&source_response)?;

    assert!(source_output.is_success(), "Should be a success response");

    if let GetItemSourceOutput::Success(source_info) = source_output {
        assert!(!source_info.code.is_empty(), "Should contain source code");
        assert!(
            !source_info.location.filename.is_empty(),
            "Should have filename"
        );
        assert_eq!(
            source_info.context_lines,
            Some(5),
            "Context lines should match request"
        );
    }

    Ok(())
}

// ===== SEARCH TOOLS TESTS =====

#[tokio::test]
#[ignore = "requires network access"]
async fn test_search_items_fuzzy() -> Result<()> {
    let (service, _temp_dir) = create_test_service()?;
    setup_test_crate(&service).await?;

    // Test fuzzy search with typos
    let params = SearchItemsFuzzyParams {
        crate_name: "semver".to_string(),
        version: SEMVER_VERSION.to_string(),
        query: "Versoin".to_string(), // Typo in "Version"
        fuzzy_enabled: Some(true),
        fuzzy_distance: Some(1),
        limit: Some(10),
        kind_filter: None,
        member: None,
    };

    let response = service.search_items_fuzzy(Parameters(params)).await;
    let output: SearchItemsFuzzyOutput = serde_json::from_str(&response)?;

    assert!(output.fuzzy_enabled, "Fuzzy should be enabled");
    assert_eq!(output.query, "Versoin", "Query should match request");
    assert_eq!(output.crate_name, "semver", "Crate name should match");
    assert_eq!(output.version, SEMVER_VERSION, "Version should match");

    // Test exact search (fuzzy disabled)
    let params = SearchItemsFuzzyParams {
        crate_name: "semver".to_string(),
        version: SEMVER_VERSION.to_string(),
        query: "Version".to_string(),
        fuzzy_enabled: Some(false),
        fuzzy_distance: Some(0),
        limit: Some(5),
        kind_filter: Some("struct".to_string()),
        member: None,
    };

    let response = service.search_items_fuzzy(Parameters(params)).await;
    let output: SearchItemsFuzzyOutput = serde_json::from_str(&response)?;

    assert!(!output.fuzzy_enabled, "Fuzzy should be disabled");

    // Check all results are structs if any results were found
    for result in &output.results {
        assert_eq!(result.kind, "struct", "All results should be structs");
    }

    Ok(())
}

// ===== ANALYSIS TOOLS TESTS =====

#[tokio::test]
#[ignore = "requires network access"]
async fn test_structure() -> Result<()> {
    let (service, _temp_dir) = create_test_service()?;
    setup_test_crate(&service).await?;

    // Test basic structure analysis
    let params = AnalyzeCrateStructureParams {
        crate_name: "semver".to_string(),
        version: SEMVER_VERSION.to_string(),
        member: None,
        lib: Some(true),
        bin: None,
        no_default_features: None,
        all_features: None,
        features: None,
        target: None,
        cfg_test: None,
        no_fns: None,
        no_traits: None,
        no_types: None,
        sort_by: None,
        sort_reversed: None,
        focus_on: None,
        max_depth: Some(3),
    };

    let response = service.structure(Parameters(params)).await;
    let output: StructureOutput = serde_json::from_str(&response)?;

    assert!(output.is_success(), "Structure analysis should succeed");
    assert_eq!(output.status, "success", "Status should be success");
    assert!(!output.message.is_empty(), "Should have a message");
    assert!(!output.tree.name.is_empty(), "Tree should have a name");
    assert!(!output.tree.kind.is_empty(), "Tree should have a kind");

    // Test with filtering options
    let params = AnalyzeCrateStructureParams {
        crate_name: "semver".to_string(),
        version: SEMVER_VERSION.to_string(),
        member: None,
        lib: Some(true),
        bin: None,
        no_default_features: None,
        all_features: None,
        features: None,
        target: None,
        cfg_test: None,
        no_fns: Some(true), // Filter out functions
        no_traits: None,
        no_types: None,
        sort_by: Some("name".to_string()),
        sort_reversed: None,
        focus_on: None,
        max_depth: Some(2),
    };

    let response = service.structure(Parameters(params)).await;
    let output: StructureOutput = serde_json::from_str(&response)?;

    assert!(
        output.is_success(),
        "Filtered structure analysis should succeed"
    );

    Ok(())
}

// ===== DEPENDENCY TOOLS TESTS =====

#[tokio::test]
#[ignore = "requires network access"]
async fn test_get_dependencies() -> Result<()> {
    let (service, _temp_dir) = create_test_service()?;
    setup_test_crate(&service).await?;

    // Test direct dependencies
    let params = GetDependenciesParams {
        crate_name: "semver".to_string(),
        version: SEMVER_VERSION.to_string(),
        include_tree: Some(false),
        filter: None,
        member: None,
    };

    let response = service.get_dependencies(Parameters(params)).await;
    let output: GetDependenciesOutput = serde_json::from_str(&response)?;

    assert_eq!(output.crate_info.name, "semver", "Crate name should match");
    assert_eq!(
        output.crate_info.version, SEMVER_VERSION,
        "Version should match"
    );
    // Direct dependencies is a list, could be empty
    // No need to check >= 0 as len() returns usize which is always >= 0

    // Test full dependency tree
    let params = GetDependenciesParams {
        crate_name: "semver".to_string(),
        version: SEMVER_VERSION.to_string(),
        include_tree: Some(true),
        filter: None,
        member: None,
    };

    let response = service.get_dependencies(Parameters(params)).await;
    let output: GetDependenciesOutput = serde_json::from_str(&response)?;

    // When include_tree is true, dependency_tree should be populated
    assert!(
        output.dependency_tree.is_some(),
        "Should include dependency tree when requested"
    );

    // Test with filter
    let params = GetDependenciesParams {
        crate_name: "semver".to_string(),
        version: SEMVER_VERSION.to_string(),
        include_tree: Some(false),
        filter: Some("serde".to_string()),
        member: None,
    };

    let response = service.get_dependencies(Parameters(params)).await;
    let output: GetDependenciesOutput = serde_json::from_str(&response)?;

    // Filter might return empty results, but that's OK
    // Just verify the response is valid
    assert_eq!(
        output.crate_info.name, "semver",
        "Crate name should match even with filter"
    );

    Ok(())
}

// ===== METADATA TOOLS TESTS =====

#[tokio::test]
#[ignore = "requires network access"]
async fn test_get_crates_metadata() -> Result<()> {
    let (service, _temp_dir) = create_test_service()?;
    setup_test_crate(&service).await?;

    // Test batch metadata query
    let params = GetCratesMetadataParams {
        queries: vec![
            CrateMetadataQuery {
                crate_name: "semver".to_string(),
                version: SEMVER_VERSION.to_string(),
                members: None,
            },
            CrateMetadataQuery {
                crate_name: "nonexistent-crate".to_string(),
                version: "1.0.0".to_string(),
                members: None,
            },
        ],
    };

    let response = service.get_crates_metadata(Parameters(params)).await;
    let output: GetCratesMetadataOutput = serde_json::from_str(&response)?;

    assert_eq!(output.total_queried, 2, "Should query 2 crates");
    assert_eq!(output.metadata.len(), 2, "Should have 2 metadata entries");

    // First query should show semver as cached
    assert_eq!(output.metadata[0].crate_name, "semver");
    assert!(output.metadata[0].cached);

    // Second query should show nonexistent crate as not cached
    assert_eq!(output.metadata[1].crate_name, "nonexistent-crate");
    assert!(!output.metadata[1].cached);

    Ok(())
}

// ===== EDGE CASES TESTS =====

#[tokio::test]
#[ignore = "requires network access"]
async fn test_invalid_item_ids() -> Result<()> {
    let (service, _temp_dir) = create_test_service()?;
    setup_test_crate(&service).await?;

    // Test with invalid item ID
    let params = GetItemDetailsParams {
        crate_name: "semver".to_string(),
        version: SEMVER_VERSION.to_string(),
        item_id: 999999, // Invalid ID
        member: None,
    };

    let response = service.get_item_details(Parameters(params)).await;
    assert!(
        response.contains("error") || response.contains("not found"),
        "Should return error for invalid ID: {response}"
    );

    // Test docs with invalid ID
    let params = GetItemDocsParams {
        crate_name: "semver".to_string(),
        version: SEMVER_VERSION.to_string(),
        item_id: 999999,
        member: None,
    };

    let response = service.get_item_docs(Parameters(params)).await;
    assert!(
        response.contains("error") || response.contains("not found"),
        "Should return error for invalid docs ID: {response}"
    );

    // Test source with invalid ID
    let params = GetItemSourceParams {
        crate_name: "semver".to_string(),
        version: SEMVER_VERSION.to_string(),
        item_id: 999999,
        context_lines: Some(3),
        member: None,
    };

    let response = service.get_item_source(Parameters(params)).await;
    assert!(
        response.contains("error") || response.contains("not found"),
        "Should return error for invalid source ID: {response}"
    );

    Ok(())
}

#[tokio::test]
#[ignore = "requires network access"]
async fn test_empty_search_results() -> Result<()> {
    let (service, _temp_dir) = create_test_service()?;
    setup_test_crate(&service).await?;

    // Test search with pattern that should return no results
    let params = SearchItemsPreviewParams {
        crate_name: "semver".to_string(),
        version: SEMVER_VERSION.to_string(),
        pattern: "ThisPatternShouldNotExistAnywhere123".to_string(),
        limit: Some(10),
        offset: None,
        kind_filter: None,
        path_filter: None,
        member: None,
    };

    let response = service.search_items_preview(Parameters(params)).await;
    let output: SearchItemsPreviewOutput = serde_json::from_str(&response)?;

    assert!(
        output.items.is_empty(),
        "Should return empty results for non-existent pattern"
    );
    assert_eq!(
        output.pagination.total, 0,
        "Total should be 0 for no results"
    );

    // Test fuzzy search with no results
    let params = SearchItemsFuzzyParams {
        crate_name: "semver".to_string(),
        version: SEMVER_VERSION.to_string(),
        query: "XyZabc123NonExistent".to_string(),
        fuzzy_enabled: Some(true),
        fuzzy_distance: Some(1),
        limit: Some(10),
        kind_filter: None,
        member: None,
    };

    let response = service.search_items_fuzzy(Parameters(params)).await;
    let output: SearchItemsFuzzyOutput = serde_json::from_str(&response)?;

    // Fuzzy search might return some results even for non-existent patterns, but should be valid
    assert_eq!(
        output.query, "XyZabc123NonExistent",
        "Query should match request"
    );
    assert!(output.fuzzy_enabled, "Fuzzy should be enabled");
    // Results could be empty or have some fuzzy matches
    // No need to check >= 0 as total_results is u64 which is always >= 0

    Ok(())
}

// ===== FEATURE FALLBACK TESTS =====

/// Tests the feature fallback strategy (--all-features → default → --no-default-features)
/// using a local crate that intentionally fails to compile with --all-features.
/// This test requires no network access and works on all platforms.
#[tokio::test]
async fn test_feature_fallback_with_broken_feature() -> Result<()> {
    let _ = tracing_subscriber::fmt()
        .with_env_filter("rust_docs_mcp=debug")
        .try_init();

    let (service, _temp_dir) = create_test_service()?;

    // Create a local crate whose `broken` feature triggers a compile_error.
    // --all-features will fail; default features (which exclude `broken`) will succeed.
    let crate_dir = TempDir::new()?;
    std::fs::write(
        crate_dir.path().join("Cargo.toml"),
        r#"[package]
name = "feature-fallback-fixture"
version = "0.1.0"
edition = "2021"

[features]
broken = []
"#,
    )?;
    let src_dir = crate_dir.path().join("src");
    std::fs::create_dir(&src_dir)?;
    std::fs::write(
        src_dir.join("lib.rs"),
        r#"//! Feature fallback fixture crate.
//! The `broken` feature intentionally fails to compile to exercise the
//! feature fallback strategy in rustdoc generation.

#[cfg(feature = "broken")]
compile_error!("The `broken` feature is intentionally uncompilable (fallback test fixture)");

/// A function that always works.
pub fn always_works() -> &'static str {
    "ok"
}
"#,
    )?;

    let params = CacheCrateParams {
        crate_name: "feature-fallback-fixture".to_string(),
        source_type: "local".to_string(),
        version: Some("0.1.0".to_string()),
        github_url: None,
        branch: None,
        tag: None,
        path: Some(crate_dir.path().to_str().unwrap().to_string()),
        members: None,
        update: None,
    };

    let response = service.cache_crate(Parameters(params)).await;
    let task = parse_cache_task_started(&response)?;
    let result = wait_for_task_completion(&service, &task.task_id, TEST_TIMEOUT).await?;

    assert!(
        matches!(result, TaskResult::Success),
        "Feature fallback should have succeeded by skipping the `broken` feature: {result:?}"
    );

    // Verify the crate is actually queryable
    let versions_response = service
        .list_crate_versions(Parameters(ListCrateVersionsParams {
            crate_name: "feature-fallback-fixture".to_string(),
        }))
        .await;
    assert!(
        versions_response.contains("0.1.0"),
        "Cached version not found: {versions_response}"
    );

    Ok(())
}

// ===== PLATFORM-SPECIFIC COMPILATION TESTS =====

#[tokio::test]
#[cfg(target_os = "macos")]
#[ignore = "requires network access"]
async fn test_cache_bevy_with_feature_fallback() -> Result<()> {
    // NOTE: This test depends on external resources and may fail due to:
    // - Network connectivity issues
    // - crates.io downtime or rate limiting
    // - Bevy crate removal, version change, or dependency updates
    // - Platform-specific build environment issues
    //
    // If this test becomes unreliable in CI, consider:
    // - Mocking the crate download mechanism
    // - Using a smaller, more stable test crate
    // - Adding #[cfg_attr(env = "CI", ignore)] to skip on CI

    // Initialize tracing for this test
    let _ = tracing_subscriber::fmt()
        .with_env_filter("rust_docs_mcp=info")
        .try_init();

    let (service, _temp_dir) = create_test_service()?;

    // Test caching bevy 0.17.1 which has known compilation issues on macOS with --all-features
    // This should succeed using the feature fallback strategy (default features or no-default-features)
    let params = CacheCrateParams {
        crate_name: "bevy".to_string(),
        source_type: "cratesio".to_string(),
        version: Some("0.17.1".to_string()),
        github_url: None,
        branch: None,
        tag: None,
        path: None,
        members: None,
        update: None,
        features: None,
    };

    // Use a longer timeout for bevy as it's a large crate
    let response = service.cache_crate(Parameters(params)).await;

    // Parse async task response
    let task = parse_cache_task_started(&response)?;

    // Wait for completion with extended timeout
    let result = wait_for_task_completion(&service, &task.task_id, LARGE_CRATE_TEST_TIMEOUT)
        .await
        .context("Timeout while caching bevy - consider increasing LARGE_CRATE_TEST_TIMEOUT")?;

    // On macOS, bevy should succeed with the fallback strategy
    assert!(
        matches!(result, TaskResult::Success),
        "Bevy should cache successfully with feature fallback strategy: {result:?}"
    );

    eprintln!("✓ Successfully cached bevy on macOS (likely used fallback feature strategy)");

    // Verify it's actually cached
    let list_params = ListCrateVersionsParams {
        crate_name: "bevy".to_string(),
    };

    let versions_response = service.list_crate_versions(Parameters(list_params)).await;
    assert!(
        versions_response.contains("0.17.1"),
        "Bevy 0.17.1 should be in cache"
    );

    Ok(())
}

// ===== PROGRESS TRACKING TESTS =====

#[tokio::test]
#[ignore = "requires network access"]
async fn test_step_tracking() -> Result<()> {
    let (service, _temp_dir) = create_test_service()?;

    // Cache a small crate and track step updates
    let params = CacheCrateParams {
        crate_name: "semver".to_string(),
        source_type: "cratesio".to_string(),
        version: Some(SEMVER_VERSION.to_string()),
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
    let task_id = &task_output.task_id;

    // Track step updates - collect all steps we see
    let mut step_updates = Vec::new();
    let start = std::time::Instant::now();
    let timeout = TEST_TIMEOUT;
    let poll_interval = Duration::from_millis(100); // Poll faster to catch step updates

    loop {
        if start.elapsed() > timeout {
            return Err(anyhow::anyhow!(
                "Timeout waiting for task {task_id} to complete after {timeout:?}"
            ));
        }

        // Check task status
        let check_params = CacheOperationsParams {
            task_id: Some(task_id.to_string()),
            status_filter: None,
            cancel: false,
            clear: false,
        };

        let response = service.cache_operations(Parameters(check_params)).await;

        // Extract step if present
        if let Some((current, total, desc)) = extract_step_from_response(&response) {
            // Only record if different from last value
            let last_matches = step_updates
                .last()
                .map(|(c, t, _)| *c == current && *t == total)
                .unwrap_or(false);
            if !last_matches {
                println!("Step update: {current} of {total} {desc:?}");
                step_updates.push((current, total, desc));
            }
        }

        // Check if complete
        if response.contains("COMPLETED ✓") {
            break;
        } else if response.contains("FAILED ✗") {
            return Err(anyhow::anyhow!("Task failed: {response}"));
        } else if response.contains("CANCELLED") {
            return Err(anyhow::anyhow!("Task was cancelled"));
        }

        tokio::time::sleep(poll_interval).await;
    }

    println!("Step updates observed: {step_updates:?}");

    // Verify step tracking requirements:

    // 1. We should have seen at least SOME step updates
    assert!(
        !step_updates.is_empty(),
        "Should have observed at least some step updates"
    );

    // 2. Current step should never exceed total steps
    for (current, total, _) in &step_updates {
        assert!(
            current <= total,
            "Current step {current} exceeds total steps {total}"
        );
    }

    // 3. Steps should make sense (start at 1)
    if let Some((first_step, _, _)) = step_updates.first() {
        assert_eq!(*first_step, 1, "First step should be 1, got {first_step}");
    }

    println!("✓ Step tracking test passed");
    println!("  - Observed {} step updates", step_updates.len());
    if let (Some(first), Some(last)) = (step_updates.first(), step_updates.last()) {
        println!("  - First step: {} of {}", first.0, first.1);
        println!("  - Last step: {} of {}", last.0, last.1);
    }

    Ok(())
}

// Integration tests for the `features` parameter

/// Write a Cargo.toml + src/lib.rs for a crate that gates two public symbols
/// behind the `axum` and `actix` features. Both features are non-default, so the
/// presence of each symbol in the generated docs reflects which features were
/// enabled at `cargo rustdoc` time.
fn write_features_crate_sources(dir: &std::path::Path, package_name: &str) -> Result<()> {
    std::fs::write(
        dir.join("Cargo.toml"),
        format!(
            r#"[package]
name = "{package_name}"
version = "0.1.0"
edition = "2021"

[features]
default = []
axum = []
actix = []
"#
        ),
    )?;
    let src_dir = dir.join("src");
    std::fs::create_dir(&src_dir)?;
    std::fs::write(
        src_dir.join("lib.rs"),
        r#"//! Test crate gating symbols behind axum/actix features.

#[cfg(feature = "axum")]
pub mod axum_module {
    pub fn axum_handler() {}
}

#[cfg(feature = "actix")]
pub mod actix_module {
    pub fn actix_handler() {}
}
"#,
    )?;
    Ok(())
}

#[tokio::test]
async fn test_cache_with_specific_features() -> Result<()> {
    let (service, _temp_dir) = create_test_service()?;

    let fixture_dir = TempDir::new()?;
    write_features_crate_sources(fixture_dir.path(), "test-features-crate")?;

    let params = CacheCrateParams {
        crate_name: "test-features-crate".to_string(),
        source_type: "local".to_string(),
        version: Some("0.1.0".to_string()),
        github_url: None,
        branch: None,
        tag: None,
        path: Some(fixture_dir.path().to_str().unwrap().to_string()),
        members: None,
        update: None,
        features: Some(vec!["axum".to_string()]),
    };

    let response = service.cache_crate(Parameters(params)).await;
    let task_output = parse_cache_task_started(&response)?;
    let result = wait_for_task_completion(&service, &task_output.task_id, TEST_TIMEOUT).await?;
    assert!(
        matches!(result, TaskResult::Success),
        "Failed to cache with features=[axum]: {result:?}"
    );

    let search_axum = service
        .search_items_preview(Parameters(SearchItemsPreviewParams {
            crate_name: "test-features-crate".to_string(),
            version: "0.1.0".to_string(),
            pattern: "axum_handler".to_string(),
            limit: Some(10),
            offset: None,
            kind_filter: None,
            path_filter: None,
            member: None,
        }))
        .await;
    let axum_output: SearchItemsPreviewOutput = serde_json::from_str(&search_axum)?;
    assert!(
        !axum_output.items.is_empty(),
        "axum_handler not found in docs although features=[axum] was requested: {search_axum}"
    );

    let search_actix = service
        .search_items_preview(Parameters(SearchItemsPreviewParams {
            crate_name: "test-features-crate".to_string(),
            version: "0.1.0".to_string(),
            pattern: "actix_handler".to_string(),
            limit: Some(10),
            offset: None,
            kind_filter: None,
            path_filter: None,
            member: None,
        }))
        .await;
    let actix_output: SearchItemsPreviewOutput = serde_json::from_str(&search_actix)?;
    assert!(
        actix_output.items.is_empty(),
        "actix_handler visible in docs, but only features=[axum] was requested: {search_actix}"
    );

    Ok(())
}

#[tokio::test]
async fn test_cache_workspace_member_with_features() -> Result<()> {
    let (service, _temp_dir) = create_test_service()?;

    // Create a minimal workspace with a single member that has the features fixture
    let workspace_dir = TempDir::new()?;
    std::fs::write(
        workspace_dir.path().join("Cargo.toml"),
        r#"[workspace]
members = ["member-a"]
resolver = "2"
"#,
    )?;
    let member_dir = workspace_dir.path().join("member-a");
    std::fs::create_dir(&member_dir)?;
    write_features_crate_sources(&member_dir, "member-a")?;

    let params = CacheCrateParams {
        crate_name: "test-features-workspace".to_string(),
        source_type: "local".to_string(),
        version: Some("0.1.0".to_string()),
        github_url: None,
        branch: None,
        tag: None,
        path: Some(workspace_dir.path().to_str().unwrap().to_string()),
        members: Some(vec!["member-a".to_string()]),
        update: None,
        features: Some(vec!["axum".to_string()]),
    };

    let response = service.cache_crate(Parameters(params)).await;
    let task_output = parse_cache_task_started(&response)?;
    let result = wait_for_task_completion(&service, &task_output.task_id, TEST_TIMEOUT).await?;
    assert!(
        matches!(result, TaskResult::Success),
        "Failed to cache workspace member with features=[axum]: {result:?}"
    );

    let search_axum = service
        .search_items_preview(Parameters(SearchItemsPreviewParams {
            crate_name: "test-features-workspace".to_string(),
            version: "0.1.0".to_string(),
            pattern: "axum_handler".to_string(),
            limit: Some(10),
            offset: None,
            kind_filter: None,
            path_filter: None,
            member: Some("member-a".to_string()),
        }))
        .await;
    let axum_output: SearchItemsPreviewOutput = serde_json::from_str(&search_axum)?;
    assert!(
        !axum_output.items.is_empty(),
        "axum_handler not found in workspace member docs: {search_axum}"
    );

    let search_actix = service
        .search_items_preview(Parameters(SearchItemsPreviewParams {
            crate_name: "test-features-workspace".to_string(),
            version: "0.1.0".to_string(),
            pattern: "actix_handler".to_string(),
            limit: Some(10),
            offset: None,
            kind_filter: None,
            path_filter: None,
            member: Some("member-a".to_string()),
        }))
        .await;
    let actix_output: SearchItemsPreviewOutput = serde_json::from_str(&search_actix)?;
    assert!(
        actix_output.items.is_empty(),
        "actix_handler visible in workspace member docs although only features=[axum] was requested: {search_actix}"
    );

    Ok(())
}

#[tokio::test]
#[ignore = "Heavy network test (compiles leptos-use ~60s+), starves the 2-core CI runner. Run with --ignored."]
async fn test_cache_leptos_use_with_axum_feature() -> Result<()> {
    // End-to-end reproduction of the real-world scenario that motivated PR #57:
    // a crate with mutually exclusive features (axum vs actix) that cannot be
    // cached with --all-features. Takes ~80s locally (mostly cargo compile).
    let (service, _temp_dir) = create_test_service()?;

    let params = CacheCrateParams {
        crate_name: "leptos-use".to_string(),
        source_type: "cratesio".to_string(),
        version: Some("0.18.3".to_string()),
        github_url: None,
        branch: None,
        tag: None,
        path: None,
        members: None,
        update: None,
        features: Some(vec!["axum".to_string()]),
    };

    let response = service.cache_crate(Parameters(params)).await;
    let task_output = parse_cache_task_started(&response)?;
    let result =
        wait_for_task_completion(&service, &task_output.task_id, HEAVY_NETWORK_TEST_TIMEOUT)
            .await?;
    assert!(
        matches!(result, TaskResult::Success),
        "Failed to cache leptos-use@0.18.3 with features=[axum]: {result:?}"
    );

    Ok(())
}

#[tokio::test]
#[ignore = "Documents a pre-existing cache-key bug (features not part of cache identity). Expected to FAIL today; will pass once the cache key includes a features fingerprint."]
async fn test_cache_respects_feature_change() -> Result<()> {
    // This test documents a known, pre-existing bug that is NOT in scope of PR #57
    // but became user-reachable once features are honored: the on-disk cache is
    // keyed only by (name, version), so a second cache_crate call with a different
    // feature set short-circuits on has_docs() and returns the first call's docs.
    // The user sees success but gets the wrong feature set's docs.
    //
    // Once the cache key includes a features fingerprint (or features-differing
    // calls trigger invalidation), this test will pass and #[ignore] can be removed.
    let (service, _temp_dir) = create_test_service()?;

    let fixture_dir = TempDir::new()?;
    write_features_crate_sources(fixture_dir.path(), "test-features-cachekey")?;

    // First cache with features=["axum"]
    let params_axum = CacheCrateParams {
        crate_name: "test-features-cachekey".to_string(),
        source_type: "local".to_string(),
        version: Some("0.1.0".to_string()),
        github_url: None,
        branch: None,
        tag: None,
        path: Some(fixture_dir.path().to_str().unwrap().to_string()),
        members: None,
        update: None,
        features: Some(vec!["axum".to_string()]),
    };
    let response = service.cache_crate(Parameters(params_axum)).await;
    let task_output = parse_cache_task_started(&response)?;
    let result = wait_for_task_completion(&service, &task_output.task_id, TEST_TIMEOUT).await?;
    assert!(
        matches!(result, TaskResult::Success),
        "First cache (features=[axum]) failed: {result:?}"
    );

    // Second cache of same (name, version) but features=["actix"] — no update flag
    let params_actix = CacheCrateParams {
        crate_name: "test-features-cachekey".to_string(),
        source_type: "local".to_string(),
        version: Some("0.1.0".to_string()),
        github_url: None,
        branch: None,
        tag: None,
        path: Some(fixture_dir.path().to_str().unwrap().to_string()),
        members: None,
        update: None,
        features: Some(vec!["actix".to_string()]),
    };
    let response = service.cache_crate(Parameters(params_actix)).await;
    let task_output = parse_cache_task_started(&response)?;
    let result = wait_for_task_completion(&service, &task_output.task_id, TEST_TIMEOUT).await?;
    assert!(
        matches!(result, TaskResult::Success),
        "Second cache (features=[actix]) failed: {result:?}"
    );

    // These two assertions FAIL today because the second call short-circuits on
    // has_docs() without regenerating. They should pass once the cache key is
    // feature-aware.
    let search_actix = service
        .search_items_preview(Parameters(SearchItemsPreviewParams {
            crate_name: "test-features-cachekey".to_string(),
            version: "0.1.0".to_string(),
            pattern: "actix_handler".to_string(),
            limit: Some(10),
            offset: None,
            kind_filter: None,
            path_filter: None,
            member: None,
        }))
        .await;
    let actix_output: SearchItemsPreviewOutput = serde_json::from_str(&search_actix)?;
    assert!(
        !actix_output.items.is_empty(),
        "actix_handler NOT visible after features=[actix] was requested — the cache returned stale docs from the first features=[axum] call"
    );

    let search_axum = service
        .search_items_preview(Parameters(SearchItemsPreviewParams {
            crate_name: "test-features-cachekey".to_string(),
            version: "0.1.0".to_string(),
            pattern: "axum_handler".to_string(),
            limit: Some(10),
            offset: None,
            kind_filter: None,
            path_filter: None,
            member: None,
        }))
        .await;
    let axum_output: SearchItemsPreviewOutput = serde_json::from_str(&search_axum)?;
    assert!(
        axum_output.items.is_empty(),
        "axum_handler still visible after re-cache with features=[actix] — stale docs from the first call were not invalidated"
    );

    Ok(())
}
