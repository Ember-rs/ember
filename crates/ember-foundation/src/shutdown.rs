//! Runtime-agnostic shutdown contracts.

use std::time::Duration;

/// The framework-neutral cause selected for an application shutdown.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ShutdownReason {
    /// An external signal or adapter notification requested shutdown.
    Signal,
    /// The application explicitly requested shutdown.
    ApplicationRequest,
    /// Startup could not complete successfully.
    StartupFailure,
    /// A runtime failure requires the application to stop.
    RuntimeFailure,
}

/// Pure shutdown timing policy for a future lifecycle executor.
///
/// This value does not execute a timer or force a task to stop. Executors may
/// adapt it to their runtime while applications remain free to supply their
/// own policy.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ShutdownPolicy {
    grace_period: Duration,
    force_after_grace: bool,
}

impl ShutdownPolicy {
    /// The default grace period used by [`Default`].
    pub const DEFAULT_GRACE_PERIOD: Duration = Duration::from_secs(30);

    /// Creates a shutdown policy from an explicit grace period and force rule.
    pub const fn new(grace_period: Duration, force_after_grace: bool) -> Self {
        Self {
            grace_period,
            force_after_grace,
        }
    }

    /// Returns the time allowed for graceful shutdown.
    pub const fn grace_period(self) -> Duration {
        self.grace_period
    }

    /// Returns whether an executor should force shutdown after the grace
    /// period expires.
    pub const fn force_after_grace(self) -> bool {
        self.force_after_grace
    }
}

impl Default for ShutdownPolicy {
    fn default() -> Self {
        Self::new(Self::DEFAULT_GRACE_PERIOD, true)
    }
}
