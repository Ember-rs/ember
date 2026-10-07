//! Shared, framework-neutral foundations used by Scafra's library crates.
//!
//! This crate is intentionally small. It provides common logging, staged
//! foundation, and shutdown policy contracts; it is not an application
//! container, service locator, or event registry.

mod backtrace;
mod logging;

pub mod phase;
pub mod shutdown;

pub use backtrace::capture_error_backtrace;
pub use logging::{
    init_runtime_logging, init_runtime_logging_with_directive, BacktraceMode, LogLevel,
    LoggingConfig, LoggingConfigError,
};
pub use phase::{Phase, PhaseMetadata};
pub use shutdown::{ShutdownPolicy, ShutdownReason};

/// Re-export the standard tracing macros so all Scafra crates use the same
/// logging surface.
pub use tracing::{debug, error, info, trace, warn};

/// Emits an Scafra startup event, rendered as `STARTUP` by Scafra's default logger.
#[macro_export]
macro_rules! startup {
    ($($arg:tt)*) => {
        ::tracing::info!(target: "scafra::startup", $($arg)*)
    };
}
