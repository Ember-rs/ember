use crate::{
    errors::{LifecycleError, LifecycleFailure, ScafraError},
    registration::{ComponentMetadata, ComponentRegistration, PostProcessorRegistration},
};
use scafra_foundation::{Phase, PhaseMetadata, ShutdownPolicy, ShutdownReason};
use std::{cmp::Ordering, fmt, sync::Arc};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LifecyclePhase {
    Startup,
    Shutdown,
    BeanPostProcessing,
}

/// The observable lifecycle state of an [`Application`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LifecycleState {
    Created,
    Running,
    Stopped,
}

/// A synchronous lifecycle hook. Async adapters can perform their own async
/// setup before registering a small synchronous state transition hook.
pub trait LifecycleHook: Send + Sync + 'static {
    fn name(&self) -> &'static str;

    /// Returns stable startup metadata while preserving the original hook
    /// implementation contract through a compatibility default.
    fn metadata(&self) -> PhaseMetadata {
        PhaseMetadata::new(
            Phase::Startup,
            self.name(),
            0,
            "application::lifecycle_hook",
        )
    }

    fn on_start(&self, context: &ApplicationContext) -> Result<(), String>;

    fn on_shutdown(&self, context: &ApplicationContext) -> Result<(), String>;
}

/// Read-only context shared with lifecycle hooks.
#[derive(Debug, Clone)]
pub struct ApplicationContext {
    metadata: Arc<[ComponentMetadata]>,
}

impl ApplicationContext {
    pub fn new(metadata: impl IntoIterator<Item = ComponentMetadata>) -> Self {
        Self {
            metadata: metadata.into_iter().collect(),
        }
    }

    pub fn metadata(&self) -> &[ComponentMetadata] {
        &self.metadata
    }

    pub fn discover() -> Self {
        Self::new(
            inventory::iter::<ComponentRegistration>().map(|component| ComponentMetadata {
                name: component.name,
                kind: component.kind,
            }),
        )
    }
}

impl Default for ApplicationContext {
    fn default() -> Self {
        Self::new([])
    }
}

/// The framework-neutral application lifecycle owner.
pub struct Application {
    context: ApplicationContext,
    hooks: Vec<Box<dyn LifecycleHook>>,
    state: LifecycleState,
}

impl fmt::Debug for Application {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Application")
            .field("context", &self.context)
            .field("hook_count", &self.hooks.len())
            .field("state", &self.state)
            .finish()
    }
}

impl Application {
    pub fn new(context: ApplicationContext) -> Self {
        Self {
            context,
            hooks: Vec::new(),
            state: LifecycleState::Created,
        }
    }

    pub fn context(&self) -> &ApplicationContext {
        &self.context
    }

    pub fn state(&self) -> LifecycleState {
        self.state
    }

    pub fn with_hook(mut self, hook: impl LifecycleHook + 'static) -> Self {
        self.hooks.push(Box::new(hook));
        self
    }

    pub fn start(&mut self) -> Result<(), ScafraError> {
        if self.state != LifecycleState::Created {
            return Err(ScafraError::InvalidLifecycle {
                action: "start",
                state: self.state,
            });
        }

        let hook_metadata = self.prepare_hooks()?;
        let processor_registrations = prepare_processor_registrations()?;
        let processors = processor_registrations
            .into_iter()
            .map(|(metadata, registration)| (metadata, (registration.create)()))
            .collect::<Vec<_>>();

        for (processor_metadata, processor) in &processors {
            for component_metadata in self.context.metadata() {
                if let Err(message) =
                    processor.before_initialization(component_metadata.name, component_metadata)
                {
                    let original =
                        LifecycleFailure::new(Phase::Startup, *processor_metadata, message);
                    return Err(startup_failure(original, Vec::new()));
                }
            }
        }

        let mut started_hooks = Vec::with_capacity(self.hooks.len());
        for (index, (hook, metadata)) in self.hooks.iter().zip(&hook_metadata).enumerate() {
            match hook.on_start(&self.context) {
                Ok(()) => started_hooks.push(index),
                Err(message) => {
                    let original = LifecycleFailure::new(Phase::Startup, *metadata, message);
                    return Err(startup_failure_with_rollback(
                        original,
                        &self.context,
                        &self.hooks,
                        &hook_metadata,
                        &started_hooks,
                    ));
                }
            }
        }

        for (processor_metadata, processor) in &processors {
            for component_metadata in self.context.metadata() {
                if let Err(message) =
                    processor.after_initialization(component_metadata.name, component_metadata)
                {
                    let original =
                        LifecycleFailure::new(Phase::Startup, *processor_metadata, message);
                    return Err(startup_failure_with_rollback(
                        original,
                        &self.context,
                        &self.hooks,
                        &hook_metadata,
                        &started_hooks,
                    ));
                }
            }
        }

        self.state = LifecycleState::Running;
        Ok(())
    }

    pub fn shutdown(&mut self) -> Result<(), ScafraError> {
        self.shutdown_with(
            ShutdownReason::ApplicationRequest,
            ShutdownPolicy::default(),
        )
    }

    pub fn shutdown_with(
        &mut self,
        reason: ShutdownReason,
        policy: ShutdownPolicy,
    ) -> Result<(), ScafraError> {
        if self.state != LifecycleState::Running {
            return Err(ScafraError::InvalidLifecycle {
                action: "shutdown",
                state: self.state,
            });
        }

        let mut failures = Vec::new();
        for hook in self.hooks.iter().rev() {
            if let Err(message) = hook.on_shutdown(&self.context) {
                failures.push(LifecycleFailure::new(
                    Phase::Shutdown,
                    hook.metadata(),
                    message,
                ));
            }
        }
        self.state = LifecycleState::Stopped;

        if failures.is_empty() {
            Ok(())
        } else {
            Err(ScafraError::Lifecycle(LifecycleError::ShutdownFailure {
                reason,
                policy,
                failures,
            }))
        }
    }

    fn prepare_hooks(&mut self) -> Result<Vec<PhaseMetadata>, ScafraError> {
        let hooks = std::mem::take(&mut self.hooks);
        let mut entries = hooks
            .into_iter()
            .map(|hook| {
                let name = hook.name();
                let metadata = hook.metadata();
                (metadata, name, hook)
            })
            .collect::<Vec<_>>();

        for (metadata, name, _) in &entries {
            if let Some(error) = invalid_metadata(*metadata, name) {
                self.hooks = entries.into_iter().map(|(_, _, hook)| hook).collect();
                return Err(error);
            }
        }

        entries.sort_by(|left, right| compare_metadata(left.0, right.0));
        for pair in entries.windows(2) {
            if same_metadata(pair[0].0, pair[1].0) {
                let error = ScafraError::Lifecycle(LifecycleError::DuplicateMetadata {
                    first: pair[0].0,
                    duplicate: pair[1].0,
                });
                self.hooks = entries.into_iter().map(|(_, _, hook)| hook).collect();
                return Err(error);
            }
        }

        let metadata = entries.iter().map(|(metadata, _, _)| *metadata).collect();
        self.hooks = entries.into_iter().map(|(_, _, hook)| hook).collect();
        Ok(metadata)
    }
}

fn prepare_processor_registrations(
) -> Result<Vec<(PhaseMetadata, &'static PostProcessorRegistration)>, ScafraError> {
    let mut entries = inventory::iter::<PostProcessorRegistration>()
        .map(|registration| (registration.metadata(), registration))
        .collect::<Vec<_>>();

    for (metadata, _) in &entries {
        if let Some(error) = invalid_metadata(*metadata, metadata.name()) {
            return Err(error);
        }
    }

    entries.sort_by(|left, right| compare_metadata(left.0, right.0));
    for pair in entries.windows(2) {
        if same_metadata(pair[0].0, pair[1].0) {
            return Err(ScafraError::Lifecycle(LifecycleError::DuplicateMetadata {
                first: pair[0].0,
                duplicate: pair[1].0,
            }));
        }
    }
    Ok(entries)
}

fn invalid_metadata(metadata: PhaseMetadata, expected_name: &'static str) -> Option<ScafraError> {
    if metadata.phase() != Phase::Startup
        || metadata.name().is_empty()
        || metadata.source().is_empty()
        || metadata.name() != expected_name
    {
        Some(ScafraError::Lifecycle(LifecycleError::InvalidMetadata {
            metadata,
            expected_phase: Phase::Startup,
            expected_name,
        }))
    } else {
        None
    }
}

fn compare_metadata(left: PhaseMetadata, right: PhaseMetadata) -> Ordering {
    left.order()
        .cmp(&right.order())
        .then_with(|| left.name().cmp(right.name()))
        .then_with(|| left.source().cmp(right.source()))
}

fn same_metadata(left: PhaseMetadata, right: PhaseMetadata) -> bool {
    left.order() == right.order() && left.name() == right.name() && left.source() == right.source()
}

fn startup_failure_with_rollback(
    original: LifecycleFailure,
    context: &ApplicationContext,
    hooks: &[Box<dyn LifecycleHook>],
    hook_metadata: &[PhaseMetadata],
    started_hooks: &[usize],
) -> ScafraError {
    let mut rollback_failures = Vec::new();
    for index in started_hooks.iter().rev() {
        let hook = &hooks[*index];
        if let Err(message) = hook.on_shutdown(context) {
            rollback_failures.push(LifecycleFailure::new(
                Phase::Shutdown,
                hook_metadata[*index],
                message,
            ));
        }
    }
    startup_failure(original, rollback_failures)
}

fn startup_failure(
    original: LifecycleFailure,
    rollback_failures: impl IntoIterator<Item = LifecycleFailure>,
) -> ScafraError {
    ScafraError::Lifecycle(LifecycleError::StartupFailure {
        reason: ShutdownReason::StartupFailure,
        original,
        rollback_failures: rollback_failures.into_iter().collect(),
    })
}
