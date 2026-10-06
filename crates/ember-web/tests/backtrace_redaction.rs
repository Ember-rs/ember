use axum::response::IntoResponse;
use ember_foundation::{init_runtime_logging, BacktraceMode, LogLevel, LoggingConfig};
use ember_web::AppError;
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

#[tokio::test]
async fn internal_error_redaction_holds_when_backtraces_are_enabled() {
    let output = Arc::new(Mutex::new(Vec::new()));
    let writer_output = Arc::clone(&output);
    let subscriber = tracing_subscriber::fmt()
        .with_ansi(false)
        .with_writer(move || SharedWriter(Arc::clone(&writer_output)))
        .finish();
    let secret = "database-password-that-must-not-appear";

    let response = tracing::subscriber::with_default(subscriber, || {
        init_runtime_logging(&LoggingConfig {
            level: LogLevel::Trace,
            backtrace: BacktraceMode::Errors,
        });
        AppError::Internal(secret.to_owned()).into_response()
    });

    let body = axum::body::to_bytes(response.into_body(), 1024)
        .await
        .expect("error response body should be readable");
    let body = String::from_utf8(body.to_vec()).expect("error response should be UTF-8");
    let logs = String::from_utf8(
        output
            .lock()
            .expect("test writer lock should not be poisoned")
            .clone(),
    )
    .expect("diagnostics should be UTF-8");

    assert!(!body.contains(secret), "secret leaked in response: {body}");
    assert!(!logs.contains(secret), "secret leaked in logs: {logs}");
    assert!(
        logs.contains("request failed"),
        "missing error event: {logs}"
    );
    assert!(
        logs.contains("backtrace"),
        "missing backtrace field: {logs}"
    );
}
