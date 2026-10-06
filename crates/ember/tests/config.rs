use std::{
    fs,
    path::PathBuf,
    sync::atomic::{AtomicBool, Ordering},
};

use ember::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Debug, Default, Deserialize, Serialize, Config)]
#[config(prefix = "server")]
struct TestConfig {
    server: ServerSettings,
}

#[derive(Debug, Default, Deserialize, Serialize)]
struct ServerSettings {
    host: String,
    port: u16,
}

#[derive(Debug, Default, Deserialize, Serialize, Config)]
#[config(prefix = "required")]
struct RequiredConfig {
    #[config(required)]
    secret: String,
    #[config(required)]
    port: Option<u16>,
    #[config(required)]
    fully_qualified: std::string::String,
    #[config(required)]
    optional_text: std::option::Option<String>,
}

#[derive(Debug, Default, Deserialize, Serialize, Config, PartialEq)]
#[config(prefix = "mail")]
struct MailProperties {
    host: String,
    port: u16,
}

#[test]
fn services_can_load_typed_properties_from_resources() {
    let root = std::env::temp_dir().join(format!("ember-properties-test-{}", std::process::id()));
    fs::create_dir_all(root.join("src/resources")).unwrap();
    fs::write(
        root.join("src/resources/application.yaml"),
        "mail:\n  host: smtp.example.test\n  port: 2525\n",
    )
    .unwrap();

    let properties = Properties::<MailProperties>::with_loader(ConfigLoader::new().root(&root))
        .load()
        .unwrap();

    assert_eq!(
        properties,
        MailProperties {
            host: "smtp.example.test".to_owned(),
            port: 2525,
        }
    );
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn application_configuration_selects_the_active_profile() {
    let root = std::env::temp_dir().join(format!("ember-profile-test-{}", std::process::id()));
    fs::create_dir_all(root.join("src/resources")).unwrap();
    fs::write(
        root.join("src/resources/application.yaml"),
        "ember:\n  profiles:\n    active: dev\nserver:\n  port: 8080\n",
    )
    .unwrap();
    fs::write(
        root.join("src/resources/application-dev.properties"),
        "server.port=8090\n",
    )
    .unwrap();

    let loader = ConfigLoader::new().root(&root);
    let config = loader.load::<EmberConfig>().unwrap();

    assert_eq!(loader.active_profile().unwrap(), "dev");
    assert_eq!(config.server.port, 8090);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn configuration_precedence_is_defaults_yaml_profile_environment_then_override() {
    let root = unique_temp_dir();
    fs::create_dir_all(&root).unwrap();
    fs::write(
        root.join("application.yaml"),
        "server:\n  host: 0.0.0.0\n  port: 8080\n",
    )
    .unwrap();
    fs::write(
        root.join("application-test.yaml"),
        "server:\n  port: 8081\n",
    )
    .unwrap();

    let env_prefix = format!("EMBER_TEST_{}", std::process::id());
    let env_key = format!("{env_prefix}_SERVER_PORT");
    std::env::set_var(&env_key, "8082");

    let config = ConfigLoader::new()
        .root(&root)
        .profile("test")
        .env_prefix(&env_prefix)
        .override_value("server.port", "8083")
        .load::<TestConfig>()
        .unwrap();

    assert_eq!(config.server.host, "0.0.0.0");
    assert_eq!(config.server.port, 8083);
    std::env::remove_var(env_key);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn config_derive_validates_supported_required_shapes_in_source_order() {
    let valid = RequiredConfig {
        secret: "configured-secret".to_owned(),
        port: Some(0),
        fully_qualified: "é".to_owned(),
        optional_text: Some(String::new()),
    };
    valid
        .validate()
        .expect("present required values should validate");
    assert_eq!(RequiredConfig::CONFIG_PREFIX, "required");

    let blank = RequiredConfig {
        secret: " \n\t".to_owned(),
        ..valid
    };
    let error = blank
        .validate()
        .expect_err("whitespace-only strings must fail validation");
    assert_eq!(error.field, "secret");
    assert_eq!(error.message, "must not be blank");
    assert!(!error.to_string().contains("configured-secret"));

    let missing = RequiredConfig {
        secret: "configured-secret".to_owned(),
        port: None,
        fully_qualified: "present".to_owned(),
        optional_text: Some(String::new()),
    };
    let error = missing
        .validate()
        .expect_err("missing optional values marked required must fail");
    assert_eq!(error.field, "port");
    assert_eq!(error.message, "must be present");
}

#[test]
fn validated_loader_keeps_derived_validation_errors_redacted() {
    let error = ConfigLoader::new()
        .load_validated::<RequiredConfig>()
        .expect_err("default required values must fail before startup");

    assert!(matches!(
        &error,
        ConfigError::Validation(message) if message == "validation failed"
    ));
    let text = error.to_string();
    assert_eq!(text, "configuration validation failed");
    assert!(!text.contains("configured-secret"));
    assert!(!text.contains("must not be blank"));
}

#[test]
fn invalid_derived_configuration_stops_before_context_and_router_construction() {
    let startup_reached = AtomicBool::new(false);
    let result = start_application_after_validation(&startup_reached);

    assert!(matches!(
        result,
        Err(ConfigError::Validation(message)) if message == "validation failed"
    ));
    assert!(
        !startup_reached.load(Ordering::Acquire),
        "application startup must not construct context or router after validation fails"
    );
}

fn start_application_after_validation(
    startup_reached: &AtomicBool,
) -> std::result::Result<(), ConfigError> {
    let _config = ConfigLoader::new().load_validated::<RequiredConfig>()?;
    let context = ApplicationContext::discover();
    let router = build_router().expect("the startup fixture router should build");
    startup_reached.store(true, Ordering::Release);
    let _ = (context, router);
    Ok(())
}

fn unique_temp_dir() -> PathBuf {
    std::env::temp_dir().join(format!("ember-config-test-{}", std::process::id()))
}
