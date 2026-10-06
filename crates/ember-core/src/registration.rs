use ember_foundation::{Phase, PhaseMetadata};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ComponentKind {
    Bean,
    Configuration,
    Service,
    Controller,
}

/// Small, static metadata record for a framework component.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ComponentMetadata {
    pub name: &'static str,
    pub kind: ComponentKind,
}

/// Link-time component metadata emitted by component macros.
pub struct ComponentRegistration {
    pub name: &'static str,
    pub kind: ComponentKind,
}

/// A framework extension that is linked into the application explicitly.
pub struct OptionalExtensionRegistration {
    pub name: &'static str,
    pub start: fn(),
}

inventory::collect!(OptionalExtensionRegistration);

inventory::collect!(ComponentRegistration);

/// A typed extension point invoked around component initialization.
pub trait BeanPostProcessor: Send + Sync + 'static {
    fn before_initialization(
        &self,
        _bean_name: &str,
        _metadata: &ComponentMetadata,
    ) -> Result<(), String> {
        Ok(())
    }

    fn after_initialization(
        &self,
        _bean_name: &str,
        _metadata: &ComponentMetadata,
    ) -> Result<(), String> {
        Ok(())
    }
}

/// Static registration record emitted by `#[post_processor]`.
pub struct PostProcessorRegistration {
    pub name: &'static str,
    pub create: fn() -> Box<dyn BeanPostProcessor>,
}

impl PostProcessorRegistration {
    /// Derives the stable startup metadata for this compatibility-preserving
    /// registration shape.
    pub const fn metadata(&self) -> PhaseMetadata {
        PhaseMetadata::new(Phase::Startup, self.name, 0, "inventory::post_processor")
    }
}

inventory::collect!(PostProcessorRegistration);

/// Marker trait for values supplied by a typed `#[bean]` provider.
pub trait Bean: Send + Sync + 'static {}

#[doc(hidden)]
pub mod __private {
    pub use inventory;
}

/// Marker trait implemented by `#[service]` components.
pub trait Service: Send + Sync + 'static {}
