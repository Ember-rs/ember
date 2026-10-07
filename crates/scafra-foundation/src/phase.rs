//! Stable stage and extension metadata contracts.

/// A stage through which Scafra application code or its tooling may pass.
///
/// The discriminants and names are part of the foundation contract. They are
/// descriptive in this crate; the crate that owns a stage is responsible for
/// executing and ordering its work.
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Phase {
    /// Macro expansion and other compile-time generation.
    CompileTime = 0,
    /// Build-script discovery, parsing, and rendering.
    BuildTime = 1,
    /// Application initialization before serving work.
    Startup = 2,
    /// Normal application operation.
    Runtime = 3,
    /// Application and adapter cleanup.
    Shutdown = 4,
}

impl Phase {
    /// Every phase in its stable execution-stage order.
    pub const ALL: [Self; 5] = [
        Self::CompileTime,
        Self::BuildTime,
        Self::Startup,
        Self::Runtime,
        Self::Shutdown,
    ];

    /// Returns the stable lower-snake-case name of this phase.
    pub const fn name(self) -> &'static str {
        match self {
            Self::CompileTime => "compile_time",
            Self::BuildTime => "build_time",
            Self::Startup => "startup",
            Self::Runtime => "runtime",
            Self::Shutdown => "shutdown",
        }
    }

    /// Returns the stable stage order of this phase.
    pub const fn order(self) -> u8 {
        self as u8
    }
}

/// Static metadata describing one extension within a foundation phase.
///
/// The order is local to the phase and is distinct from [`Phase::order`].
/// Later executors may sort descriptors by phase order, extension order, and
/// name; constructing a descriptor does not register or sort it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PhaseMetadata {
    phase: Phase,
    name: &'static str,
    order: u32,
    source: &'static str,
}

impl PhaseMetadata {
    /// Creates static metadata for an extension.
    pub const fn new(phase: Phase, name: &'static str, order: u32, source: &'static str) -> Self {
        Self {
            phase,
            name,
            order,
            source,
        }
    }

    /// Returns the phase that owns this extension.
    pub const fn phase(self) -> Phase {
        self.phase
    }

    /// Returns the stable extension name.
    pub const fn name(self) -> &'static str {
        self.name
    }

    /// Returns the deterministic order within [`Self::phase`].
    pub const fn order(self) -> u32 {
        self.order
    }

    /// Returns the static source label for this extension.
    pub const fn source(self) -> &'static str {
        self.source
    }
}
