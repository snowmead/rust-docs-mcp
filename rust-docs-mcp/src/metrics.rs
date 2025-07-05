use std::sync::Arc;
use std::time::Duration;
use anyhow::Result;
use axum::{
    extract::State,
    http::StatusCode,
    response::Json,
    routing::get,
    Router,
};
use axum_prometheus::PrometheusMetricLayer;
use prometheus::{
    register_counter_vec, register_gauge, register_histogram, register_int_counter_vec,
    CounterVec, Gauge, Histogram, IntCounterVec,
};
use serde_json::json;
use tokio::sync::RwLock;

use crate::health::HealthStatus;

#[derive(Clone)]
pub struct MetricsServer {
    // Cache Performance Metrics
    cache_hits: IntCounterVec,
    cache_misses: IntCounterVec,
    cache_size_bytes: Gauge,
    cache_entries_total: Gauge,
    
    // Query Performance Metrics
    query_duration: Histogram,
    query_total: IntCounterVec,
    query_errors: IntCounterVec,
    
    // System Health Metrics
    memory_usage_bytes: Gauge,
    disk_usage_bytes: Gauge,
    uptime_seconds: Gauge,
    
    // Health status
    health_status: Arc<RwLock<HealthStatus>>,
}

impl MetricsServer {
    pub fn new() -> Result<Self> {
        let cache_hits = register_int_counter_vec!(
            "rust_docs_mcp_cache_hits_total",
            "Total number of cache hits by crate",
            &["crate_name"]
        )?;
        
        let cache_misses = register_int_counter_vec!(
            "rust_docs_mcp_cache_misses_total",
            "Total number of cache misses by crate",
            &["crate_name"]
        )?;
        
        let cache_size_bytes = register_gauge!(
            "rust_docs_mcp_cache_size_bytes",
            "Total cache size in bytes"
        )?;
        
        let cache_entries_total = register_gauge!(
            "rust_docs_mcp_cache_entries_total",
            "Total number of cached crates"
        )?;
        
        let query_duration = register_histogram!(
            "rust_docs_mcp_query_duration_seconds",
            "Query response time in seconds"
        )?;
        
        let query_total = register_int_counter_vec!(
            "rust_docs_mcp_query_total",
            "Total number of queries by tool/operation type",
            &["tool", "operation"]
        )?;
        
        let query_errors = register_int_counter_vec!(
            "rust_docs_mcp_query_errors_total",
            "Total number of query errors by type",
            &["error_type"]
        )?;
        
        let memory_usage_bytes = register_gauge!(
            "rust_docs_mcp_memory_usage_bytes",
            "Current memory usage in bytes"
        )?;
        
        let disk_usage_bytes = register_gauge!(
            "rust_docs_mcp_disk_usage_bytes",
            "Current cache disk usage in bytes"
        )?;
        
        let uptime_seconds = register_gauge!(
            "rust_docs_mcp_uptime_seconds",
            "Service uptime in seconds"
        )?;
        
        Ok(Self {
            cache_hits,
            cache_misses,
            cache_size_bytes,
            cache_entries_total,
            query_duration,
            query_total,
            query_errors,
            memory_usage_bytes,
            disk_usage_bytes,
            uptime_seconds,
            health_status: Arc::new(RwLock::new(HealthStatus::new())),
        })
    }
    
    pub async fn start_server(&self, addr: &str) -> Result<()> {
        let (prometheus_layer, metric_handle) = PrometheusMetricLayer::pair();
        
        let app = Router::new()
            .route("/metrics", get(move || async move { metric_handle.render() }))
            .route("/healthz", get(health_check))
            .route("/health", get(health_check))
            .layer(prometheus_layer)
            .with_state(self.health_status.clone());
        
        let listener = tokio::net::TcpListener::bind(addr).await?;
        tracing::info!("Metrics server listening on {}", addr);
        
        axum::serve(listener, app).await?;
        Ok(())
    }
    
    // Cache metrics
    pub fn record_cache_hit(&self, crate_name: &str) {
        self.cache_hits.with_label_values(&[crate_name]).inc();
    }
    
    pub fn record_cache_miss(&self, crate_name: &str) {
        self.cache_misses.with_label_values(&[crate_name]).inc();
    }
    
    pub fn set_cache_size_bytes(&self, size: f64) {
        self.cache_size_bytes.set(size);
    }
    
    pub fn set_cache_entries_total(&self, count: f64) {
        self.cache_entries_total.set(count);
    }
    
    // Query metrics
    pub fn record_query_duration(&self, tool: &str, operation: &str, duration: Duration) {
        self.query_duration.observe(duration.as_secs_f64());
        self.query_total.with_label_values(&[tool, operation]).inc();
    }
    
    pub fn record_query_error(&self, error_type: &str) {
        self.query_errors.with_label_values(&[error_type]).inc();
    }
    
    // System metrics
    pub fn set_memory_usage_bytes(&self, usage: f64) {
        self.memory_usage_bytes.set(usage);
    }
    
    pub fn set_disk_usage_bytes(&self, usage: f64) {
        self.disk_usage_bytes.set(usage);
    }
    
    pub fn set_uptime_seconds(&self, uptime: f64) {
        self.uptime_seconds.set(uptime);
    }
    
    // Health status access
    pub fn get_health_status(&self) -> Arc<RwLock<HealthStatus>> {
        self.health_status.clone()
    }
}

async fn health_check(
    State(health): State<Arc<RwLock<HealthStatus>>>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let status = health.read().await;
    
    if status.is_healthy() {
        Ok(Json(json!({
            "status": "healthy",
            "timestamp": chrono::Utc::now().to_rfc3339(),
            "version": env!("CARGO_PKG_VERSION"),
            "checks": status.checks,
            "metrics": status.metrics
        })))
    } else {
        Err(StatusCode::SERVICE_UNAVAILABLE)
    }
}