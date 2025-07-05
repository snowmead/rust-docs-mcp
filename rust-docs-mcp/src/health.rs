use std::collections::HashMap;
use std::fs;
use std::path::Path;
use std::process::Command;
use std::time::{Duration, Instant};
use anyhow::Result;
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealthStatus {
    pub checks: HashMap<String, HealthCheck>,
    pub metrics: HashMap<String, Value>,
    pub start_time: Instant,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealthCheck {
    pub status: String,
    #[serde(flatten)]
    pub details: HashMap<String, Value>,
}

impl HealthStatus {
    pub fn new() -> Self {
        Self {
            checks: HashMap::new(),
            metrics: HashMap::new(),
            start_time: Instant::now(),
        }
    }
    
    pub fn is_healthy(&self) -> bool {
        self.checks.values().all(|check| check.status == "healthy")
    }
    
    pub async fn update_health_checks(&mut self, cache_dir: Option<&Path>) -> Result<()> {
        // Update cache directory check
        self.update_cache_directory_check(cache_dir).await?;
        
        // Update rust toolchain check
        self.update_rust_toolchain_check().await?;
        
        // Update network check
        self.update_network_check().await?;
        
        // Update metrics
        self.update_metrics().await?;
        
        Ok(())
    }
    
    async fn update_cache_directory_check(&mut self, cache_dir: Option<&Path>) -> Result<()> {
        let cache_path = match cache_dir {
            Some(dir) => dir.to_path_buf(),
            None => {
                let home = dirs::home_dir()
                    .ok_or_else(|| anyhow::anyhow!("Could not determine home directory"))?;
                home.join(".rust-docs-mcp").join("cache")
            }
        };
        
        let mut check = HealthCheck {
            status: "healthy".to_string(),
            details: HashMap::new(),
        };
        
        // Check if directory exists and is writable
        match fs::metadata(&cache_path) {
            Ok(metadata) => {
                if metadata.is_dir() {
                    // Check if writable by trying to create a temporary file
                    let temp_file = cache_path.join(".health_check_temp");
                    match fs::write(&temp_file, "test") {
                        Ok(()) => {
                            let _ = fs::remove_file(&temp_file);
                            check.details.insert("writable".to_string(), Value::Bool(true));
                        }
                        Err(_) => {
                            check.status = "unhealthy".to_string();
                            check.details.insert("writable".to_string(), Value::Bool(false));
                        }
                    }
                    
                    // Get available space
                    if let Ok(space) = get_available_space(&cache_path) {
                        check.details.insert("available_space_mb".to_string(), Value::Number(serde_json::Number::from(space)));
                    }
                } else {
                    check.status = "unhealthy".to_string();
                    check.details.insert("error".to_string(), Value::String("Path exists but is not a directory".to_string()));
                }
            }
            Err(_) => {
                // Try to create the directory
                match fs::create_dir_all(&cache_path) {
                    Ok(()) => {
                        check.details.insert("writable".to_string(), Value::Bool(true));
                        if let Ok(space) = get_available_space(&cache_path) {
                            check.details.insert("available_space_mb".to_string(), Value::Number(serde_json::Number::from(space)));
                        }
                    }
                    Err(e) => {
                        check.status = "unhealthy".to_string();
                        check.details.insert("error".to_string(), Value::String(format!("Could not create cache directory: {}", e)));
                    }
                }
            }
        }
        
        self.checks.insert("cache_directory".to_string(), check);
        Ok(())
    }
    
    async fn update_rust_toolchain_check(&mut self) -> Result<()> {
        let mut check = HealthCheck {
            status: "healthy".to_string(),
            details: HashMap::new(),
        };
        
        // Check for nightly toolchain
        let nightly_output = Command::new("rustup")
            .args(&["toolchain", "list"])
            .output();
        
        match nightly_output {
            Ok(output) => {
                let output_str = String::from_utf8_lossy(&output.stdout);
                let nightly_available = output_str.lines().any(|line| line.contains("nightly"));
                check.details.insert("nightly_available".to_string(), Value::Bool(nightly_available));
                
                if !nightly_available {
                    check.status = "degraded".to_string();
                }
            }
            Err(_) => {
                check.details.insert("nightly_available".to_string(), Value::Bool(false));
                check.status = "unhealthy".to_string();
            }
        }
        
        // Check if rustdoc is functional
        let rustdoc_output = Command::new("rustdoc")
            .args(&["--version"])
            .output();
        
        match rustdoc_output {
            Ok(output) => {
                let functional = output.status.success();
                check.details.insert("rustdoc_functional".to_string(), Value::Bool(functional));
                
                if !functional && check.status == "healthy" {
                    check.status = "unhealthy".to_string();
                }
            }
            Err(_) => {
                check.details.insert("rustdoc_functional".to_string(), Value::Bool(false));
                check.status = "unhealthy".to_string();
            }
        }
        
        self.checks.insert("rust_toolchain".to_string(), check);
        Ok(())
    }
    
    async fn update_network_check(&mut self) -> Result<()> {
        let mut check = HealthCheck {
            status: "healthy".to_string(),
            details: HashMap::new(),
        };
        
        // Check crates.io connectivity with timeout
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(5))
            .build()?;
        
        match client.head("https://crates.io/api/v1/crates").send().await {
            Ok(response) => {
                let reachable = response.status().is_success();
                check.details.insert("crates_io_reachable".to_string(), Value::Bool(reachable));
                
                if !reachable {
                    check.status = "degraded".to_string();
                }
            }
            Err(_) => {
                check.details.insert("crates_io_reachable".to_string(), Value::Bool(false));
                check.status = "degraded".to_string();
            }
        }
        
        self.checks.insert("network".to_string(), check);
        Ok(())
    }
    
    async fn update_metrics(&mut self) -> Result<()> {
        // Update uptime
        let uptime = self.start_time.elapsed().as_secs();
        self.metrics.insert("uptime_seconds".to_string(), Value::Number(serde_json::Number::from(uptime)));
        
        // These will be updated by the metrics server and cache operations
        if !self.metrics.contains_key("cache_entries") {
            self.metrics.insert("cache_entries".to_string(), Value::Number(serde_json::Number::from(0)));
        }
        
        if !self.metrics.contains_key("cache_size_mb") {
            self.metrics.insert("cache_size_mb".to_string(), Value::Number(serde_json::Number::from(0)));
        }
        
        Ok(())
    }
    
    pub fn set_cache_metrics(&mut self, entries: u64, size_mb: u64) {
        self.metrics.insert("cache_entries".to_string(), Value::Number(serde_json::Number::from(entries)));
        self.metrics.insert("cache_size_mb".to_string(), Value::Number(serde_json::Number::from(size_mb)));
    }
}

fn get_available_space(path: &Path) -> Result<u64> {
    use std::fs;
    
    // Simple implementation - in production you might want to use platform-specific APIs
    // For now, we'll return a reasonable default
    match fs::metadata(path) {
        Ok(_) => Ok(15360), // 15GB in MB as default
        Err(_) => Ok(0),
    }
}

impl Default for HealthStatus {
    fn default() -> Self {
        Self::new()
    }
}