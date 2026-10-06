use ember::foundation::{capture_error_backtrace, BacktraceMode, LogLevel, LoggingConfig};

#[test]
fn facade_exposes_the_canonical_foundation_logging_contract() {
    let config: ember::LoggingConfig = LoggingConfig::default();
    assert_eq!(config.level, LogLevel::Info);
    assert_eq!(config.backtrace, BacktraceMode::Off);
}

#[test]
fn startup_error_kinds_are_stable() {
    assert_eq!(
        ember::StartupError::Config(ember::ConfigError::InvalidProfile).kind(),
        "configuration"
    );
    assert_eq!(
        ember::StartupError::Address("invalid".to_owned()).kind(),
        "address"
    );
    assert_eq!(
        ember::StartupError::Core(ember::EmberError::InvalidLifecycle {
            action: "start",
            state: ember::core::LifecycleState::Running,
        })
        .kind(),
        "lifecycle"
    );
    assert_eq!(
        ember::StartupError::Web(ember::WebError::Server(std::io::Error::other(
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
    let error = ember::StartupError::Core(ember::EmberError::BeanPostProcessor {
        name: "secret_processor",
        phase: "before_initialization",
        message: "processor-secret-message".to_owned(),
    });

    tracing::subscriber::with_default(subscriber, || {
        ember::__private::log_startup_failure(&error);
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
    let error = ember::StartupError::Address(secret_address.to_owned());

    tracing::subscriber::with_default(subscriber, || {
        ember::__private::log_startup_failure(&error);
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
    let previous_profile = std::env::var_os("EMBER_PROFILE");
    std::env::set_var("EMBER_PROFILE", profile);

    let config_error = ember::run()
        .await
        .expect_err("unsafe profiles must fail before runtime initialization");
    assert!(matches!(
        config_error,
        ember::StartupError::Config(ember::ConfigError::InvalidProfile)
    ));
    assert!(
        capture_error_backtrace().is_none(),
        "configuration failures must not initialize foundation logging"
    );

    restore_environment("EMBER_PROFILE", previous_profile);

    let previous_host = std::env::var_os("EMBER_SERVER_HOST");
    let previous_backtrace = std::env::var_os("EMBER_LOGGING_BACKTRACE");
    std::env::set_var("EMBER_SERVER_HOST", "invalid-host-secret");
    std::env::set_var("EMBER_LOGGING_BACKTRACE", "errors");

    let address_error = ember::run()
        .await
        .expect_err("an invalid address must fail before application startup");
    assert!(matches!(address_error, ember::StartupError::Address(_)));
    assert!(
        !address_error.to_string().contains("invalid-host-secret"),
        "configured address must not be echoed in startup diagnostics: {address_error}"
    );
    assert!(
        capture_error_backtrace().is_some(),
        "valid configuration must initialize foundation before address validation"
    );

    restore_environment("EMBER_SERVER_HOST", previous_host);
    restore_environment("EMBER_LOGGING_BACKTRACE", previous_backtrace);
}

fn restore_environment(key: &str, value: Option<std::ffi::OsString>) {
    match value {
        Some(value) => std::env::set_var(key, value),
        None => std::env::remove_var(key),
    }
}
