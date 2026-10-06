use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Mutex,
};

use ember::prelude::*;

#[service]
#[allow(dead_code)]
struct ContainerService;

static BEFORE_COUNT: AtomicUsize = AtomicUsize::new(0);
static AFTER_COUNT: AtomicUsize = AtomicUsize::new(0);
static PROCESSOR_EVENTS: Mutex<Vec<String>> = Mutex::new(Vec::new());

#[post_processor]
struct CountingPostProcessor;

impl BeanPostProcessor for CountingPostProcessor {
    fn before_initialization(
        &self,
        bean_name: &str,
        _: &ComponentMetadata,
    ) -> std::result::Result<(), String> {
        if bean_name == "ContainerService" {
            BEFORE_COUNT.fetch_add(1, Ordering::SeqCst);
        }
        Ok(())
    }

    fn after_initialization(
        &self,
        bean_name: &str,
        _: &ComponentMetadata,
    ) -> std::result::Result<(), String> {
        if bean_name == "ContainerService" {
            AFTER_COUNT.fetch_add(1, Ordering::SeqCst);
        }
        Ok(())
    }
}

#[post_processor]
struct AlphaProcessor;

impl BeanPostProcessor for AlphaProcessor {
    fn before_initialization(
        &self,
        bean_name: &str,
        _: &ComponentMetadata,
    ) -> std::result::Result<(), String> {
        if bean_name == "processor-order-target" {
            PROCESSOR_EVENTS
                .lock()
                .expect("processor event log should not be poisoned")
                .push("alpha-before".to_owned());
        }
        Ok(())
    }

    fn after_initialization(
        &self,
        bean_name: &str,
        _: &ComponentMetadata,
    ) -> std::result::Result<(), String> {
        if bean_name == "processor-order-target" {
            PROCESSOR_EVENTS
                .lock()
                .expect("processor event log should not be poisoned")
                .push("alpha-after".to_owned());
        }
        Ok(())
    }
}

#[post_processor]
struct BetaProcessor;

impl BeanPostProcessor for BetaProcessor {
    fn before_initialization(
        &self,
        bean_name: &str,
        _: &ComponentMetadata,
    ) -> std::result::Result<(), String> {
        if bean_name == "processor-order-target" {
            PROCESSOR_EVENTS
                .lock()
                .expect("processor event log should not be poisoned")
                .push("beta-before".to_owned());
        }
        Ok(())
    }

    fn after_initialization(
        &self,
        bean_name: &str,
        _: &ComponentMetadata,
    ) -> std::result::Result<(), String> {
        if bean_name == "processor-order-target" {
            PROCESSOR_EVENTS
                .lock()
                .expect("processor event log should not be poisoned")
                .push("beta-after".to_owned());
        }
        Ok(())
    }
}

#[post_processor]
struct FailureProcessor;

impl BeanPostProcessor for FailureProcessor {
    fn before_initialization(
        &self,
        bean_name: &str,
        _: &ComponentMetadata,
    ) -> std::result::Result<(), String> {
        if bean_name == "processor-before-failure" {
            Err("before processor failed".to_owned())
        } else {
            Ok(())
        }
    }

    fn after_initialization(
        &self,
        bean_name: &str,
        _: &ComponentMetadata,
    ) -> std::result::Result<(), String> {
        if bean_name == "processor-after-failure" {
            Err("after processor failed".to_owned())
        } else {
            Ok(())
        }
    }
}

fn processor_context(name: &'static str) -> ApplicationContext {
    ApplicationContext::new([ComponentMetadata {
        name,
        kind: ComponentKind::Bean,
    }])
}

struct ProcessorLifecycleHook;

impl ember::core::LifecycleHook for ProcessorLifecycleHook {
    fn name(&self) -> &'static str {
        "processor-lifecycle-hook"
    }

    fn on_start(&self, _: &ApplicationContext) -> std::result::Result<(), String> {
        PROCESSOR_EVENTS
            .lock()
            .expect("processor event log should not be poisoned")
            .push("hook-start".to_owned());
        Ok(())
    }

    fn on_shutdown(&self, _: &ApplicationContext) -> std::result::Result<(), String> {
        PROCESSOR_EVENTS
            .lock()
            .expect("processor event log should not be poisoned")
            .push("hook-shutdown".to_owned());
        Ok(())
    }
}

#[test]
fn application_context_discovers_components_and_runs_post_processors() {
    let context = ApplicationContext::discover();
    assert!(context
        .metadata()
        .iter()
        .any(|metadata| metadata.name == "ContainerService"));

    let mut application = Application::new(context);
    application.start().unwrap();
    application.shutdown().unwrap();

    assert_eq!(BEFORE_COUNT.load(Ordering::SeqCst), 1);
    assert_eq!(AFTER_COUNT.load(Ordering::SeqCst), 1);
}

#[test]
fn post_processors_run_in_stable_name_order_before_and_after_hooks() {
    let mut application = Application::new(processor_context("processor-order-target"));
    application.start().expect("processors should start");
    application.shutdown().expect("application should stop");

    let events = PROCESSOR_EVENTS
        .lock()
        .expect("processor event log should not be poisoned")
        .iter()
        .filter(|event| {
            matches!(
                event.as_str(),
                "alpha-before" | "beta-before" | "alpha-after" | "beta-after"
            )
        })
        .cloned()
        .collect::<Vec<_>>();
    assert_eq!(
        events,
        vec!["alpha-before", "beta-before", "alpha-after", "beta-after"]
    );
}

#[test]
fn processor_failures_identify_the_processor_and_roll_back_started_hooks() {
    let mut before_failure = Application::new(processor_context("processor-before-failure"));
    let error = before_failure
        .start()
        .expect_err("before processor failure should stop startup");
    assert_eq!(before_failure.state(), ember::core::LifecycleState::Created);
    let EmberError::Lifecycle(ember::core::LifecycleError::StartupFailure {
        original,
        rollback_failures,
        ..
    }) = error
    else {
        panic!("expected structured startup failure");
    };
    assert_eq!(original.participant().name(), "FailureProcessor");
    assert_eq!(original.message(), "before processor failed");
    assert!(rollback_failures.is_empty());

    let mut after_failure = Application::new(processor_context("processor-after-failure"))
        .with_hook(ProcessorLifecycleHook);
    let error = after_failure
        .start()
        .expect_err("after processor failure should stop startup");
    assert_eq!(after_failure.state(), ember::core::LifecycleState::Created);
    let EmberError::Lifecycle(ember::core::LifecycleError::StartupFailure {
        original,
        rollback_failures,
        ..
    }) = error
    else {
        panic!("expected structured startup failure");
    };
    assert_eq!(original.participant().name(), "FailureProcessor");
    assert_eq!(original.message(), "after processor failed");
    assert!(rollback_failures.is_empty());

    let events = PROCESSOR_EVENTS
        .lock()
        .expect("processor event log should not be poisoned")
        .iter()
        .filter(|event| matches!(event.as_str(), "hook-start" | "hook-shutdown"))
        .cloned()
        .collect::<Vec<_>>();
    assert_eq!(events, vec!["hook-start", "hook-shutdown"]);
}
