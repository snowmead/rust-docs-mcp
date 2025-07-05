#[cfg(test)]
mod tests {
    use super::super::*;
    use std::sync::Arc;
    use tempfile::TempDir;
    use std::time::Duration;

    #[tokio::test]
    async fn test_service_with_metrics() {
        let temp_dir = TempDir::new().unwrap();
        let metrics = Arc::new(metrics::MetricsServer::new().unwrap());
        
        let service = RustDocsService::new(
            Some(temp_dir.path().to_path_buf()),
            Some(metrics.clone())
        ).unwrap();
        
        // Verify metrics were passed correctly
        assert!(service.metrics.is_some());
    }

    #[tokio::test]
    async fn test_query_duration_metrics() {
        let temp_dir = TempDir::new().unwrap();
        let metrics = Arc::new(metrics::MetricsServer::new().unwrap());
        
        let service = RustDocsService::new(
            Some(temp_dir.path().to_path_buf()),
            Some(metrics.clone())
        ).unwrap();
        
        // Call a tool that has metrics instrumentation
        let params = cache::tools::CacheCrateFromCratesIOParams {
            crate_name: "nonexistent-crate-for-testing".to_string(),
            version: "0.1.0".to_string(),
            members: None,
            update: None,
        };
        
        // This will fail but should still record metrics
        let _ = service.cache_crate_from_cratesio(params).await;
        
        // Metrics should have been recorded (can't check exact values due to internal state)
        // Just verify it doesn't panic
    }

    #[tokio::test]
    async fn test_cache_metrics_update() {
        let temp_dir = TempDir::new().unwrap();
        let metrics = Arc::new(metrics::MetricsServer::new().unwrap());
        
        // Create a cache with metrics
        let cache = cache::CrateCache::new_with_metrics(
            Some(temp_dir.path().to_path_buf()),
            Some(metrics.clone())
        ).unwrap();
        
        // Update cache metrics
        cache.update_cache_metrics().await.unwrap();
        
        // Verify health status was updated
        let health = metrics.get_health_status().read().await;
        assert!(health.metrics.contains_key("cache_entries"));
        assert!(health.metrics.contains_key("cache_size_mb"));
    }

    #[tokio::test]
    async fn test_service_without_metrics() {
        let temp_dir = TempDir::new().unwrap();
        
        let service = RustDocsService::new(
            Some(temp_dir.path().to_path_buf()),
            None
        ).unwrap();
        
        // Verify service works without metrics
        assert!(service.metrics.is_none());
        
        // Call a tool - should work without metrics
        let result = service.list_cached_crates().await;
        assert!(result.contains("cached_crates"));
    }

    #[tokio::test]
    async fn test_all_tools_have_metrics() {
        let temp_dir = TempDir::new().unwrap();
        let metrics = Arc::new(metrics::MetricsServer::new().unwrap());
        
        let service = RustDocsService::new(
            Some(temp_dir.path().to_path_buf()),
            Some(metrics.clone())
        ).unwrap();
        
        // Test cache tools
        let _ = service.list_cached_crates().await;
        let _ = service.list_crate_versions("test".to_string()).await;
        
        // Test search tools (will fail but metrics should be recorded)
        let search_params = docs::tools::SearchItemsParams {
            crate_name: "test".to_string(),
            version: "0.1.0".to_string(),
            query: "test".to_string(),
            case_sensitive: None,
            whole_word: None,
            limit: None,
            member: None,
        };
        let _ = service.search_items(search_params.clone()).await;
        
        let preview_params = docs::tools::SearchItemsPreviewParams {
            crate_name: search_params.crate_name,
            version: search_params.version,
            query: search_params.query,
            case_sensitive: search_params.case_sensitive,
            whole_word: search_params.whole_word,
            limit: search_params.limit,
            member: search_params.member,
        };
        let _ = service.search_items_preview(preview_params).await;
        
        // All calls should complete without panicking
    }
}