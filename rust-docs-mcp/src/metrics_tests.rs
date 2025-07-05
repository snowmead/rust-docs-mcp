#[cfg(test)]
mod tests {
    use super::super::metrics::*;
    use std::time::Duration;

    #[test]
    fn test_metrics_server_new() {
        let metrics = MetricsServer::new();
        assert!(metrics.is_ok());
    }

    #[test]
    fn test_cache_metrics() {
        let metrics = MetricsServer::new().unwrap();
        
        // Record cache hits
        metrics.record_cache_hit("test_crate");
        metrics.record_cache_hit("test_crate");
        metrics.record_cache_hit("another_crate");
        
        // Record cache misses
        metrics.record_cache_miss("test_crate");
        metrics.record_cache_miss("missing_crate");
        
        // Set cache size and entries
        metrics.set_cache_size_bytes(1024.0 * 1024.0 * 100.0); // 100MB
        metrics.set_cache_entries_total(5.0);
    }

    #[test]
    fn test_query_metrics() {
        let metrics = MetricsServer::new().unwrap();
        
        // Record query duration
        metrics.record_query_duration("cache", "crate_from_cratesio", Duration::from_millis(150));
        metrics.record_query_duration("docs", "search_items", Duration::from_millis(50));
        metrics.record_query_duration("deps", "get_dependencies", Duration::from_millis(75));
        
        // Record query errors
        metrics.record_query_error("network_error");
        metrics.record_query_error("parse_error");
    }

    #[test]
    fn test_system_metrics() {
        let metrics = MetricsServer::new().unwrap();
        
        // Set system metrics
        metrics.set_memory_usage_bytes(1024.0 * 1024.0 * 512.0); // 512MB
        metrics.set_disk_usage_bytes(1024.0 * 1024.0 * 1024.0 * 2.0); // 2GB
        metrics.set_uptime_seconds(3600.0); // 1 hour
    }

    #[tokio::test]
    async fn test_health_status_access() {
        let metrics = MetricsServer::new().unwrap();
        let health_status = metrics.get_health_status();
        
        {
            let mut health = health_status.write().await;
            health.set_cache_metrics(10, 100);
        }
        
        {
            let health = health_status.read().await;
            assert_eq!(
                health.metrics.get("cache_entries").unwrap(),
                &serde_json::Value::Number(serde_json::Number::from(10))
            );
        }
    }

    #[test]
    fn test_metrics_server_clone() {
        let metrics = MetricsServer::new().unwrap();
        let cloned = metrics.clone();
        
        // Both should point to the same metrics
        metrics.record_cache_hit("test");
        cloned.record_cache_hit("test");
        
        // Should work without panicking
        metrics.set_cache_size_bytes(1000.0);
        cloned.set_cache_size_bytes(2000.0);
    }

    #[tokio::test]
    async fn test_metrics_server_concurrent_access() {
        use std::sync::Arc;
        use tokio::task;
        
        let metrics = Arc::new(MetricsServer::new().unwrap());
        let mut handles = vec![];
        
        // Spawn multiple tasks that update metrics concurrently
        for i in 0..10 {
            let metrics_clone = metrics.clone();
            let handle = task::spawn(async move {
                metrics_clone.record_cache_hit(&format!("crate_{}", i));
                metrics_clone.record_query_duration(
                    "test",
                    &format!("op_{}", i),
                    Duration::from_millis(i as u64 * 10),
                );
                
                if i % 2 == 0 {
                    metrics_clone.set_cache_entries_total(i as f64);
                }
            });
            handles.push(handle);
        }
        
        // Wait for all tasks to complete
        for handle in handles {
            handle.await.unwrap();
        }
        
        // Verify metrics were updated (no specific values checked due to race conditions)
        let health = metrics.get_health_status().read().await;
        assert!(health.metrics.contains_key("cache_entries"));
    }
}