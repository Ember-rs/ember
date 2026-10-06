use std::{backtrace::Backtrace, sync::OnceLock};

static BACKTRACE_ENABLED: OnceLock<bool> = OnceLock::new();

pub(crate) fn set_backtrace_enabled(enabled: bool) {
    let _ = BACKTRACE_ENABLED.set(enabled);
}

/// Returns an error backtrace when the configured runtime policy enables it.
pub fn capture_error_backtrace() -> Option<Backtrace> {
    match BACKTRACE_ENABLED.get().copied().unwrap_or(false) {
        false => None,
        true => Some(Backtrace::force_capture()),
    }
}
