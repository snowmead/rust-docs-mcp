//! Task manager for async caching operations
//!
//! This module provides task tracking and management for background caching operations.
//! Each caching operation gets a unique task ID and can be monitored, cancelled, or cleared.

use dashmap::DashMap;
use std::sync::Arc;
use std::time::SystemTime;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

/// Unique identifier for a caching task
pub type TaskId = String;

/// Status of a caching task
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TaskStatus {
    /// Task is queued but not yet started
    Pending,
    /// Task is currently executing
    InProgress,
    /// Task completed successfully
    Completed,
    /// Task failed with an error
    Failed,
    /// Task was cancelled by user request
    Cancelled,
}

impl TaskStatus {
    /// Convert status to string representation
    pub fn as_str(&self) -> &'static str {
        match self {
            TaskStatus::Pending => "pending",
            TaskStatus::InProgress => "in_progress",
            TaskStatus::Completed => "completed",
            TaskStatus::Failed => "failed",
            TaskStatus::Cancelled => "cancelled",
        }
    }

    /// Convert status to display string
    pub fn display(&self) -> &'static str {
        match self {
            TaskStatus::Pending => "PENDING",
            TaskStatus::InProgress => "IN PROGRESS",
            TaskStatus::Completed => "COMPLETED ✓",
            TaskStatus::Failed => "FAILED ✗",
            TaskStatus::Cancelled => "CANCELLED",
        }
    }
}

/// Current stage of a caching operation
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CachingStage {
    /// Downloading crate source code
    Downloading,
    /// Generating JSON documentation via cargo rustdoc
    GeneratingDocs,
    /// Creating search index
    Indexing,
    /// Operation completed
    Completed,
}

impl CachingStage {
    /// Convert stage to string representation
    pub fn as_str(&self) -> &'static str {
        match self {
            CachingStage::Downloading => "downloading",
            CachingStage::GeneratingDocs => "generating_docs",
            CachingStage::Indexing => "indexing",
            CachingStage::Completed => "completed",
        }
    }

    /// Get human-readable description of the stage
    pub fn description(&self) -> &'static str {
        match self {
            CachingStage::Downloading => "Downloading crate source code",
            CachingStage::GeneratingDocs => "Generating JSON documentation using cargo rustdoc",
            CachingStage::Indexing => "Creating search index",
            CachingStage::Completed => "Operation completed",
        }
    }

    /// Get total number of steps for this stage
    pub fn total_steps(&self) -> u8 {
        match self {
            CachingStage::Downloading => 1,     // Single step: download
            CachingStage::GeneratingDocs => 2,  // 1. Run rustdoc, 2. Save docs
            CachingStage::Indexing => 3,        // 1. Load docs, 2. Create index, 3. Commit
            CachingStage::Completed => 1,       // Single step: complete
        }
    }
}

/// Information about a caching task
#[derive(Debug, Clone)]
pub struct CachingTask {
    /// Unique task identifier
    pub task_id: TaskId,
    /// Name of the crate being cached
    pub crate_name: String,
    /// Version of the crate
    pub version: String,
    /// Source type (cratesio, github, local)
    pub source_type: String,
    /// Optional source details (e.g., GitHub URL, local path)
    pub source_details: Option<String>,
    /// Current status
    pub status: TaskStatus,
    /// Current stage (if in progress)
    pub stage: Option<CachingStage>,
    /// Current step within the stage (1-based)
    pub current_step: Option<u8>,
    /// Description of the current step
    pub step_description: Option<String>,
    /// When the task started
    pub started_at: SystemTime,
    /// When the task completed (success, failure, or cancellation)
    pub completed_at: Option<SystemTime>,
    /// Error message if failed
    pub error: Option<String>,
    /// Token to signal cancellation
    pub cancellation_token: CancellationToken,
}

impl CachingTask {
    /// Create a new caching task
    pub fn new(
        crate_name: String,
        version: String,
        source_type: String,
        source_details: Option<String>,
    ) -> Self {
        Self {
            task_id: Uuid::new_v4().to_string(),
            crate_name,
            version,
            source_type,
            source_details,
            status: TaskStatus::Pending,
            stage: None,
            current_step: None,
            step_description: None,
            started_at: SystemTime::now(),
            completed_at: None,
            error: None,
            cancellation_token: CancellationToken::new(),
        }
    }

    /// Get elapsed time in seconds
    pub fn elapsed_secs(&self) -> u64 {
        let end_time = self.completed_at.unwrap_or_else(SystemTime::now);
        end_time
            .duration_since(self.started_at)
            .unwrap_or_default()
            .as_secs()
    }

    /// Update task status
    pub fn set_status(&mut self, status: TaskStatus) {
        self.status = status;
        if matches!(
            status,
            TaskStatus::Completed | TaskStatus::Failed | TaskStatus::Cancelled
        ) {
            self.completed_at = Some(SystemTime::now());
        }
    }

    /// Update task stage
    pub fn set_stage(&mut self, stage: CachingStage) {
        self.stage = Some(stage);
        // Reset step tracking when changing stages
        self.current_step = Some(1); // Start at step 1
        self.step_description = None;
        if self.status == TaskStatus::Pending {
            self.status = TaskStatus::InProgress;
        }
    }

    /// Update current step within the stage
    pub fn set_step(&mut self, step: u8, description: impl Into<String>) {
        self.current_step = Some(step);
        self.step_description = Some(description.into());
    }

    /// Set error and mark as failed
    pub fn set_error(&mut self, error: String) {
        self.error = Some(error);
        self.set_status(TaskStatus::Failed);
    }

    /// Check if task is terminal (completed, failed, or cancelled)
    pub fn is_terminal(&self) -> bool {
        matches!(
            self.status,
            TaskStatus::Completed | TaskStatus::Failed | TaskStatus::Cancelled
        )
    }
}

/// Manager for caching tasks
#[derive(Debug, Clone)]
pub struct TaskManager {
    /// Map of task IDs to tasks using DashMap for concurrent access
    tasks: Arc<DashMap<TaskId, CachingTask>>,
}

impl TaskManager {
    /// Create a new task manager
    pub fn new() -> Self {
        Self {
            tasks: Arc::new(DashMap::new()),
        }
    }

    /// Create and register a new task
    pub async fn create_task(
        &self,
        crate_name: String,
        version: String,
        source_type: String,
        source_details: Option<String>,
    ) -> CachingTask {
        let task = CachingTask::new(crate_name, version, source_type, source_details);
        self.tasks.insert(task.task_id.clone(), task.clone());
        task
    }

    /// Get a task by ID
    pub async fn get_task(&self, task_id: &str) -> Option<CachingTask> {
        self.tasks.get(task_id).map(|r| r.clone())
    }

    /// List all tasks, optionally filtered by status
    pub async fn list_tasks(&self, status_filter: Option<&TaskStatus>) -> Vec<CachingTask> {
        let mut result: Vec<_> = self
            .tasks
            .iter()
            .filter(|entry| {
                status_filter
                    .map(|filter| &entry.value().status == filter)
                    .unwrap_or(true)
            })
            .map(|entry| entry.value().clone())
            .collect();

        // Sort by started_at descending (newest first)
        result.sort_by(|a, b| b.started_at.cmp(&a.started_at));
        result
    }

    /// Update task status
    pub async fn update_status(&self, task_id: &str, status: TaskStatus) -> bool {
        if let Some(mut task) = self.tasks.get_mut(task_id) {
            task.set_status(status);
            true
        } else {
            false
        }
    }

    /// Update task stage
    pub async fn update_stage(&self, task_id: &str, stage: CachingStage) -> bool {
        if let Some(mut task) = self.tasks.get_mut(task_id) {
            task.set_stage(stage);
            true
        } else {
            false
        }
    }

    /// Update task step
    pub async fn update_step(&self, task_id: &str, step: u8, description: impl Into<String>) -> bool {
        if let Some(mut task) = self.tasks.get_mut(task_id) {
            task.set_step(step, description);
            true
        } else {
            false
        }
    }

    /// Set task error and mark as failed
    pub async fn set_error(&self, task_id: &str, error: String) -> bool {
        if let Some(mut task) = self.tasks.get_mut(task_id) {
            task.set_error(error);
            true
        } else {
            false
        }
    }

    /// Cancel a task
    pub async fn cancel_task(&self, task_id: &str) -> Option<CachingTask> {
        if let Some(mut task) = self.tasks.get_mut(task_id) {
            // Only cancel if not already terminal
            if !task.is_terminal() {
                task.cancellation_token.cancel();
                task.set_status(TaskStatus::Cancelled);
                Some(task.clone())
            } else {
                Some(task.clone())
            }
        } else {
            None
        }
    }

    /// Remove a task from the manager
    pub async fn remove_task(&self, task_id: &str) -> Option<CachingTask> {
        self.tasks.remove(task_id).map(|(_, task)| task)
    }

    /// Remove all terminal tasks (completed, failed, cancelled)
    pub async fn clear_terminal_tasks(&self) -> Vec<CachingTask> {
        let terminal_tasks: Vec<_> = self
            .tasks
            .iter()
            .filter(|entry| entry.value().is_terminal())
            .map(|entry| (entry.key().clone(), entry.value().clone()))
            .collect();

        terminal_tasks
            .into_iter()
            .filter_map(|(id, task)| self.tasks.remove(&id).map(|_| task))
            .collect()
    }

    /// Get task count by status
    pub async fn count_by_status(&self) -> HashMap<TaskStatus, usize> {
        let mut counts = HashMap::new();
        for entry in self.tasks.iter() {
            *counts.entry(entry.value().status).or_insert(0) += 1;
        }
        counts
    }
}

impl Default for TaskManager {
    fn default() -> Self {
        Self::new()
    }
}
