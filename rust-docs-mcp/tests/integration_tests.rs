//! Integration tests for rust-docs-mcp caching functionality
//!
//! These tests verify that caching works correctly for all three sources:
//! - crates.io
//! - GitHub
//! - Local paths

use anyhow::Result;
use rust_docs_mcp::RustDocsService;
use rust_docs_mcp::cache::tools::{
    CacheCrateFromCratesIOParams, CacheCrateFromGitHubParams, CacheCrateFromLocalParams,
    ListCrateVersionsParams,
};
use rmcp::handler::server::tool::Parameters;
use tempfile::TempDir;

/// Helper to create a test service with temporary cache
fn create_test_service() -> Result<(RustDocsService, TempDir)> {
    let temp_dir = TempDir::new()?;
    let service = RustDocsService::new(Some(temp_dir.path().to_path_buf()))?;
    Ok((service, temp_dir))
}

#[tokio::test]
async fn test_cache_from_crates_io() -> Result<()> {
    let (service, _temp_dir) = create_test_service()?;
    
    // Cache a small, stable crate from crates.io
    let params = CacheCrateFromCratesIOParams {
        crate_name: "semver".to_string(),
        version: "1.0.0".to_string(),
        members: None,
        update: None,
    };
    
    let response = service.cache_crate_from_cratesio(Parameters(params)).await;
    assert!(response.contains("success"), "Failed to cache from crates.io: {}", response);
    
    // Verify it's in the cache by listing versions
    let list_params = ListCrateVersionsParams {
        crate_name: "semver".to_string(),
    };
    
    let versions_response = service.list_crate_versions(Parameters(list_params)).await;
    assert!(versions_response.contains("1.0.0"), "Version not found in cache: {}", versions_response);
    
    Ok(())
}

#[tokio::test]
async fn test_cache_from_github() -> Result<()> {
    let (service, _temp_dir) = create_test_service()?;
    
    // Cache a crate from GitHub using a tag
    let params = CacheCrateFromGitHubParams {
        crate_name: "serde-test".to_string(),
        github_url: "https://github.com/serde-rs/serde".to_string(),
        branch: None,
        tag: Some("v1.0.136".to_string()),
        members: None,
        update: None,
    };
    
    let response = service.cache_crate_from_github(Parameters(params)).await;
    assert!(response.contains("success") || response.contains("Success"), 
        "Failed to cache from GitHub: {}", response);
    
    // Verify cached
    let list_params = ListCrateVersionsParams {
        crate_name: "serde-test".to_string(),
    };
    
    let versions_response = service.list_crate_versions(Parameters(list_params)).await;
    assert!(versions_response.contains("v1.0.136"), "Version not found: {}", versions_response);
    
    Ok(())
}

#[tokio::test]
async fn test_cache_from_github_branch() -> Result<()> {
    let (service, _temp_dir) = create_test_service()?;
    
    // Cache from GitHub using a branch
    let params = CacheCrateFromGitHubParams {
        crate_name: "clippy-test".to_string(),
        github_url: "https://github.com/rust-lang/rust-clippy".to_string(),
        branch: Some("master".to_string()),
        tag: None,
        members: None,
        update: None,
    };
    
    let response = service.cache_crate_from_github(Parameters(params)).await;
    // Clippy is a workspace, so we might get a workspace detection response
    assert!(response.contains("success") || response.contains("workspace"), 
        "Unexpected response: {}", response);
    
    Ok(())
}

#[tokio::test]
async fn test_cache_from_local_path() -> Result<()> {
    let (service, _temp_dir) = create_test_service()?;
    
    // Create a test crate in a temporary directory
    let test_crate_dir = TempDir::new()?;
    let cargo_toml_path = test_crate_dir.path().join("Cargo.toml");
    std::fs::write(&cargo_toml_path, r#"
[package]
name = "test-local"
version = "0.1.0"
edition = "2021"

[dependencies]
    "#)?;
    
    // Create a minimal lib.rs
    let src_dir = test_crate_dir.path().join("src");
    std::fs::create_dir(&src_dir)?;
    std::fs::write(src_dir.join("lib.rs"), "//! Test local crate\npub fn test() {}")?;
    
    // Cache from local path
    let params = CacheCrateFromLocalParams {
        crate_name: "test-local".to_string(),
        version: Some("0.1.0".to_string()),
        path: test_crate_dir.path().to_str().unwrap().to_string(),
        members: None,
        update: None,
    };
    
    let response = service.cache_crate_from_local(Parameters(params)).await;
    assert!(response.contains("success"), "Failed to cache from local path: {}", response);
    
    // Verify cached
    let list_params = ListCrateVersionsParams {
        crate_name: "test-local".to_string(),
    };
    
    let versions_response = service.list_crate_versions(Parameters(list_params)).await;
    assert!(versions_response.contains("0.1.0"), "Version not found: {}", versions_response);
    
    Ok(())
}

#[tokio::test]
async fn test_workspace_crate_detection() -> Result<()> {
    let (service, _temp_dir) = create_test_service()?;
    
    // Create a workspace crate
    let workspace_dir = TempDir::new()?;
    let workspace_toml = workspace_dir.path().join("Cargo.toml");
    std::fs::write(&workspace_toml, r#"
[workspace]
members = ["crate-a", "crate-b"]
resolver = "2"

[workspace.dependencies]
serde = "1.0"
    "#)?;
    
    // Create member crates
    for member in &["crate-a", "crate-b"] {
        let member_dir = workspace_dir.path().join(member);
        std::fs::create_dir_all(&member_dir)?;
        std::fs::write(member_dir.join("Cargo.toml"), format!(r#"
[package]
name = "{}"
version = "0.1.0"
edition = "2021"

[dependencies]
serde = {{ workspace = true }}
        "#, member))?;
        
        let src_dir = member_dir.join("src");
        std::fs::create_dir(&src_dir)?;
        std::fs::write(src_dir.join("lib.rs"), format!("//! {} crate", member))?;
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
    assert!(response.contains("workspace"), "Response should mention workspace: {}", response);
    assert!(response.contains("crate-a") && response.contains("crate-b"), 
        "Response should list workspace members: {}", response);
    
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
    
    let response1 = service.cache_crate_from_cratesio(Parameters(params1)).await;
    assert!(response1.contains("success"), "Initial cache failed: {}", response1);
    
    // Cache again with update flag
    let params2 = CacheCrateFromCratesIOParams {
        crate_name: "once_cell".to_string(),
        version: "1.17.0".to_string(),
        members: None,
        update: Some(true),
    };
    
    let response2 = service.cache_crate_from_cratesio(Parameters(params2)).await;
    assert!(response2.contains("success") || response2.contains("updated"), 
        "Update cache failed: {}", response2);
    
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
    
    let response = service.cache_crate_from_cratesio(Parameters(params)).await;
    assert!(response.contains("error") || response.contains("Error") || response.contains("failed"), 
        "Expected error response: {}", response);
    
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
    assert!(response.contains("error") || response.contains("Error") || response.contains("invalid"), 
        "Expected error response: {}", response);
    
    // Test non-existent local path
    let params = CacheCrateFromLocalParams {
        crate_name: "invalid".to_string(),
        version: Some("1.0.0".to_string()),
        path: "/this/path/does/not/exist".to_string(),
        members: None,
        update: None,
    };
    
    let response = service.cache_crate_from_local(Parameters(params)).await;
    assert!(response.contains("error") || response.contains("Error") || response.contains("not found"), 
        "Expected error response: {}", response);
    
    Ok(())
}

#[tokio::test]
async fn test_concurrent_caching() -> Result<()> {
    let (service, _temp_dir) = create_test_service()?;
    let service = std::sync::Arc::new(service);
    
    // Cache multiple crates concurrently
    let mut handles = vec![];
    
    for (name, version) in &[
        ("serde", "1.0.0"),
        ("serde_json", "1.0.0"),
        ("anyhow", "1.0.0"),
    ] {
        let service_clone = service.clone();
        let name = name.to_string();
        let version = version.to_string();
        
        let handle = tokio::spawn(async move {
            let params = CacheCrateFromCratesIOParams {
                crate_name: name,
                version,
                members: None,
                update: None,
            };
            service_clone.cache_crate_from_cratesio(Parameters(params)).await
        });
        
        handles.push(handle);
    }
    
    // Wait for all to complete
    let mut all_success = true;
    for handle in handles {
        let result = handle.await?;
        if !result.contains("success") {
            all_success = false;
            eprintln!("Concurrent cache failed: {}", result);
        }
    }
    
    assert!(all_success, "Some concurrent caching operations failed");
    
    // Verify all are cached
    let cached_crates_response = service.list_cached_crates().await;
    assert!(cached_crates_response.contains("serde"), "serde not found in cache");
    assert!(cached_crates_response.contains("serde_json"), "serde_json not found in cache");
    assert!(cached_crates_response.contains("anyhow"), "anyhow not found in cache");
    
    Ok(())
}

#[tokio::test]
async fn test_workspace_member_caching() -> Result<()> {
    let (service, _temp_dir) = create_test_service()?;
    
    // Create a workspace with members
    let workspace_dir = TempDir::new()?;
    let workspace_toml = workspace_dir.path().join("Cargo.toml");
    std::fs::write(&workspace_toml, r#"
[workspace]
members = ["lib-a", "lib-b"]
resolver = "2"
    "#)?;
    
    // Create member crates
    for (member, version) in &[("lib-a", "0.1.0"), ("lib-b", "0.2.0")] {
        let member_dir = workspace_dir.path().join(member);
        std::fs::create_dir_all(&member_dir)?;
        std::fs::write(member_dir.join("Cargo.toml"), format!(r#"
[package]
name = "{}"
version = "{}"
edition = "2021"
        "#, member, version))?;
        
        let src_dir = member_dir.join("src");
        std::fs::create_dir(&src_dir)?;
        std::fs::write(src_dir.join("lib.rs"), format!("//! {} library", member))?;
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
    assert!(response1.contains("workspace") && response1.contains("lib-a") && response1.contains("lib-b"),
        "Should detect workspace and list members: {}", response1);
    
    // Now cache with specific members
    let params2 = CacheCrateFromLocalParams {
        crate_name: "my-workspace".to_string(),
        version: Some("1.0.0".to_string()),
        path: workspace_dir.path().to_str().unwrap().to_string(),
        members: Some(vec!["lib-a".to_string(), "lib-b".to_string()]),
        update: None,
    };
    
    let response2 = service.cache_crate_from_local(Parameters(params2)).await;
    assert!(response2.contains("success") || response2.contains("cached"),
        "Should successfully cache workspace members: {}", response2);
    
    Ok(())
}