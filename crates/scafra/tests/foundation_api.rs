use scafra::foundation::{capture_error_backtrace, BacktraceMode, LogLevel, LoggingConfig};

#[test]
fn facade_exposes_the_canonical_foundation_logging_contract() {
    let config: scafra::LoggingConfig = LoggingConfig::default();
    assert_eq!(config.level, LogLevel::Info);
    assert_eq!(config.backtrace, BacktraceMode::Off);
}

#[test]
fn startup_error_kinds_are_stable() {
    assert_eq!(
        scafra::StartupError::Config(scafra::ConfigError::InvalidProfile).kind(),
        "configuration"
    );
    assert_eq!(
        scafra::StartupError::Address("invalid".to_owned()).kind(),
        "address"
    );
    assert_eq!(
        scafra::StartupError::Core(scafra::ScafraError::InvalidLifecycle {
            action: "start",
            state: scafra::core::LifecycleState::Running,
        })
        .kind(),
        "lifecycle"
    );
    assert_eq!(
        scafra::StartupError::Web(scafra::WebError::Server(std::io::Error::other(
            "server failure",
        )))
        .kind(),
        "web"
    );
}

#[test]
fn generated_startup_logging_redacts_processor_messages() {
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
                .expect("writer lock should not be poisoned")
                .extend_from_slice(bytes);
            Ok(bytes.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    let output = Arc::new(Mutex::new(Vec::new()));
    let writer_output = Arc::clone(&output);
    let subscriber = tracing_subscriber::fmt()
        .with_ansi(false)
        .with_writer(move || SharedWriter(Arc::clone(&writer_output)))
        .finish();
    let error = scafra::StartupError::Core(scafra::ScafraError::BeanPostProcessor {
        name: "secret_processor",
        phase: "before_initialization",
        message: "processor-secret-message".to_owned(),
    });

    tracing::subscriber::with_default(subscriber, || {
        scafra::__private::log_startup_failure(&error);
    });

    let logs = String::from_utf8(
        output
            .lock()
            .expect("writer lock should not be poisoned")
            .clone(),
    )
    .expect("diagnostics should be UTF-8");
    assert!(logs.contains("application_startup_failed"));
    assert!(logs.contains("error_kind") && logs.contains("lifecycle"));
    assert!(
        !logs.contains("processor-secret-message"),
        "secret leaked in: {logs}"
    );
}

#[test]
fn generated_startup_logging_redacts_invalid_address_messages() {
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
                .expect("writer lock should not be poisoned")
                .extend_from_slice(bytes);
            Ok(bytes.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    let output = Arc::new(Mutex::new(Vec::new()));
    let writer_output = Arc::clone(&output);
    let subscriber = tracing_subscriber::fmt()
        .with_ansi(false)
        .with_writer(move || SharedWriter(Arc::clone(&writer_output)))
        .finish();
    let secret_address = "invalid-address-secret";
    let error = scafra::StartupError::Address(secret_address.to_owned());

    tracing::subscriber::with_default(subscriber, || {
        scafra::__private::log_startup_failure(&error);
    });

    let logs = String::from_utf8(
        output
            .lock()
            .expect("writer lock should not be poisoned")
            .clone(),
    )
    .expect("diagnostics should be UTF-8");
    assert!(logs.contains("application_startup_failed"));
    assert!(logs.contains("error_kind") && logs.contains("address"));
    assert!(!logs.contains(secret_address), "secret leaked in: {logs}");
}

#[tokio::test]
async fn facade_runner_orders_config_logging_and_address_validation_before_startup() {
    let profile = "../foundation-api-secret-profile";
    let previous_profile = std::env::var_os("SCAFRA_PROFILE");
    std::env::set_var("SCAFRA_PROFILE", profile);

    let config_error = scafra::run()
        .await
        .expect_err("unsafe profiles must fail before runtime initialization");
    assert!(matches!(
        config_error,
        scafra::StartupError::Config(scafra::ConfigError::InvalidProfile)
    ));
    assert!(
        capture_error_backtrace().is_none(),
        "configuration failures must not initialize foundation logging"
    );

    restore_environment("SCAFRA_PROFILE", previous_profile);

    let previous_host = std::env::var_os("SCAFRA_SERVER_HOST");
    let previous_backtrace = std::env::var_os("SCAFRA_LOGGING_BACKTRACE");
    std::env::set_var("SCAFRA_SERVER_HOST", "invalid-host-secret");
    std::env::set_var("SCAFRA_LOGGING_BACKTRACE", "errors");

    let address_error = scafra::run()
        .await
        .expect_err("an invalid address must fail before application startup");
    assert!(matches!(address_error, scafra::StartupError::Address(_)));
    assert!(
        !address_error.to_string().contains("invalid-host-secret"),
        "configured address must not be echoed in startup diagnostics: {address_error}"
    );
    assert!(
        capture_error_backtrace().is_some(),
        "valid configuration must initialize foundation before address validation"
    );

    restore_environment("SCAFRA_SERVER_HOST", previous_host);
    restore_environment("SCAFRA_LOGGING_BACKTRACE", previous_backtrace);
}

fn restore_environment(key: &str, value: Option<std::ffi::OsString>) {
    match value {
        Some(value) => std::env::set_var(key, value),
        None => std::env::remove_var(key),
    }
}
