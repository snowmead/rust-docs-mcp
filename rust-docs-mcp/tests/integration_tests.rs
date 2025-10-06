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
    CacheCrateOutput, GetCratesMetadataOutput, ListCrateVersionsOutput,
};
use rust_docs_mcp::cache::tools::{
    CacheCrateFromCratesIOParams, CacheCrateFromGitHubParams, CacheCrateFromLocalParams,
    CrateMetadataQuery, GetCratesMetadataParams, ListCachedCratesParams, ListCrateVersionsParams,
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
const SEMVER_VERSION: &str = "1.0.0";
const SERDE_VERSION: &str = "v1.0.136";
const SERDE_GITHUB_URL: &str = "https://github.com/serde-rs/serde";
const CLIPPY_GITHUB_URL: &str = "https://github.com/rust-lang/rust-clippy";
const CLIPPY_BRANCH: &str = "master";

// Response validation helpers
fn parse_cache_response(response: &str) -> Result<CacheCrateOutput> {
    serde_json::from_str(response).map_err(|e| {
        anyhow::anyhow!(
            "Failed to parse cache response: {}\nResponse: {}",
            e,
            response
        )
    })
}

fn is_binary_only_response(response: &str) -> bool {
    // Binary-only packages will return an error with this message
    if let Ok(output) = parse_cache_response(response) {
        matches!(output, CacheCrateOutput::Error { error } if error.contains("binary-only") || error.contains("no library"))
    } else {
        false
    }
}

/// Helper to create a test service with temporary cache
fn create_test_service() -> Result<(RustDocsService, TempDir)> {
    let temp_dir = TempDir::new()?;
    let service = RustDocsService::new(Some(temp_dir.path().to_path_buf()))?;
    Ok((service, temp_dir))
}

/// Helper to setup and cache the semver test crate
async fn setup_test_crate(service: &RustDocsService) -> Result<()> {
    let params = CacheCrateFromCratesIOParams {
        crate_name: "semver".to_string(),
        version: SEMVER_VERSION.to_string(),
        members: None,
        update: None,
    };

    let response = tokio::time::timeout(
        TEST_TIMEOUT,
        service.cache_crate_from_cratesio(Parameters(params)),
    )
    .await?;

    let output = parse_cache_response(&response)?;
    if !output.is_success() {
        return Err(anyhow::anyhow!("Failed to cache test crate: {:?}", output));
    }
    Ok(())
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
async fn test_cache_from_crates_io() -> Result<()> {
    let (service, _temp_dir) = create_test_service()?;

    // Cache a small, stable crate from crates.io
    let params = CacheCrateFromCratesIOParams {
        crate_name: "semver".to_string(),
        version: SEMVER_VERSION.to_string(),
        members: None,
        update: None,
    };

    let response = tokio::time::timeout(
        TEST_TIMEOUT,
        service.cache_crate_from_cratesio(Parameters(params)),
    )
    .await?;

    let output = parse_cache_response(&response)?;
    match &output {
        CacheCrateOutput::Success {
            crate_name,
            version,
            ..
        } => {
            assert_eq!(crate_name, "semver");
            assert_eq!(version, SEMVER_VERSION);
        }
        _ => panic!("Expected success response, got: {output:?}"),
    }

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
async fn test_cache_from_github() -> Result<()> {
    let (service, _temp_dir) = create_test_service()?;

    // Cache a crate from GitHub using a tag
    let params = CacheCrateFromGitHubParams {
        crate_name: "serde-test".to_string(),
        github_url: SERDE_GITHUB_URL.to_string(),
        branch: None,
        tag: Some(SERDE_VERSION.to_string()),
        members: None,
        update: None,
    };

    let response = tokio::time::timeout(
        TEST_TIMEOUT,
        service.cache_crate_from_github(Parameters(params)),
    )
    .await?;

    // Serde is a workspace, so we should get a workspace detection response
    let output = parse_cache_response(&response)?;
    assert!(
        output.is_workspace_detected(),
        "Expected workspace detection for serde: {output:?}"
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
async fn test_cache_from_github_branch() -> Result<()> {
    // Initialize tracing for this test
    let _ = tracing_subscriber::fmt()
        .with_env_filter("rust_docs_mcp=debug")
        .try_init();

    let (service, _temp_dir) = create_test_service()?;

    // Cache from GitHub using a branch
    let params = CacheCrateFromGitHubParams {
        crate_name: "clippy-test".to_string(),
        github_url: CLIPPY_GITHUB_URL.to_string(),
        branch: Some(CLIPPY_BRANCH.to_string()),
        tag: None,
        members: None,
        update: None,
    };

    let response = tokio::time::timeout(
        TEST_TIMEOUT,
        service.cache_crate_from_github(Parameters(params)),
    )
    .await?;

    // Print the response for debugging
    println!("Response: {response}");

    // Clippy is a binary-only package, so we should expect an appropriate error
    assert!(
        is_binary_only_response(&response),
        "Expected binary-only package response, got: {response}"
    );

    Ok(())
}

#[tokio::test]
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
    let params = CacheCrateFromLocalParams {
        crate_name: "test-local".to_string(),
        version: Some("0.1.0".to_string()),
        path: test_crate_dir.path().to_str().unwrap().to_string(),
        members: None,
        update: None,
    };

    let response = service.cache_crate_from_local(Parameters(params)).await;
    let output = parse_cache_response(&response)?;
    assert!(
        output.is_success(),
        "Failed to cache from local path: {output:?}"
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
    let params = CacheCrateFromLocalParams {
        crate_name: "test-workspace".to_string(),
        version: Some("0.1.0".to_string()),
        path: workspace_dir.path().to_str().unwrap().to_string(),
        members: None, // Should detect workspace and return member list
        update: None,
    };

    let response = service.cache_crate_from_local(Parameters(params)).await;

    // Response should indicate workspace detection
    let output = parse_cache_response(&response)?;
    assert!(
        output.is_workspace_detected(),
        "Response should indicate workspace detection: {output:?}"
    );

    if let CacheCrateOutput::WorkspaceDetected {
        workspace_members, ..
    } = &output
    {
        assert!(workspace_members.contains(&"crate-a".to_string()));
        assert!(workspace_members.contains(&"crate-b".to_string()));
    }

    Ok(())
}

#[tokio::test]
async fn test_cache_update() -> Result<()> {
    let (service, _temp_dir) = create_test_service()?;

    // Cache initially
    let params1 = CacheCrateFromCratesIOParams {
        crate_name: "once_cell".to_string(),
        version: "1.17.0".to_string(),
        members: None,
        update: None,
    };

    let response1 = tokio::time::timeout(
        TEST_TIMEOUT,
        service.cache_crate_from_cratesio(Parameters(params1)),
    )
    .await?;
    let output1 = parse_cache_response(&response1)?;
    assert!(output1.is_success(), "Initial cache failed: {output1:?}");

    // Cache again with update flag
    let params2 = CacheCrateFromCratesIOParams {
        crate_name: "once_cell".to_string(),
        version: "1.17.0".to_string(),
        members: None,
        update: Some(true),
    };

    let response2 = tokio::time::timeout(
        TEST_TIMEOUT,
        service.cache_crate_from_cratesio(Parameters(params2)),
    )
    .await?;
    // Update should return "Successfully updated" message
    let output2 = parse_cache_response(&response2)?;
    assert!(output2.is_success(), "Update cache failed: {output2:?}");

    if let CacheCrateOutput::Success { updated, .. } = &output2 {
        assert_eq!(*updated, Some(true), "Should have updated flag set");
    }

    Ok(())
}

#[tokio::test]
async fn test_invalid_inputs() -> Result<()> {
    let (service, _temp_dir) = create_test_service()?;

    // Test non-existent crate from crates.io
    let params = CacheCrateFromCratesIOParams {
        crate_name: "this-crate-definitely-does-not-exist-123456".to_string(),
        version: "1.0.0".to_string(),
        members: None,
        update: None,
    };

    let response = tokio::time::timeout(
        TEST_TIMEOUT,
        service.cache_crate_from_cratesio(Parameters(params)),
    )
    .await?;

    // crates.io returns 403 Forbidden for non-existent crates
    let output = parse_cache_response(&response)?;
    assert!(
        output.is_error(),
        "Expected error response, got: {output:?}"
    );

    // Test invalid GitHub URL
    let params = CacheCrateFromGitHubParams {
        crate_name: "invalid".to_string(),
        github_url: "not-a-valid-url".to_string(),
        branch: None,
        tag: Some("v1.0.0".to_string()),
        members: None,
        update: None,
    };

    let response = service.cache_crate_from_github(Parameters(params)).await;
    let output = parse_cache_response(&response)?;
    assert!(
        output.is_error(),
        "Expected error response, got: {output:?}"
    );

    // Test non-existent local path
    let params = CacheCrateFromLocalParams {
        crate_name: "invalid".to_string(),
        version: Some("1.0.0".to_string()),
        path: "/this/path/does/not/exist".to_string(),
        members: None,
        update: None,
    };

    let response = service.cache_crate_from_local(Parameters(params)).await;
    let output = parse_cache_response(&response)?;
    assert!(
        output.is_error(),
        "Expected error response, got: {output:?}"
    );

    Ok(())
}

#[tokio::test]
async fn test_concurrent_caching() -> Result<()> {
    let (service, _temp_dir) = create_test_service()?;
    let service = std::sync::Arc::new(service);

    // Define test crates with expected metadata
    let test_crates = vec![
        ("semver", "1.0.0"),
        ("once_cell", "1.0.0"),
        ("regex", "1.11.1"), // Use the latest version compatible with nightly toolchain
    ];

    // Cache multiple crates concurrently
    let mut handles = vec![];

    for (name, version) in &test_crates {
        let service_clone = service.clone();
        let name = name.to_string();
        let version = version.to_string();

        let handle = tokio::spawn(async move {
            let params = CacheCrateFromCratesIOParams {
                crate_name: name.clone(),
                version: version.clone(),
                members: None,
                update: None,
            };
            let start = std::time::Instant::now();
            let result = service_clone
                .cache_crate_from_cratesio(Parameters(params))
                .await;
            let duration = start.elapsed();
            (name, version, result, duration)
        });

        handles.push(handle);
    }

    // Wait for all to complete and verify results
    let mut results = vec![];
    for handle in handles {
        let (name, version, result, duration) = handle.await?;
        println!("Cached {name} {version} in {duration:?}");

        let success = if let Ok(output) = parse_cache_response(&result) {
            output.is_success()
        } else {
            false
        };
        if !success {
            eprintln!("Concurrent cache failed for {name}: {result}");
        }
        results.push((name, version, result));
    }

    // Verify all operations succeeded
    for (name, version, result) in &results {
        let output = parse_cache_response(result)?;
        assert!(
            output.is_success(),
            "Failed to cache {name} {version}: {output:?}"
        );
    }

    // Verify cache consistency - all crates should be present
    let cached_crates_response = service
        .list_cached_crates(Parameters(ListCachedCratesParams {}))
        .await;
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

    // Verify no corruption by attempting to use the cached crates
    // This would fail if the cache was corrupted during concurrent access
    for (name, version) in &test_crates {
        let params = CacheCrateFromCratesIOParams {
            crate_name: name.to_string(),
            version: version.to_string(),
            members: None,
            update: Some(false), // Should not re-download if already cached
        };
        let result = service.cache_crate_from_cratesio(Parameters(params)).await;
        let output = parse_cache_response(&result)?;
        // Should either be already cached or successful
        let is_valid = match &output {
            CacheCrateOutput::Success { .. } => true,
            CacheCrateOutput::Error { error } if error.contains("already cached") => true,
            _ => false,
        };
        assert!(
            is_valid,
            "Cache integrity check failed for {name} {version}: {output:?}"
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
    let params1 = CacheCrateFromLocalParams {
        crate_name: "my-workspace".to_string(),
        version: Some("1.0.0".to_string()),
        path: workspace_dir.path().to_str().unwrap().to_string(),
        members: None,
        update: None,
    };

    let response1 = service.cache_crate_from_local(Parameters(params1)).await;
    let output1 = parse_cache_response(&response1)?;
    assert!(
        matches!(&output1, CacheCrateOutput::WorkspaceDetected { workspace_members, .. }
            if workspace_members.contains(&"lib-a".to_string())
            && workspace_members.contains(&"lib-b".to_string())),
        "Should detect workspace and list members: {output1:?}"
    );

    // Now cache with specific members
    let params2 = CacheCrateFromLocalParams {
        crate_name: "my-workspace".to_string(),
        version: Some("1.0.0".to_string()),
        path: workspace_dir.path().to_str().unwrap().to_string(),
        members: Some(vec!["lib-a".to_string(), "lib-b".to_string()]),
        update: None,
    };

    let response2 = service.cache_crate_from_local(Parameters(params2)).await;
    let output2 = parse_cache_response(&response2)?;
    assert!(
        matches!(&output2, CacheCrateOutput::Success { .. }),
        "Should successfully cache workspace members: {output2:?}"
    );

    Ok(())
}

// ===== DOCUMENTATION TOOLS TESTS =====

#[tokio::test]
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
