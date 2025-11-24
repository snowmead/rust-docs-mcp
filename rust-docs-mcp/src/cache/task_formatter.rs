//! Markdown formatting for caching task output
//!
//! This module provides rich markdown formatting for task status and operations,
//! optimized for LLM (AI agent) consumption. All output is designed to be clear,
//! scannable, and include actionable commands.

use super::task_manager::{CachingTask, TaskStatus};
use chrono::{DateTime, Utc};
use std::time::SystemTime;

/// Format a timestamp as ISO 8601 string
fn format_timestamp(time: SystemTime) -> String {
    let datetime: DateTime<Utc> = time.into();
    datetime.format("%Y-%m-%d %H:%M:%S UTC").to_string()
}

/// Format duration in seconds to human-readable string
fn format_duration(secs: u64) -> String {
    if secs < 60 {
        format!("{secs}s")
    } else if secs < 3600 {
        let mins = secs / 60;
        let remaining_secs = secs % 60;
        if remaining_secs == 0 {
            format!("{mins}m")
        } else {
            format!("{mins}m {remaining_secs}s")
        }
    } else {
        let hours = secs / 3600;
        let remaining_mins = (secs % 3600) / 60;
        if remaining_mins == 0 {
            format!("{hours}h")
        } else {
            format!("{hours}h {remaining_mins}m")
        }
    }
}

/// Format the cache_crate tool result when a task is started
pub fn format_task_started(task: &CachingTask) -> String {
    let source_info = if let Some(details) = &task.source_details {
        format!(" ({details})")
    } else {
        String::new()
    };

    format!(
        r#"# Caching Started

**Crate**: {}-{}
**Source**: {}{}
**Task ID**: `{}`

The caching operation is running in the background.

**To check status**: `cache_operations({{task_id: "{}"}})`
**To cancel**: `cache_operations({{task_id: "{}", cancel: true}})`
"#,
        task.crate_name,
        task.version,
        task.source_type,
        source_info,
        task.task_id,
        task.task_id,
        task.task_id
    )
}

/// Format a single task with full details
pub fn format_single_task(task: &CachingTask) -> String {
    let source_info = if let Some(details) = &task.source_details {
        format!(" ({details})")
    } else {
        String::new()
    };

    match task.status {
        TaskStatus::InProgress | TaskStatus::Pending => {
            let stage_info = if let Some(stage) = &task.stage {
                let step_str = if let Some(step) = task.current_step {
                    let total = stage.total_steps();
                    let desc = task
                        .step_description
                        .as_ref()
                        .map(|d| format!(": {d}"))
                        .unwrap_or_default();
                    format!("\n**Step**: {step} of {total}{desc}")
                } else {
                    String::new()
                };
                format!(
                    "**Current Stage**: {}{}\n\n## Progress Details\n{}",
                    stage.description(),
                    step_str,
                    get_stage_context(stage.as_str())
                )
            } else {
                "**Current Stage**: Initializing".to_string()
            };

            format!(
                r#"# Caching Task: `{}`

**Crate**: {}-{}
**Source**: {}{}
**Status**: {}
{}

## Timeline
- **Started**: {}
- **Elapsed Time**: {}

## Available Actions
- **Cancel this task**: `cache_operations({{task_id: "{}", cancel: true}})`
"#,
                task.task_id,
                task.crate_name,
                task.version,
                task.source_type,
                source_info,
                task.status.display(),
                stage_info,
                format_timestamp(task.started_at),
                format_duration(task.elapsed_secs()),
                task.task_id
            )
        }
        TaskStatus::Completed => format!(
            r#"# Caching Task: `{}`

**Crate**: {}-{}
**Source**: {}{}
**Status**: {}

## Timeline
- **Started**: {}
- **Completed**: {}
- **Total Duration**: {}

## Cache Details
The crate has been successfully cached and documentation is available. You can now use other tools like `search_items_preview` and `get_item_details` to query this crate's documentation.

## Available Actions
- **Clear this task**: `cache_operations({{task_id: "{}", clear: true}})`
"#,
            task.task_id,
            task.crate_name,
            task.version,
            task.source_type,
            source_info,
            task.status.display(),
            format_timestamp(task.started_at),
            format_timestamp(task.completed_at.unwrap_or(task.started_at)),
            format_duration(task.elapsed_secs()),
            task.task_id
        ),
        TaskStatus::Failed => {
            let error_msg = task
                .error
                .as_ref()
                .map(|e| format!("```\n{e}\n```"))
                .unwrap_or_else(|| "Unknown error".to_string());

            format!(
                r#"# Caching Task: `{}`

**Crate**: {}-{}
**Source**: {}{}
**Status**: {}

## Timeline
- **Started**: {}
- **Failed**: {}
- **Duration**: {}

## Error Details
{}

## Next Steps
- Verify the crate name and version are correct
- Check if the crate exists in the specified source
- Review the error message for specific issues
- Consider using a different source (github or local)

## Available Actions
- **Clear this task**: `cache_operations({{task_id: "{}", clear: true}})`
"#,
                task.task_id,
                task.crate_name,
                task.version,
                task.source_type,
                source_info,
                task.status.display(),
                format_timestamp(task.started_at),
                format_timestamp(task.completed_at.unwrap_or(task.started_at)),
                format_duration(task.elapsed_secs()),
                error_msg,
                task.task_id
            )
        }
        TaskStatus::Cancelled => format!(
            r#"# Caching Task: `{}`

**Crate**: {}-{}
**Source**: {}{}
**Status**: {}

## Timeline
- **Started**: {}
- **Cancelled**: {}
- **Duration**: {}

The caching operation was cancelled. Any partially downloaded or generated files have been cleaned up.

## Available Actions
- **Clear this task**: `cache_operations({{task_id: "{}", clear: true}})`
"#,
            task.task_id,
            task.crate_name,
            task.version,
            task.source_type,
            source_info,
            task.status.display(),
            format_timestamp(task.started_at),
            format_timestamp(task.completed_at.unwrap_or(task.started_at)),
            format_duration(task.elapsed_secs()),
            task.task_id
        ),
    }
}

/// Get contextual information about a caching stage
fn get_stage_context(stage: &str) -> &'static str {
    match stage {
        "downloading" => {
            "The caching operation is downloading the crate source code. This stage typically takes 10-60 seconds depending on crate size and network speed."
        }
        "generating_docs" => {
            "The caching operation is generating JSON documentation using `cargo rustdoc`. This stage typically takes 2-5 minutes for large crates, less for smaller ones."
        }
        "indexing" => {
            "The caching operation is creating a search index for fast lookups. This stage typically takes 10-30 seconds."
        }
        _ => "The caching operation is in progress.",
    }
}

/// Format task list with grouping by status
pub fn format_task_list(tasks: Vec<CachingTask>) -> String {
    // Group tasks by status
    let mut in_progress = Vec::new();
    let mut completed = Vec::new();
    let mut failed = Vec::new();
    let mut cancelled = Vec::new();
    let mut pending = Vec::new();

    for task in tasks {
        match task.status {
            TaskStatus::InProgress => in_progress.push(task),
            TaskStatus::Completed => completed.push(task),
            TaskStatus::Failed => failed.push(task),
            TaskStatus::Cancelled => cancelled.push(task),
            TaskStatus::Pending => pending.push(task),
        }
    }

    let total =
        in_progress.len() + completed.len() + failed.len() + cancelled.len() + pending.len();

    // Build summary
    let mut output = String::from("# Caching Operations\n\n");
    output.push_str("## Summary\n");
    output.push_str(&format!("- **Total Operations**: {total}\n"));
    if !in_progress.is_empty() {
        output.push_str(&format!("- **In Progress**: {}\n", in_progress.len()));
    }
    if !pending.is_empty() {
        output.push_str(&format!("- **Pending**: {}\n", pending.len()));
    }
    if !completed.is_empty() {
        output.push_str(&format!("- **Completed**: {}\n", completed.len()));
    }
    if !failed.is_empty() {
        output.push_str(&format!("- **Failed**: {}\n", failed.len()));
    }
    if !cancelled.is_empty() {
        output.push_str(&format!("- **Cancelled**: {}\n", cancelled.len()));
    }

    if total == 0 {
        output.push_str("\nNo caching operations found.\n");
        return output;
    }

    output.push_str("\n---\n");

    // Format each group
    if !in_progress.is_empty() {
        output.push_str(&format!("\n## In Progress ({})\n\n", in_progress.len()));
        for task in &in_progress {
            output.push_str(&format_task_summary(task));
            output.push_str("\n---\n");
        }
    }

    if !pending.is_empty() {
        output.push_str(&format!("\n## Pending ({})\n\n", pending.len()));
        for task in &pending {
            output.push_str(&format_task_summary(task));
            output.push_str("\n---\n");
        }
    }

    if !completed.is_empty() {
        output.push_str(&format!("\n## Completed ({})\n\n", completed.len()));
        for task in &completed {
            output.push_str(&format_task_summary(task));
            output.push_str("\n---\n");
        }
    }

    if !failed.is_empty() {
        output.push_str(&format!("\n## Failed ({})\n\n", failed.len()));
        for task in &failed {
            output.push_str(&format_task_summary(task));
            output.push_str("\n---\n");
        }
    }

    if !cancelled.is_empty() {
        output.push_str(&format!("\n## Cancelled ({})\n\n", cancelled.len()));
        for task in &cancelled {
            output.push_str(&format_task_summary(task));
            output.push_str("\n---\n");
        }
    }

    // Add quick actions section
    output.push_str("\n## Quick Actions\n");
    if !in_progress.is_empty() {
        output.push_str("- **Cancel tasks**: Use `cache_operations` with task_id and cancel: true for each task\n");
    }
    if !completed.is_empty() || !failed.is_empty() || !cancelled.is_empty() {
        output.push_str(
            "- **Clear all completed/failed/cancelled**: `cache_operations({clear: true})`\n",
        );
    }

    output
}

/// Format a concise task summary for list view
fn format_task_summary(task: &CachingTask) -> String {
    let source_info = if let Some(details) = &task.source_details {
        format!(" ({details})")
    } else {
        String::new()
    };

    let mut output = format!(
        "### Task: `{}`\n**Crate**: {}-{}  \n**Source**: {}{}  \n**Status**: {}  \n",
        task.task_id,
        task.crate_name,
        task.version,
        task.source_type,
        source_info,
        task.status.display()
    );

    match task.status {
        TaskStatus::InProgress => {
            if let Some(stage) = &task.stage {
                output.push_str(&format!("**Stage**: {}  \n", stage.description()));

                if let Some(step) = task.current_step {
                    let total = stage.total_steps();
                    let desc = task
                        .step_description
                        .as_ref()
                        .map(|d| format!(": {d}"))
                        .unwrap_or_default();
                    output.push_str(&format!("**Step**: {step} of {total}{desc}  \n"));
                }
            }
            output.push_str(&format!(
                "**Started**: {}  \n",
                format_timestamp(task.started_at)
            ));
            output.push_str(&format!(
                "**Elapsed**: {}\n\n",
                format_duration(task.elapsed_secs())
            ));
            output.push_str("**Actions**:\n");
            output.push_str(&format!(
                "- Cancel: `cache_operations({{task_id: \"{}\", cancel: true}})`\n",
                task.task_id
            ));
        }
        TaskStatus::Pending => {
            output.push_str(&format!(
                "**Started**: {}  \n",
                format_timestamp(task.started_at)
            ));
            output.push_str("\n**Actions**:\n");
            output.push_str(&format!(
                "- Cancel: `cache_operations({{task_id: \"{}\", cancel: true}})`\n",
                task.task_id
            ));
        }
        TaskStatus::Completed => {
            output.push_str(&format!(
                "**Duration**: {}  \n",
                format_duration(task.elapsed_secs())
            ));
            output.push_str(&format!(
                "**Completed**: {}\n\n",
                format_timestamp(task.completed_at.unwrap_or(task.started_at))
            ));
            output.push_str("**Actions**:\n");
            output.push_str(&format!(
                "- Clear: `cache_operations({{task_id: \"{}\", clear: true}})`\n",
                task.task_id
            ));
        }
        TaskStatus::Failed => {
            output.push_str(&format!(
                "**Duration**: {}  \n",
                format_duration(task.elapsed_secs())
            ));
            if let Some(error) = &task.error {
                // Truncate long errors for list view
                let error_preview = if error.len() > 100 {
                    format!("{}...", &error[..100])
                } else {
                    error.clone()
                };
                output.push_str(&format!("**Error**: {error_preview}\n\n"));
            }
            output.push_str("**Actions**:\n");
            output.push_str(&format!(
                "- View details: `cache_operations({{task_id: \"{}\"}})`\n",
                task.task_id
            ));
            output.push_str(&format!(
                "- Clear: `cache_operations({{task_id: \"{}\", clear: true}})`\n",
                task.task_id
            ));
        }
        TaskStatus::Cancelled => {
            output.push_str(&format!(
                "**Duration**: {}  \n",
                format_duration(task.elapsed_secs())
            ));
            output.push_str(&format!(
                "**Cancelled**: {}\n\n",
                format_timestamp(task.completed_at.unwrap_or(task.started_at))
            ));
            output.push_str("**Actions**:\n");
            output.push_str(&format!(
                "- Clear: `cache_operations({{task_id: \"{}\", clear: true}})`\n",
                task.task_id
            ));
        }
    }

    output
}

/// Format cancel result
pub fn format_cancel_result(task: &CachingTask) -> String {
    format!(
        r#"# Task Cancelled

**Task ID**: `{}`
**Crate**: {}-{}

The caching operation has been successfully cancelled. Any partially downloaded or generated files have been cleaned up.
"#,
        task.task_id, task.crate_name, task.version
    )
}

/// Format clear result
pub fn format_clear_result(tasks: Vec<CachingTask>) -> String {
    if tasks.is_empty() {
        return "# No Tasks Cleared\n\nNo completed, failed, or cancelled tasks were found to clear.".to_string();
    }

    let mut output = String::from("# Tasks Cleared\n\n");
    output.push_str(&format!(
        "Successfully cleared {} task(s) from memory:\n\n",
        tasks.len()
    ));

    for task in tasks {
        let status_str = match task.status {
            TaskStatus::Completed => "completed",
            TaskStatus::Failed => "failed",
            TaskStatus::Cancelled => "cancelled",
            _ => "unknown",
        };
        output.push_str(&format!(
            "- `{}` ({}-{} - {})\n",
            task.task_id, task.crate_name, task.version, status_str
        ));
    }

    output
}
