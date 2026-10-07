use std::{
    fs,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

use scafra_config::{Config, ConfigError, ConfigLoader};
use serde::{Deserialize, Serialize};

static NEXT_TEMP_DIR: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Default, Deserialize, Serialize)]
struct FixtureConfig {
    #[serde(default)]
    logging: LoggingFixture,
    #[serde(default)]
    server: ServerFixture,
}

#[derive(Debug, Deserialize, Serialize)]
struct LoggingFixture {
    #[serde(default = "default_level")]
    level: String,
    #[serde(default = "default_backtrace")]
    backtrace: String,
}

#[derive(Debug, Deserialize, Serialize)]
struct ServerFixture {
    #[serde(default = "default_port")]
    port: u16,
}

fn default_level() -> String {
    "info".to_owned()
}

fn default_backtrace() -> String {
    "off".to_owned()
}

fn default_port() -> u16 {
    8080
}

impl Default for LoggingFixture {
    fn default() -> Self {
        Self {
            level: default_level(),
            backtrace: default_backtrace(),
        }
    }
}

impl Default for ServerFixture {
    fn default() -> Self {
        Self {
            port: default_port(),
        }
    }
}

#[derive(Debug, thiserror::Error)]
#[error("validation detail: {detail}")]
struct SecretValidationError {
    detail: String,
}

impl Config for FixtureConfig {
    type Error = SecretValidationError;

    fn validate(&self) -> Result<(), Self::Error> {
        Err(SecretValidationError {
            detail: "validation-secret-that-must-not-appear".to_owned(),
        })
    }
}

struct TempRoot(PathBuf);

impl TempRoot {
    fn new() -> Self {
        let suffix = NEXT_TEMP_DIR.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "scafra-config-scafra014-{}-{suffix}",
            std::process::id()
        ));
        fs::create_dir_all(&path).expect("temporary configuration root should be created");
        Self(path)
    }

    fn write(&self, name: &str, contents: &str) {
        fs::write(self.0.join(name), contents).expect("configuration fixture should be written");
    }

    fn write_bytes(&self, name: &str, contents: &[u8]) {
        fs::write(self.0.join(name), contents).expect("configuration fixture should be written");
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempRoot {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn base_sources_are_loaded_in_yaml_yml_properties_order() {
    let root = TempRoot::new();
    root.write("application.yaml", "logging:\n  level: error\n");
    root.write("application.yml", "logging:\n  level: warn\n");
    root.write("application.properties", "logging.level=trace\n");

    let config = ConfigLoader::new()
        .root(root.path())
        .load::<FixtureConfig>()
        .expect("all supported base sources should load");

    assert_eq!(config.logging.level, "trace");
}

#[test]
fn profile_sources_are_loaded_in_yaml_yml_properties_order() {
    let root = TempRoot::new();
    root.write("application.yaml", "logging:\n  level: error\n");
    root.write("application-test.yaml", "logging:\n  level: warn\n");
    root.write("application-test.yml", "logging:\n  level: debug\n");
    root.write("application-test.properties", "logging.level=trace\n");

    let config = ConfigLoader::new()
        .root(root.path())
        .profile("test")
        .load::<FixtureConfig>()
        .expect("all supported profile sources should load");

    assert_eq!(config.logging.level, "trace");
}

#[test]
fn environment_and_explicit_override_precedence_remains_deterministic() {
    let root = TempRoot::new();
    root.write("application.properties", "server.port=8081\n");
    let prefix = format!("SCAFRA014_MATRIX_{}", std::process::id());
    let environment_key = format!("{prefix}_SERVER_PORT");
    let previous = std::env::var_os(&environment_key);
    std::env::set_var(&environment_key, "8082");

    let result = ConfigLoader::new()
        .root(root.path())
        .env_prefix(&prefix)
        .override_value("server.port", "8083")
        .load::<FixtureConfig>();

    match previous {
        Some(value) => std::env::set_var(&environment_key, value),
        None => std::env::remove_var(&environment_key),
    }

    let config = result.expect("environment and explicit overrides should load");
    assert_eq!(config.server.port, 8083);
}

#[test]
fn normalized_environment_key_collisions_have_stable_last_write_wins_order() {
    let root = TempRoot::new();
    let prefix = format!("SCAFRA014_COLLISION_{}", std::process::id());
    let first_key = format!("{prefix}_LOGGING_LEVEL");
    let second_key = format!("{prefix}_LOGGING__LEVEL");
    let previous_first = std::env::var_os(&first_key);
    let previous_second = std::env::var_os(&second_key);
    std::env::set_var(&first_key, "debug");
    std::env::set_var(&second_key, "trace");

    let result = ConfigLoader::new()
        .root(root.path())
        .env_prefix(&prefix)
        .load::<FixtureConfig>();

    match previous_first {
        Some(value) => std::env::set_var(&first_key, value),
        None => std::env::remove_var(&first_key),
    }
    match previous_second {
        Some(value) => std::env::set_var(&second_key, value),
        None => std::env::remove_var(&second_key),
    }

    let config = result.expect("normalized environment keys should load");
    assert_eq!(
        config.logging.level, "trace",
        "the stable sorted source order should make the later normalized key win"
    );
}

#[test]
fn invalid_environment_values_report_the_path_and_source_without_the_value() {
    let root = TempRoot::new();
    let prefix = format!("SCAFRA014_INVALID_{}", std::process::id());
    let environment_key = format!("{prefix}_SERVER_PORT");
    let secret_value = "not-a-port-secret";
    let previous = std::env::var_os(&environment_key);
    std::env::set_var(&environment_key, secret_value);

    let result = ConfigLoader::new()
        .root(root.path())
        .env_prefix(&prefix)
        .load::<FixtureConfig>();

    match previous {
        Some(value) => std::env::set_var(&environment_key, value),
        None => std::env::remove_var(&environment_key),
    }

    let error = result.expect_err("invalid environment values must fail before startup");
    let text = error.to_string();
    assert!(text.contains("server.port"), "missing path in: {text}");
    assert!(text.contains("environment"), "missing source in: {text}");
    assert!(
        !text.contains(secret_value),
        "secret value leaked in: {text}"
    );
}

#[test]
fn invalid_file_values_report_the_source_path_without_the_value() {
    let root = TempRoot::new();
    let secret_value = "not-a-port-from-file-secret";
    root.write(
        "application.properties",
        &format!("server.port={secret_value}\n"),
    );

    let error = ConfigLoader::new()
        .root(root.path())
        .load::<FixtureConfig>()
        .expect_err("invalid file values must fail before startup");
    let text = error.to_string();
    assert!(
        text.contains("application.properties"),
        "missing source in: {text}"
    );
    assert!(text.contains("server.port"), "missing path in: {text}");
    assert!(!text.contains(secret_value), "file value leaked in: {text}");
}

#[test]
fn invalid_explicit_overrides_report_the_source_without_the_value() {
    let root = TempRoot::new();
    let secret_value = "not-a-port-from-override-secret";
    let error = ConfigLoader::new()
        .root(root.path())
        .override_value("server.port", secret_value)
        .load::<FixtureConfig>()
        .expect_err("invalid explicit overrides must fail before startup");
    let text = error.to_string();
    assert!(text.contains("server.port"), "missing path in: {text}");
    assert!(
        text.to_ascii_lowercase().contains("override"),
        "missing override source in: {text}"
    );
    assert!(
        !text.contains(secret_value),
        "override value leaked in: {text}"
    );
}

#[test]
fn unsafe_profile_names_are_rejected_before_a_profile_path_is_built() {
    let root = TempRoot::new();

    for profile in [
        "",
        ".",
        "..",
        "safe..name",
        "../outside",
        "nested/profile",
        r"nested\profile",
    ] {
        let error = ConfigLoader::new()
            .root(root.path())
            .profile(profile)
            .load::<FixtureConfig>()
            .expect_err("unsafe profile names must fail");
        assert!(
            error.to_string().contains("profile"),
            "profile validation error should identify the profile: {error}"
        );
    }
}

#[test]
fn malformed_properties_report_source_path_and_one_based_line() {
    let root = TempRoot::new();
    root.write(
        "application.properties",
        "# supported comment\nlogging.level=debug\nnot-a-property\n",
    );

    let error = ConfigLoader::new()
        .root(root.path())
        .load::<FixtureConfig>()
        .expect_err("malformed properties must fail before startup");
    let text = error.to_string();
    assert!(
        text.contains("application.properties"),
        "missing path in: {text}"
    );
    assert!(text.contains("line 3"), "missing line number in: {text}");
    assert!(
        !text.contains("not-a-property"),
        "input value leaked in: {text}"
    );
}

#[test]
fn malformed_yaml_reports_source_path_and_line_without_input_values() {
    let root = TempRoot::new();
    let secret_value = "yaml-secret-that-must-not-appear";
    root.write(
        "application.yaml",
        &format!("logging:\n  level: {secret_value}\n  [malformed\n"),
    );

    let error = ConfigLoader::new()
        .root(root.path())
        .load::<FixtureConfig>()
        .expect_err("malformed YAML must fail before startup");
    let text = error.to_string();
    assert!(text.contains("application.yaml"), "missing path in: {text}");
    assert!(text.contains("line 4"), "missing line number in: {text}");
    assert!(!text.contains(secret_value), "YAML input leaked in: {text}");
}

#[test]
fn duplicate_properties_are_last_write_wins_and_adversarial_lines_remain_bounded() {
    let root = TempRoot::new();
    let long_value = format!("é{}", "x".repeat(4095));
    root.write(
        "application.properties",
        &format!(
            "# comment\n! alternate comment\nserver.port=8081\nserver.port : 8082\nlogging.level : {long_value}\n"
        ),
    );

    let config = ConfigLoader::new()
        .root(root.path())
        .load::<FixtureConfig>()
        .expect("comments, whitespace, Unicode, and long values should remain parseable");
    assert_eq!(config.server.port, 8082);
    assert_eq!(config.logging.level, long_value);
}

#[test]
fn properties_structural_conflicts_are_rejected() {
    let root = TempRoot::new();
    root.write(
        "application.properties",
        "logging=secret-value\nlogging.level=debug\n",
    );

    let error = ConfigLoader::new()
        .root(root.path())
        .load::<FixtureConfig>()
        .expect_err("scalar/object conflicts must fail deterministically");
    assert!(error.to_string().contains("application.properties"));
    assert!(!error.to_string().contains("secret-value"));
}

#[test]
fn missing_sources_are_ignored_but_unreadable_candidates_fail() {
    let root = TempRoot::new();
    let missing = ConfigLoader::new()
        .root(root.path())
        .load::<FixtureConfig>()
        .expect("missing configuration files are optional");
    assert_eq!(missing.server.port, 8080);

    fs::create_dir(root.path().join("application.yaml")).expect("directory fixture should exist");
    let error = ConfigLoader::new()
        .root(root.path())
        .load::<FixtureConfig>()
        .expect_err("directories must not be treated as missing files");
    assert!(matches!(error, ConfigError::Read { .. }));
    assert!(error.to_string().contains("application.yaml"));
}

#[test]
fn malformed_utf8_in_a_candidate_file_is_a_source_aware_failure() {
    let root = TempRoot::new();
    root.write_bytes("application.properties", b"server.port=8080\xff\n");

    let error = ConfigLoader::new()
        .root(root.path())
        .load::<FixtureConfig>()
        .expect_err("malformed UTF-8 must not be silently ignored");
    assert!(error.to_string().contains("application.properties"));
}

#[test]
fn scafra_config_uses_the_canonical_typed_logging_contract() {
    let root = TempRoot::new();
    root.write(
        "application.yaml",
        "logging:\n  level: debug\n  backtrace: errors\n",
    );

    let config = ConfigLoader::new()
        .root(root.path())
        .env_prefix(format!("SCAFRA014_TYPED_{}", std::process::id()))
        .load::<scafra_config::ScafraConfig>()
        .expect("typed logging configuration should load");

    assert_eq!(config.logging.level, scafra_config::LogLevel::Debug);
    assert_eq!(
        config.logging.backtrace,
        scafra_config::BacktraceMode::Errors
    );
}

#[test]
fn invalid_typed_logging_values_are_redacted_and_source_aware() {
    let root = TempRoot::new();
    let secret_value = "secret-invalid-log-level";
    root.write(
        "application.yaml",
        &format!("logging:\n  level: {secret_value}\n"),
    );

    let error = ConfigLoader::new()
        .root(root.path())
        .env_prefix(format!("SCAFRA014_TYPED_INVALID_{}", std::process::id()))
        .load::<scafra_config::ScafraConfig>()
        .expect_err("invalid typed logging values must fail before startup");
    let text = error.to_string();
    assert!(
        text.contains("logging.level"),
        "missing logical path: {text}"
    );
    assert!(
        text.contains("application.yaml"),
        "missing source path: {text}"
    );
    assert!(!text.contains(secret_value), "logging value leaked: {text}");
}

#[test]
fn validation_failures_are_redacted_before_startup() {
    let root = TempRoot::new();
    let error = ConfigLoader::new()
        .root(root.path())
        .load_validated::<FixtureConfig>()
        .expect_err("invalid configuration must fail before startup");
    let text = error.to_string();
    assert_eq!(text, "configuration validation failed");
    assert!(!text.contains("validation-secret-that-must-not-appear"));
}
