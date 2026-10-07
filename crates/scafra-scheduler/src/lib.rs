//! Configurable scheduled task support for Scafra applications.

use std::{collections::BTreeMap, time::Duration};

use serde::{Deserialize, Serialize};

/// Configuration for the Scafra scheduler.
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct SchedulerConfig {
    /// Scheduler is opt-in; no jobs run unless this is enabled.
    #[serde(default)]
    pub enabled: bool,
    /// Per-task overrides keyed by the registered task name.
    #[serde(default)]
    pub tasks: BTreeMap<String, ScheduledTaskConfig>,
}

/// Configuration for one scheduled task.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ScheduledTaskConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// Interval in milliseconds. A missing value uses the task's registration default.
    #[serde(default)]
    pub interval_ms: Option<u64>,
}

impl Default for ScheduledTaskConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            interval_ms: None,
        }
    }
}

fn default_true() -> bool {
    true
}

/// Link-time registration emitted for scheduled jobs.
pub struct ScheduledTaskRegistration {
    pub name: &'static str,
    pub default_interval_ms: u64,
    pub task: fn(),
}

inventory::collect!(ScheduledTaskRegistration);
pub use inventory;

/// Registers a synchronous service-owned task.
#[macro_export]
macro_rules! register_scheduled_task {
    ($name:literal, $interval_ms:expr, $task:path) => {
        $crate::inventory::submit! {
            $crate::ScheduledTaskRegistration {
                name: $name,
                default_interval_ms: $interval_ms,
                task: $task,
            }
        }
    };
}

/// A running scheduler. Dropping it stops all scheduled tasks.
#[derive(Debug)]
pub struct Scheduler {
    tasks: Vec<tokio::task::JoinHandle<()>>,
}

impl Scheduler {
    pub fn start(config: &SchedulerConfig) -> Self {
        if !config.enabled {
            return Self { tasks: Vec::new() };
        }

        let tasks = inventory::iter::<ScheduledTaskRegistration>()
            .filter_map(|registration| {
                let task_config = config.tasks.get(registration.name);
                if task_config.is_some_and(|task| !task.enabled) {
                    return None;
                }
                let interval_ms = task_config
                    .and_then(|task| task.interval_ms)
                    .unwrap_or(registration.default_interval_ms)
                    .max(1);
                let task = registration.task;
                Some(tokio::spawn(async move {
                    let interval = Duration::from_millis(interval_ms);
                    loop {
                        tokio::time::sleep(interval).await;
                        task();
                    }
                }))
            })
            .collect();
        Self { tasks }
    }

    pub fn stop(self) {
        for task in self.tasks {
            task.abort();
        }
    }
}
