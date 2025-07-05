#[cfg(test)]
mod tests {
    use super::super::health::*;
    use std::collections::HashMap;
    use std::path::Path;
    use tempfile::TempDir;

    #[test]
    fn test_health_status_new() {
        let health = HealthStatus::new();
        assert!(health.checks.is_empty());
        assert!(health.metrics.is_empty());
    }

    #[test]
    fn test_health_status_clone() {
        let health = HealthStatus::new();
        let cloned = health.clone();
        assert_eq!(health.checks.len(), cloned.checks.len());
        assert_eq!(health.metrics.len(), cloned.metrics.len());
    }

    #[test]
    fn test_is_healthy() {
        let mut health = HealthStatus::new();
        
        // Should be healthy when no checks
        assert!(health.is_healthy());
        
        // Add healthy check
        let mut healthy_check = HealthCheck {
            status: "healthy".to_string(),
            details: HashMap::new(),
        };
        healthy_check.details.insert("test".to_string(), serde_json::Value::Bool(true));
        health.checks.insert("test_check".to_string(), healthy_check);
        assert!(health.is_healthy());
        
        // Add unhealthy check
        let unhealthy_check = HealthCheck {
            status: "unhealthy".to_string(),
            details: HashMap::new(),
        };
        health.checks.insert("failing_check".to_string(), unhealthy_check);
        assert!(!health.is_healthy());
    }

    #[test]
    fn test_set_cache_metrics() {
        let mut health = HealthStatus::new();
        health.set_cache_metrics(10, 1024);
        
        assert_eq!(
            health.metrics.get("cache_entries").unwrap(),
            &serde_json::Value::Number(serde_json::Number::from(10))
        );
        assert_eq!(
            health.metrics.get("cache_size_mb").unwrap(),
            &serde_json::Value::Number(serde_json::Number::from(1024))
        );
    }

    #[tokio::test]
    async fn test_update_metrics() {
        let mut health = HealthStatus::new();
        health.update_metrics().await.unwrap();
        
        // Check uptime is set
        assert!(health.metrics.contains_key("uptime_seconds"));
        let uptime = health.metrics.get("uptime_seconds").unwrap();
        assert!(uptime.is_number());
        
        // Check default cache metrics
        assert_eq!(
            health.metrics.get("cache_entries").unwrap(),
            &serde_json::Value::Number(serde_json::Number::from(0))
        );
        assert_eq!(
            health.metrics.get("cache_size_mb").unwrap(),
            &serde_json::Value::Number(serde_json::Number::from(0))
        );
    }

    #[tokio::test]
    async fn test_cache_directory_check_with_temp_dir() {
        let mut health = HealthStatus::new();
        let temp_dir = TempDir::new().unwrap();
        
        health.update_cache_directory_check(Some(temp_dir.path())).await.unwrap();
        
        let check = health.checks.get("cache_directory").unwrap();
        assert_eq!(check.status, "healthy");
        assert_eq!(check.details.get("writable").unwrap(), &serde_json::Value::Bool(true));
        assert!(check.details.contains_key("available_space_mb"));
    }

    #[tokio::test]
    async fn test_cache_directory_check_nonexistent() {
        let mut health = HealthStatus::new();
        let temp_dir = TempDir::new().unwrap();
        let nonexistent = temp_dir.path().join("nonexistent");
        
        health.update_cache_directory_check(Some(&nonexistent)).await.unwrap();
        
        let check = health.checks.get("cache_directory").unwrap();
        // Should create the directory
        assert_eq!(check.status, "healthy");
        assert_eq!(check.details.get("writable").unwrap(), &serde_json::Value::Bool(true));
    }

    #[test]
    fn test_get_available_space() {
        let temp_dir = TempDir::new().unwrap();
        let space = super::super::health::get_available_space(temp_dir.path()).unwrap();
        // Should return a value >= 0
        assert!(space >= 0);
    }

    #[tokio::test]
    async fn test_network_check() {
        let mut health = HealthStatus::new();
        
        // This test might fail if no network is available
        let result = health.update_network_check().await;
        assert!(result.is_ok());
        
        let check = health.checks.get("network").unwrap();
        // Status should be either healthy or degraded
        assert!(check.status == "healthy" || check.status == "degraded");
        assert!(check.details.contains_key("crates_io_reachable"));
    }

    #[tokio::test]
    async fn test_rust_toolchain_check() {
        let mut health = HealthStatus::new();
        
        health.update_rust_toolchain_check().await.unwrap();
        
        let check = health.checks.get("rust_toolchain").unwrap();
        // Should have status
        assert!(!check.status.is_empty());
        assert!(check.details.contains_key("nightly_available"));
        assert!(check.details.contains_key("rustdoc_functional"));
    }
}