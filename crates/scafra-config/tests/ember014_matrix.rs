use std::{
    fs,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

use scafra_config::{Config, ConfigError, ConfigLoader, ScafraConfig};
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

#[derive(Debug, Default, Deserialize, Serialize)]
struct AmbiguousFixtureConfig {
    #[serde(default)]
    foo: AmbiguousNestedFixture,
    #[serde(default)]
    foo_bar: u16,
}

#[derive(Debug, Default, Deserialize, Serialize)]
struct AmbiguousNestedFixture {
    #[serde(default)]
    bar: u16,
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

impl scafra_config::ConfigProperties for FixtureConfig {
    const CONFIG_PREFIX: &'static str = "";
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
    let prefix = format!("SCFRA014_MATRIX_{}", std::process::id());
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
fn environment_names_resolving_to_the_same_path_are_rejected() {
    let root = TempRoot::new();
    let prefix = format!("SCFRA014_COLLISION_{}", std::process::id());
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

    let error = result.expect_err("two names for one path must be rejected");
    assert!(matches!(
        error,
        ConfigError::InvalidValue {
            path,
            source_kind
        } if path == "logging.level" && source_kind == "environment"
    ));
}

#[test]
fn environment_names_resolve_underscored_fields_and_dynamic_task_keys() {
    let root = TempRoot::new();
    let prefix = format!("SCFRA014_UNDERSCORES_{}", std::process::id());
    let variables = [
        (format!("{prefix}_SERVER_PORT"), "9000"),
        (
            format!("{prefix}_SECURITY_BEARER_TOKEN"),
            "security-token-70",
        ),
        (
            format!("{prefix}_SECURITY_JWT_ISSUER_URI"),
            "https://issuer.example.test",
        ),
        (
            format!("{prefix}_ACTUATOR_SECURITY_BEARER_TOKEN"),
            "actuator-token-70",
        ),
        (
            format!("{prefix}_SCHEDULER_TASKS_CLEANUP_INTERVAL_MS"),
            "1500",
        ),
        (format!("{prefix}_SCHEDULER_TASKS_CLEANUP_ENABLED"), "false"),
        (format!("{prefix}_BOOTUI_LOCAL_ONLY"), "false"),
    ];
    let previous = variables
        .iter()
        .map(|(key, _)| (key.clone(), std::env::var_os(key)))
        .collect::<Vec<_>>();
    for (key, value) in &variables {
        std::env::set_var(key, value);
    }

    let result = ConfigLoader::new()
        .root(root.path())
        .env_prefix(&prefix)
        .load::<ScafraConfig>();

    for (key, value) in previous {
        match value {
            Some(value) => std::env::set_var(key, value),
            None => std::env::remove_var(key),
        }
    }

    let config = result.expect("each unique schema path should resolve");
    assert_eq!(config.server.port, 9000);
    assert_eq!(
        config.security.bearer_token.as_deref(),
        Some("security-token-70")
    );
    assert_eq!(
        config.security.jwt.issuer_uri.as_deref(),
        Some("https://issuer.example.test")
    );
    assert_eq!(
        config.actuator.security.bearer_token.as_deref(),
        Some("actuator-token-70")
    );
    assert_eq!(config.scheduler.tasks["cleanup"].interval_ms, Some(1500));
    assert!(!config.scheduler.tasks["cleanup"].enabled);
    assert!(!config.bootui.local_only);
}

#[test]
fn explicit_environment_paths_preserve_underscores_inside_fields() {
    let root = TempRoot::new();
    let prefix = format!("SCFRA014_EXPLICIT_PATH_{}", std::process::id());
    let environment_key = format!("{prefix}_SECURITY__BEARER_TOKEN");
    let previous = std::env::var_os(&environment_key);
    std::env::set_var(&environment_key, "explicit-token-70");

    let result = ConfigLoader::new()
        .root(root.path())
        .env_prefix(&prefix)
        .load::<ScafraConfig>();

    match previous {
        Some(value) => std::env::set_var(&environment_key, value),
        None => std::env::remove_var(&environment_key),
    }

    let config = result.expect("double underscores should separate path levels");
    assert_eq!(
        config.security.bearer_token.as_deref(),
        Some("explicit-token-70")
    );
}

#[test]
fn ambiguous_and_unknown_environment_names_fail_deterministically() {
    let root = TempRoot::new();
    let ambiguous_prefix = format!("SCFRA014_AMBIGUOUS_{}", std::process::id());
    let ambiguous_key = format!("{ambiguous_prefix}_FOO_BAR");
    let previous_ambiguous = std::env::var_os(&ambiguous_key);
    std::env::set_var(&ambiguous_key, "7");
    let ambiguous_result = ConfigLoader::new()
        .root(root.path())
        .env_prefix(&ambiguous_prefix)
        .load::<AmbiguousFixtureConfig>();
    match previous_ambiguous {
        Some(value) => std::env::set_var(&ambiguous_key, value),
        None => std::env::remove_var(&ambiguous_key),
    }
    assert!(matches!(
        ambiguous_result,
        Err(ConfigError::InvalidValue {
            path,
            source_kind
        }) if path == "foo_bar" && source_kind == "environment variable name"
    ));

    let unknown_root = TempRoot::new();
    unknown_root.write("application.yaml", "server:\n  unknown_field: 10\n");
    let unknown_prefix = format!("SCFRA014_UNKNOWN_{}", std::process::id());
    let unknown_key = format!("{unknown_prefix}_SERVER_UNKNOWN_FIELD");
    let previous_unknown = std::env::var_os(&unknown_key);
    std::env::set_var(&unknown_key, "7");
    let unknown_result = ConfigLoader::new()
        .root(unknown_root.path())
        .env_prefix(&unknown_prefix)
        .load::<FixtureConfig>();
    match previous_unknown {
        Some(value) => std::env::set_var(&unknown_key, value),
        None => std::env::remove_var(&unknown_key),
    }
    assert!(matches!(
        unknown_result,
        Err(ConfigError::InvalidValue {
            path,
            source_kind
        }) if path == "server_unknown_field" && source_kind == "environment variable name"
    ));
}

#[test]
fn secret_environment_values_stay_redacted_from_decode_diagnostics() {
    let root = TempRoot::new();
    let prefix = format!("SCFRA014_SECRET_REDACTION_{}", std::process::id());
    let secret_key = format!("{prefix}_SECURITY__JWT__SECRET");
    let invalid_key = format!("{prefix}_SECURITY__JWT__ENABLED");
    let previous_secret = std::env::var_os(&secret_key);
    let previous_invalid = std::env::var_os(&invalid_key);
    let secret_value = "jwt-secret-redaction-sentinel-70";
    std::env::set_var(&secret_key, secret_value);
    std::env::set_var(&invalid_key, "not-a-boolean");

    let result = ConfigLoader::new()
        .root(root.path())
        .env_prefix(&prefix)
        .load::<ScafraConfig>();

    match previous_secret {
        Some(value) => std::env::set_var(&secret_key, value),
        None => std::env::remove_var(&secret_key),
    }
    match previous_invalid {
        Some(value) => std::env::set_var(&invalid_key, value),
        None => std::env::remove_var(&invalid_key),
    }

    let error = result.expect_err("the invalid boolean should fail decoding");
    assert!(!error.to_string().contains(secret_value));
    assert!(!format!("{error:?}").contains(secret_value));
}

#[test]
fn invalid_environment_values_report_the_path_and_source_without_the_value() {
    let root = TempRoot::new();
    let prefix = format!("SCFRA014_INVALID_{}", std::process::id());
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

#[cfg(unix)]
#[test]
fn non_unicode_environment_values_fail_without_panicking_or_echoing_values() {
    use std::os::unix::ffi::OsStringExt;

    let root = TempRoot::new();
    let prefix = format!("SCFRA014_NON_UTF8_{}", std::process::id());
    let environment_key = format!("{prefix}_SERVER_PORT");
    let previous = std::env::var_os(&environment_key);
    std::env::set_var(
        &environment_key,
        std::ffi::OsString::from_vec(vec![0xff, 0xfe]),
    );

    let result = ConfigLoader::new()
        .root(root.path())
        .env_prefix(&prefix)
        .load::<FixtureConfig>();

    match previous {
        Some(value) => std::env::set_var(&environment_key, value),
        None => std::env::remove_var(&environment_key),
    }

    let error = result.expect_err("non-Unicode values must fail configuration loading");
    assert!(matches!(
        &error,
        ConfigError::InvalidValue {
            path,
            source_kind
        } if path == "server.port" && source_kind == "environment"
    ));
    assert_eq!(
        error.to_string(),
        "invalid configuration value for `server.port` from environment"
    );
    assert!(!format!("{error:?}").contains("ff"));
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
        .env_prefix(format!("SCFRA014_TYPED_{}", std::process::id()))
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
        .env_prefix(format!("SCFRA014_TYPED_INVALID_{}", std::process::id()))
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
    assert!(!format!("{error:?}").contains("validation-secret-that-must-not-appear"));
}

#[test]
fn properties_redact_arbitrary_validation_errors_in_display_and_debug() {
    let root = TempRoot::new();
    let error = scafra_config::Properties::<FixtureConfig>::with_loader(
        ConfigLoader::new().root(root.path()),
    )
    .load()
    .expect_err("invalid properties must fail before use");

    assert_eq!(error.to_string(), "configuration validation failed");
    assert!(!format!("{error:?}").contains("validation-secret-that-must-not-appear"));
    assert_eq!(format!("{error:?}"), "Validation");
}

#[test]
fn manually_constructed_validation_errors_stay_redacted_when_formatted() {
    let secret = "manual-validation-secret";
    let error = ConfigError::Validation(secret.to_owned());

    assert_eq!(error.to_string(), "configuration validation failed");
    assert_eq!(format!("{error:?}"), "Validation");
}
