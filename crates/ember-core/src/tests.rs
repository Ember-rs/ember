use std::{
    error::Error,
    sync::{Arc, Mutex},
};

use super::*;

const NODES: &[GraphNodeDescriptor] = &[
    GraphNodeDescriptor {
        provider: "service",
        output: "Service",
        source: "services.rs:2",
        kind: GraphNodeKind::Service,
    },
    GraphNodeDescriptor {
        provider: "bean",
        output: "Bean",
        source: "beans.rs:2",
        kind: GraphNodeKind::Bean,
    },
];

struct Hook;

impl LifecycleHook for Hook {
    fn name(&self) -> &'static str {
        "test-hook"
    }

    fn on_start(&self, _: &ApplicationContext) -> Result<(), String> {
        Ok(())
    }

    fn on_shutdown(&self, _: &ApplicationContext) -> Result<(), String> {
        Ok(())
    }
}

struct RecordingHook {
    name: &'static str,
    order: u32,
    source: &'static str,
    events: Arc<Mutex<Vec<&'static str>>>,
    start_error: Option<&'static str>,
    shutdown_error: Option<&'static str>,
}

impl LifecycleHook for RecordingHook {
    fn name(&self) -> &'static str {
        self.name
    }

    fn metadata(&self) -> ember_foundation::PhaseMetadata {
        ember_foundation::PhaseMetadata::new(
            ember_foundation::Phase::Startup,
            self.name,
            self.order,
            self.source,
        )
    }

    fn on_start(&self, _: &ApplicationContext) -> Result<(), String> {
        self.events
            .lock()
            .expect("event log should not be poisoned")
            .push(self.name);
        self.start_error
            .map_or(Ok(()), |error| Err(error.to_owned()))
    }

    fn on_shutdown(&self, _: &ApplicationContext) -> Result<(), String> {
        self.events
            .lock()
            .expect("event log should not be poisoned")
            .push(self.name);
        self.shutdown_error
            .map_or(Ok(()), |error| Err(error.to_owned()))
    }
}

fn recording_hook(
    name: &'static str,
    order: u32,
    source: &'static str,
    events: &Arc<Mutex<Vec<&'static str>>>,
) -> RecordingHook {
    RecordingHook {
        name,
        order,
        source,
        events: Arc::clone(events),
        start_error: None,
        shutdown_error: None,
    }
}

#[test]
fn lifecycle_is_explicit_and_ordered() {
    let mut application = Application::new(ApplicationContext::default()).with_hook(Hook);
    assert_eq!(application.state(), LifecycleState::Created);
    application.start().unwrap();
    assert_eq!(application.state(), LifecycleState::Running);
    application.shutdown().unwrap();
    assert_eq!(application.state(), LifecycleState::Stopped);
}

#[test]
fn lifecycle_sorts_hooks_and_shuts_down_in_reverse_order() {
    let events = Arc::new(Mutex::new(Vec::new()));
    let first = recording_hook("first", 0, "tests::first", &events);
    let mut second = recording_hook("second", 10, "tests::second", &events);
    second.order = 10;
    let third = recording_hook("third", 10, "tests::third", &events);
    let mut application = Application::new(ApplicationContext::default())
        .with_hook(third)
        .with_hook(first)
        .with_hook(second);

    application.start().expect("hooks should start");
    assert_eq!(
        *events.lock().expect("event log should not be poisoned"),
        vec!["first", "second", "third"]
    );
    application.shutdown().expect("hooks should shut down");
    assert_eq!(
        *events.lock().expect("event log should not be poisoned"),
        vec!["first", "second", "third", "third", "second", "first"]
    );
}

#[test]
fn startup_failure_rolls_back_successful_hooks_and_preserves_original_error() {
    let events = Arc::new(Mutex::new(Vec::new()));
    let first = recording_hook("first", 0, "tests::first", &events);
    let mut second = recording_hook("second", 1, "tests::second", &events);
    second.shutdown_error = Some("second rollback failed");
    let mut failed = recording_hook("failed", 2, "tests::failed", &events);
    failed.start_error = Some("startup failed");
    let mut application = Application::new(ApplicationContext::default())
        .with_hook(failed)
        .with_hook(first)
        .with_hook(second);

    let error = application.start().expect_err("startup should fail");
    assert_eq!(application.state(), LifecycleState::Created);
    assert_eq!(
        *events.lock().expect("event log should not be poisoned"),
        vec!["first", "second", "failed", "second", "first"]
    );
    let EmberError::Lifecycle(LifecycleError::StartupFailure {
        reason,
        original,
        rollback_failures,
    }) = error
    else {
        panic!("expected structured startup failure");
    };
    assert_eq!(reason, ember_foundation::ShutdownReason::StartupFailure);
    assert_eq!(original.phase(), ember_foundation::Phase::Startup);
    assert_eq!(original.participant().name(), "failed");
    assert_eq!(original.message(), "startup failed");
    assert_eq!(rollback_failures.len(), 1);
    assert_eq!(
        rollback_failures[0].phase(),
        ember_foundation::Phase::Shutdown
    );
    assert_eq!(rollback_failures[0].participant().name(), "second");
    assert_eq!(rollback_failures[0].message(), "second rollback failed");
}

#[test]
fn shutdown_attempts_every_hook_and_transitions_after_cleanup_failures() {
    let events = Arc::new(Mutex::new(Vec::new()));
    let mut first = recording_hook("first", 0, "tests::first", &events);
    first.shutdown_error = Some("first cleanup failed");
    let second = recording_hook("second", 1, "tests::second", &events);
    let mut third = recording_hook("third", 2, "tests::third", &events);
    third.shutdown_error = Some("third cleanup failed");
    let mut application = Application::new(ApplicationContext::default())
        .with_hook(third)
        .with_hook(first)
        .with_hook(second);
    application.start().expect("hooks should start");

    let policy = ember_foundation::ShutdownPolicy::new(std::time::Duration::from_secs(4), false);
    let error = application
        .shutdown_with(ember_foundation::ShutdownReason::Signal, policy)
        .expect_err("cleanup failures should be reported");
    assert_eq!(application.state(), LifecycleState::Stopped);
    assert_eq!(
        *events.lock().expect("event log should not be poisoned"),
        vec!["first", "second", "third", "third", "second", "first"]
    );
    let EmberError::Lifecycle(LifecycleError::ShutdownFailure {
        reason,
        policy: actual_policy,
        failures,
    }) = error
    else {
        panic!("expected structured shutdown failure");
    };
    assert_eq!(reason, ember_foundation::ShutdownReason::Signal);
    assert_eq!(actual_policy, policy);
    assert_eq!(
        failures
            .iter()
            .map(|failure| failure.participant().name())
            .collect::<Vec<_>>(),
        vec!["third", "first"]
    );
    assert!(matches!(
        application.shutdown(),
        Err(EmberError::InvalidLifecycle {
            action: "shutdown",
            state: LifecycleState::Stopped,
        })
    ));
}

#[test]
fn invalid_and_duplicate_metadata_are_rejected_before_start() {
    struct InvalidHook;

    impl LifecycleHook for InvalidHook {
        fn name(&self) -> &'static str {
            "invalid-hook"
        }

        fn metadata(&self) -> ember_foundation::PhaseMetadata {
            ember_foundation::PhaseMetadata::new(
                ember_foundation::Phase::Runtime,
                "different-name",
                0,
                "tests::invalid",
            )
        }

        fn on_start(&self, _: &ApplicationContext) -> Result<(), String> {
            panic!("invalid metadata must be rejected before start")
        }

        fn on_shutdown(&self, _: &ApplicationContext) -> Result<(), String> {
            Ok(())
        }
    }

    let error = Application::new(ApplicationContext::default())
        .with_hook(InvalidHook)
        .start()
        .expect_err("invalid metadata should fail startup");
    assert!(matches!(
        error,
        EmberError::Lifecycle(LifecycleError::InvalidMetadata { .. })
    ));

    let events = Arc::new(Mutex::new(Vec::new()));
    let duplicate_a = recording_hook("duplicate", 0, "tests::duplicate", &events);
    let duplicate_b = recording_hook("duplicate", 0, "tests::duplicate", &events);
    let error = Application::new(ApplicationContext::default())
        .with_hook(duplicate_a)
        .with_hook(duplicate_b)
        .start()
        .expect_err("duplicate metadata should fail startup");
    assert!(matches!(
        error,
        EmberError::Lifecycle(LifecycleError::DuplicateMetadata { .. })
    ));
    assert!(events
        .lock()
        .expect("event log should not be poisoned")
        .is_empty());
}

#[test]
fn graph_plan_is_deterministic_and_renders_order() {
    let descriptor = GraphDescriptor {
        nodes: NODES,
        edges: &[GraphEdgeDescriptor {
            consumer: "Service",
            dependency: "Bean",
            source: "services.rs:2",
        }],
    };

    let plan = descriptor.plan().unwrap();
    assert_eq!(
        plan.ordered_nodes()
            .map(|node| node.output)
            .collect::<Vec<_>>(),
        vec!["Bean", "Service"]
    );
    assert!(plan.render().contains("order: Bean Service"));
}

#[test]
fn graph_validation_reports_duplicate_missing_and_cycles() {
    let duplicate = GraphDescriptor {
        nodes: &[
            GraphNodeDescriptor {
                provider: "first",
                output: "Shared",
                source: "first.rs:1",
                kind: GraphNodeKind::Bean,
            },
            GraphNodeDescriptor {
                provider: "second",
                output: "Shared",
                source: "second.rs:1",
                kind: GraphNodeKind::Bean,
            },
        ],
        edges: &[],
    };
    assert!(duplicate
        .plan()
        .unwrap_err()
        .to_string()
        .contains("duplicate"));

    let missing = GraphDescriptor {
        nodes: NODES,
        edges: &[GraphEdgeDescriptor {
            consumer: "Service",
            dependency: "Missing",
            source: "services.rs:2",
        }],
    };
    assert!(missing
        .plan()
        .unwrap_err()
        .to_string()
        .contains("missing graph dependency `Missing`"));

    let cycle = GraphDescriptor {
        nodes: &[
            GraphNodeDescriptor {
                provider: "a",
                output: "A",
                source: "a.rs:1",
                kind: GraphNodeKind::Service,
            },
            GraphNodeDescriptor {
                provider: "b",
                output: "B",
                source: "b.rs:1",
                kind: GraphNodeKind::Service,
            },
        ],
        edges: &[
            GraphEdgeDescriptor {
                consumer: "A",
                dependency: "B",
                source: "a.rs:1",
            },
            GraphEdgeDescriptor {
                consumer: "B",
                dependency: "A",
                source: "b.rs:1",
            },
        ],
    };
    assert_eq!(
        cycle.plan().unwrap_err().to_string(),
        "graph contains a dependency cycle: A -> B -> A (declarations: a.rs:1, b.rs:1)"
    );

    let shared = GraphDescriptor {
        nodes: &[
            GraphNodeDescriptor {
                provider: "root",
                output: "Root",
                source: "root.rs:1",
                kind: GraphNodeKind::Bean,
            },
            GraphNodeDescriptor {
                provider: "first",
                output: "First",
                source: "first.rs:1",
                kind: GraphNodeKind::Service,
            },
            GraphNodeDescriptor {
                provider: "second",
                output: "Second",
                source: "second.rs:1",
                kind: GraphNodeKind::Service,
            },
        ],
        edges: &[
            GraphEdgeDescriptor {
                consumer: "First",
                dependency: "Root",
                source: "first.rs:1",
            },
            GraphEdgeDescriptor {
                consumer: "Second",
                dependency: "Root",
                source: "second.rs:1",
            },
        ],
    };
    assert!(shared
        .plan()
        .unwrap_err()
        .to_string()
        .contains("multiple consumers"));
}

#[test]
fn graph_validation_reports_unknown_consumers_and_stable_disconnected_order() {
    let unknown_consumer = GraphDescriptor {
        nodes: NODES,
        edges: &[GraphEdgeDescriptor {
            consumer: "MissingConsumer",
            dependency: "Bean",
            source: "src/missing.rs:7",
        }],
    };
    assert_eq!(
            unknown_consumer.plan().unwrap_err().to_string(),
            "graph edge consumer `MissingConsumer` is not declared while resolving `Bean` at src/missing.rs:7"
        );

    const DISCONNECTED_NODES: &[GraphNodeDescriptor] = &[
        GraphNodeDescriptor {
            provider: "z_provider",
            output: "Zed",
            source: "src/z.rs:1",
            kind: GraphNodeKind::Service,
        },
        GraphNodeDescriptor {
            provider: "a_provider",
            output: "Alpha",
            source: "src/a.rs:1",
            kind: GraphNodeKind::Service,
        },
    ];
    let descriptor = GraphDescriptor {
        nodes: DISCONNECTED_NODES,
        edges: &[],
    };
    let plan = descriptor.plan().unwrap();
    assert_eq!(
        plan.ordered_nodes()
            .map(|node| node.output)
            .collect::<Vec<_>>(),
        vec!["Alpha", "Zed"]
    );
    assert_eq!(plan.render(), "graph:\n  node a_provider -> Alpha (Service) [src/a.rs:1]\n  node z_provider -> Zed (Service) [src/z.rs:1]\n  order: Alpha Zed\n");
}

#[test]
fn graph_planning_has_bounded_small_and_medium_startup_measurements() {
    const SMALL_BUDGET: std::time::Duration = std::time::Duration::from_millis(200);
    const MEDIUM_BUDGET: std::time::Duration = std::time::Duration::from_secs(1);

    for (label, node_count, budget) in [("small", 32, SMALL_BUDGET), ("medium", 256, MEDIUM_BUDGET)]
    {
        let measurement = measure_graph_planning(node_count);
        eprintln!(
                "graph startup {label}: nodes={node_count}, elapsed={elapsed:?}, threshold={budget:?}, report_bytes={report_bytes}",
                elapsed = measurement.elapsed,
                report_bytes = measurement.report_bytes,
            );
        assert!(
            measurement.elapsed < budget,
            "{label} graph startup exceeded {budget:?}: {:?}",
            measurement.elapsed
        );
    }
}

#[derive(Debug)]
struct PlanningMeasurement {
    elapsed: std::time::Duration,
    report_bytes: usize,
}

/// Keeps startup-planning timing evidence bounded and inspectable without
/// adding a benchmark dependency or a runtime performance API.
fn measure_graph_planning(node_count: usize) -> PlanningMeasurement {
    let descriptor = performance_descriptor(node_count);
    let started = std::time::Instant::now();
    let report = descriptor.plan().unwrap().render();
    PlanningMeasurement {
        elapsed: started.elapsed(),
        report_bytes: report.len(),
    }
}

fn performance_descriptor(node_count: usize) -> GraphDescriptor {
    let nodes = (0..node_count)
        .map(|index| GraphNodeDescriptor {
            provider: Box::leak(format!("provider_{index:04}").into_boxed_str()),
            output: Box::leak(format!("Node{index:04}").into_boxed_str()),
            source: "src/generated.rs:1",
            kind: GraphNodeKind::Service,
        })
        .collect::<Vec<_>>();
    let edges = (1..node_count)
        .map(|index| GraphEdgeDescriptor {
            consumer: Box::leak(format!("Node{index:04}").into_boxed_str()),
            dependency: Box::leak(format!("Node{:04}", index - 1).into_boxed_str()),
            source: "src/generated.rs:1",
        })
        .collect::<Vec<_>>();
    GraphDescriptor {
        nodes: Box::leak(nodes.into_boxed_slice()),
        edges: Box::leak(edges.into_boxed_slice()),
    }
}

#[derive(Debug, thiserror::Error)]
#[error("secret provider detail")]
struct SecretProviderError;

#[test]
fn provider_failure_redacts_underlying_error_by_default() {
    let error = GraphError::provider_failure(
        "database_config",
        GraphPhase::Construction,
        "src/config.rs:4",
        SecretProviderError,
    );
    let display = error.to_string();
    let debug = format!("{error:?}");
    assert!(display.contains("database_config"));
    assert!(display.contains("Construction"));
    assert!(!display.contains("secret provider detail"));
    assert!(!debug.contains("secret provider detail"));
    assert!(error.source().and_then(Error::source).is_some());

    let failure = match &error {
        GraphError::ProviderFailure(failure) => failure,
        _ => unreachable!("provider_failure must create a ProviderFailure"),
    };
    assert_eq!(failure.provider(), "database_config");
    assert_eq!(failure.phase(), GraphPhase::Construction);
    assert_eq!(failure.source_location(), "src/config.rs:4");
    assert!(error
        .source()
        .and_then(Error::source)
        .is_some_and(|source| source.is::<SecretProviderError>()));
}
