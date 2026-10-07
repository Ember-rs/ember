use scafra_foundation::{
    capture_error_backtrace, init_runtime_logging, BacktraceMode, LogLevel, LoggingConfig,
};
use std::{
    io::{self, Write},
    sync::{Arc, Mutex},
};

#[derive(Clone)]
struct SharedWriter(Arc<Mutex<Vec<u8>>>);

impl Write for SharedWriter {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.0
            .lock()
            .expect("test writer lock should not be poisoned")
            .extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[test]
fn runtime_initialization_uses_the_first_backtrace_policy() {
    assert!(
        capture_error_backtrace().is_none(),
        "backtraces must be disabled before an error policy is configured"
    );

    let first_config = LoggingConfig {
        level: LogLevel::Off,
        backtrace: BacktraceMode::Off,
    };
    let output = Arc::new(Mutex::new(Vec::new()));
    let writer_output = Arc::clone(&output);
    let subscriber = tracing_subscriber::fmt()
        .with_ansi(false)
        .with_writer(move || SharedWriter(Arc::clone(&writer_output)))
        .finish();

    tracing::subscriber::with_default(subscriber, || {
        init_runtime_logging(&first_config);
        scafra_foundation::info!("application subscriber remains authoritative");
        assert!(capture_error_backtrace().is_none());

        init_runtime_logging(&LoggingConfig {
            level: LogLevel::Trace,
            backtrace: BacktraceMode::Errors,
        });
        assert!(
            capture_error_backtrace().is_none(),
            "a later initialization must not replace the first policy"
        );
    });

    let logs = String::from_utf8(
        output
            .lock()
            .expect("test writer lock should not be poisoned")
            .clone(),
    )
    .expect("diagnostics should be UTF-8");
    assert!(logs.contains("application subscriber remains authoritative"));
}
