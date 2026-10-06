//! Framework-neutral application primitives used by Embers adapters.

mod errors;
mod graph;
mod lifecycle;
mod registration;

pub use errors::{
    EmberError, GraphError, GraphPhase, LifecycleError, LifecycleFailure, ProviderFailure,
};
pub use graph::{
    GraphDescriptor, GraphEdgeDescriptor, GraphNodeDescriptor, GraphNodeKind, GraphPlan,
};
pub use lifecycle::{
    Application, ApplicationContext, LifecycleHook, LifecyclePhase, LifecycleState,
};
pub use registration::__private;
pub use registration::{
    Bean, BeanPostProcessor, ComponentKind, ComponentMetadata, ComponentRegistration,
    OptionalExtensionRegistration, PostProcessorRegistration, Service,
};

#[cfg(test)]
mod tests;
