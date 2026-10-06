use ember_scheduler::{Scheduler, SchedulerConfig};

#[test]
fn scheduler_is_disabled_by_default() {
    let config = SchedulerConfig::default();
    let scheduler = Scheduler::start(&config);
    scheduler.stop();
    assert!(!config.enabled);
    assert!(config.tasks.is_empty());
}
