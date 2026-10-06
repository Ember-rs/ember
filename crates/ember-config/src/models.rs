use crate::sources::Config;
use ember_actuator::ActuatorConfig;
use ember_foundation::LoggingConfig;
use ember_scheduler::SchedulerConfig;
use ember_security::SecurityConfig;
#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
pub struct ServerConfig {
    #[serde(default = "default_host")]
    pub host: String,
    #[serde(default = "default_port")]
    pub port: u16,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            host: default_host(),
            port: default_port(),
        }
    }
}

impl Config for ServerConfig {
    type Error = std::convert::Infallible;

    fn validate(&self) -> Result<(), Self::Error> {
        Ok(())
    }
}

#[derive(Debug, Clone, thiserror::Error)]
#[error("configuration validation failed for {field}: {message}")]
pub struct ValidationError {
    pub field: &'static str,
    pub message: String,
}

#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
pub struct StartupConfig {
    #[serde(default = "default_true")]
    pub banner: bool,
    #[serde(default = "default_true")]
    pub show_config: bool,
}

impl Default for StartupConfig {
    fn default() -> Self {
        Self {
            banner: true,
            show_config: true,
        }
    }
}

/// Configuration reserved for the optional `ember-bootui` dependency.
#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
pub struct BootUiConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "default_bootui_host")]
    pub host: String,
    #[serde(default = "default_bootui_port")]
    pub port: u16,
    #[serde(default = "default_bootui_path")]
    pub path: String,
    #[serde(default = "default_true")]
    pub local_only: bool,
}

impl Default for BootUiConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            host: default_bootui_host(),
            port: default_bootui_port(),
            path: default_bootui_path(),
            local_only: true,
        }
    }
}

#[derive(Debug, Clone, Default, serde::Deserialize, serde::Serialize)]
pub struct EmberConfig {
    #[serde(default)]
    pub server: ServerConfig,
    #[serde(default)]
    pub logging: LoggingConfig,
    #[serde(default)]
    pub actuator: ActuatorConfig,
    #[serde(default)]
    pub startup: StartupConfig,
    #[serde(default)]
    pub scheduler: SchedulerConfig,
    #[serde(default)]
    pub security: SecurityConfig,
    #[serde(default)]
    pub bootui: BootUiConfig,
}

impl Config for EmberConfig {
    type Error = ValidationError;

    fn validate(&self) -> Result<(), Self::Error> {
        if self.server.host.trim().is_empty() {
            return Err(ValidationError {
                field: "server.host",
                message: "must not be empty".to_owned(),
            });
        }
        if self.server.port == 0 {
            return Err(ValidationError {
                field: "server.port",
                message: "must be greater than zero".to_owned(),
            });
        }
        if self.actuator.security.enabled && self.actuator.security.bearer_token.is_none() {
            return Err(ValidationError {
                field: "actuator.security.bearer_token",
                message: "must be configured when actuator security is enabled".to_owned(),
            });
        }
        if self.security.validate().is_err() {
            return Err(ValidationError {
                field: "security",
                message: "credentials must be configured when security is enabled".to_owned(),
            });
        }
        if self.bootui.port == 0 {
            return Err(ValidationError {
                field: "bootui.port",
                message: "must be greater than zero".to_owned(),
            });
        }
        if !self.bootui.path.starts_with('/') {
            return Err(ValidationError {
                field: "bootui.path",
                message: "must start with '/'".to_owned(),
            });
        }
        Ok(())
    }
}

fn default_host() -> String {
    "127.0.0.1".to_owned()
}

fn default_port() -> u16 {
    8080
}

fn default_true() -> bool {
    true
}

fn default_bootui_host() -> String {
    "127.0.0.1".to_owned()
}

fn default_bootui_port() -> u16 {
    8091
}

fn default_bootui_path() -> String {
    "/bootui".to_owned()
}
