use std::{
    env,
    fmt::{self, Display, Formatter},
    str::FromStr,
};

use crate::backtrace::set_backtrace_enabled;
use serde::{de::Error as _, Deserialize, Deserializer, Serialize};
use tracing::{Event, Subscriber};
use tracing_subscriber::{
    fmt::{
        format::{FormatEvent, FormatFields, Writer},
        FmtContext,
    },
    registry::LookupSpan,
    EnvFilter,
};

#[derive(Debug, Clone, Copy, Default)]
struct EmberEventFormatter;

impl<S, N> FormatEvent<S, N> for EmberEventFormatter
where
    S: Subscriber + for<'a> LookupSpan<'a>,
    N: for<'a> FormatFields<'a> + 'static,
{
    fn format_event(
        &self,
        context: &FmtContext<'_, S, N>,
        mut writer: Writer<'_>,
        event: &Event<'_>,
    ) -> fmt::Result {
        let ansi = writer.has_ansi_escapes();
        let mut formatted = String::new();
        tracing_subscriber::fmt::format().format_event(
            context,
            Writer::new(&mut formatted),
            event,
        )?;

        let label = if event.metadata().target() == "ember::startup" {
            ("STARTUP", "36")
        } else {
            match *event.metadata().level() {
                tracing::Level::ERROR => ("ERROR", "31"),
                tracing::Level::WARN => ("WARN", "33"),
                tracing::Level::INFO => ("INFO", "38;5;208"),
                tracing::Level::DEBUG => ("DEBUG", "34"),
                tracing::Level::TRACE => ("TRACE", "35"),
            }
        };
        let rendered_label = if ansi {
            format!("\x1b[{}m{}\x1b[0m", label.1, label.0)
        } else {
            label.0.to_owned()
        };
        formatted = formatted.replacen(" INFO ", &format!(" {} ", rendered_label), 1);
        std::fmt::Write::write_str(&mut writer.by_ref(), &formatted)
    }
}

/// Logging levels understood by Ember configuration.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum LogLevel {
    Off,
    Error,
    Warn,
    #[default]
    Info,
    Debug,
    Trace,
}

impl LogLevel {
    /// Returns the level as a `tracing-subscriber` directive.
    pub const fn directive(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::Error => "error",
            Self::Warn => "warn",
            Self::Info => "info",
            Self::Debug => "debug",
            Self::Trace => "trace",
        }
    }
}

impl Display for LogLevel {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.directive())
    }
}

/// Keeps simple string comparisons source-compatible with the original
/// string-valued configuration field while callers migrate to `LogLevel`.
impl PartialEq<&str> for LogLevel {
    fn eq(&self, other: &&str) -> bool {
        self.directive() == *other
    }
}

impl FromStr for LogLevel {
    type Err = LoggingConfigError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value.trim().to_ascii_lowercase().as_str() {
            "off" | "none" => Ok(Self::Off),
            "error" => Ok(Self::Error),
            "warn" | "warning" => Ok(Self::Warn),
            "info" => Ok(Self::Info),
            "debug" => Ok(Self::Debug),
            "trace" => Ok(Self::Trace),
            _ => Err(LoggingConfigError::InvalidLevel),
        }
    }
}

impl<'de> Deserialize<'de> for LogLevel {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)
            .map_err(|_| D::Error::custom("invalid logging level"))?;
        value.parse().map_err(D::Error::custom)
    }
}

/// Controls when internal error events include a captured Rust backtrace.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum BacktraceMode {
    #[default]
    Off,
    Errors,
}

impl FromStr for BacktraceMode {
    type Err = LoggingConfigError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value.trim().to_ascii_lowercase().as_str() {
            "off" | "none" | "false" => Ok(Self::Off),
            "error" | "errors" | "on" | "true" => Ok(Self::Errors),
            _ => Err(LoggingConfigError::InvalidBacktrace),
        }
    }
}

impl<'de> Deserialize<'de> for BacktraceMode {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)
            .map_err(|_| D::Error::custom("invalid logging backtrace mode"))?;
        value.parse().map_err(D::Error::custom)
    }
}

/// The logging portion of the application configuration.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct LoggingConfig {
    #[serde(default)]
    pub level: LogLevel,
    #[serde(default)]
    pub backtrace: BacktraceMode,
}

impl LoggingConfig {
    /// Creates a configuration using a textual level, useful for compatibility
    /// entry points that historically accepted `&str`.
    pub fn with_level(level: impl AsRef<str>) -> Result<Self, LoggingConfigError> {
        Ok(Self {
            level: level.as_ref().parse()?,
            ..Self::default()
        })
    }
}

/// Errors raised while decoding the shared logging configuration.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum LoggingConfigError {
    #[error("invalid logging level; expected off, error, warn, info, debug, or trace")]
    InvalidLevel,

    #[error("invalid logging backtrace mode; expected off or errors")]
    InvalidBacktrace,
}

/// Runtime logging is initialized once by the standard Ember runner.
///
/// `RUST_LOG` is an explicit environment override for advanced
/// `tracing-subscriber` directives. Otherwise the configured simple level is
/// used. Repeated calls are harmless, which keeps lower-level escape hatches
/// and tests compatible with the standard runner.
pub fn init_runtime_logging(config: &LoggingConfig) {
    init_runtime_logging_with_directive(config, None);
}

/// Runtime initialization variant used by compatibility entry points that
/// historically accepted a textual `tracing-subscriber` directive.
pub fn init_runtime_logging_with_directive(
    config: &LoggingConfig,
    override_directive: Option<&str>,
) {
    set_backtrace_enabled(matches!(config.backtrace, BacktraceMode::Errors));
    let rust_log = env::var("RUST_LOG").ok();
    let directive = runtime_directive(config, rust_log.as_deref(), override_directive);
    let filter = EnvFilter::new(&directive);
    let _ = tracing_subscriber::fmt()
        .event_format(EmberEventFormatter)
        .with_env_filter(filter)
        .try_init();
}

fn runtime_directive(
    config: &LoggingConfig,
    rust_log: Option<&str>,
    override_directive: Option<&str>,
) -> String {
    let candidate = rust_log
        .or(override_directive)
        .unwrap_or(config.level.directive());
    EnvFilter::try_new(candidate)
        .map(|_| candidate.to_owned())
        .unwrap_or_else(|_| config.level.directive().to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_supported_levels_and_aliases() {
        assert_eq!("debug".parse::<LogLevel>().unwrap(), LogLevel::Debug);
        assert_eq!("WARNING".parse::<LogLevel>().unwrap(), LogLevel::Warn);
        assert_eq!("none".parse::<LogLevel>().unwrap(), LogLevel::Off);
        assert!("verbose".parse::<LogLevel>().is_err());
    }

    #[test]
    fn parses_error_backtrace_policy() {
        assert_eq!(
            "errors".parse::<BacktraceMode>().unwrap(),
            BacktraceMode::Errors
        );
        assert_eq!(
            "false".parse::<BacktraceMode>().unwrap(),
            BacktraceMode::Off
        );
    }

    #[test]
    fn logging_config_defaults_to_info_without_backtraces() {
        assert_eq!(LoggingConfig::default().level, LogLevel::Info);
        assert_eq!(LoggingConfig::default().backtrace, BacktraceMode::Off);
    }

    #[test]
    fn invalid_rust_log_falls_back_without_echoing_input() {
        let invalid_rust_log = "ember-secret[invalid";
        let config = LoggingConfig {
            level: LogLevel::Warn,
            backtrace: BacktraceMode::Off,
        };

        let directive = runtime_directive(&config, Some(invalid_rust_log), None);

        assert_eq!(directive, "warn");
        assert!(!directive.contains(invalid_rust_log));
    }

    #[test]
    fn invalid_compatibility_directive_falls_back_without_echoing_input() {
        let invalid_directive = "compat-secret[invalid";
        let config = LoggingConfig {
            level: LogLevel::Debug,
            backtrace: BacktraceMode::Off,
        };

        let directive = runtime_directive(&config, None, Some(invalid_directive));

        assert_eq!(directive, "debug");
        assert!(!directive.contains(invalid_directive));
    }

    #[test]
    fn invalid_logging_values_are_redacted_from_diagnostics() {
        let secret = "db-password-that-must-not-appear";
        let level_error = secret.parse::<LogLevel>().unwrap_err().to_string();
        let backtrace_error = secret.parse::<BacktraceMode>().unwrap_err().to_string();

        assert!(
            !level_error.contains(secret),
            "level leaked in: {level_error}"
        );
        assert!(
            !backtrace_error.contains(secret),
            "backtrace mode leaked in: {backtrace_error}"
        );
    }
}
