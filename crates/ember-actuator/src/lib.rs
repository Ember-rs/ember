//! Configurable operational endpoints for Ember applications.

use axum::{
    extract::Extension,
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::get,
    Json, Router,
};
use serde::{Deserialize, Serialize};

/// Selects which built-in operational endpoint groups are installed.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(untagged)]
pub enum EndpointSelection {
    /// A YAML list such as `['health', 'info']`.
    Names(Vec<String>),
    /// A properties-friendly value such as `health,info` or `*`.
    Pattern(String),
}

impl Default for EndpointSelection {
    fn default() -> Self {
        Self::Names(Vec::new())
    }
}

/// Controls which Ember actuator endpoint groups are installed.
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct ActuatorConfig {
    /// Endpoint groups: `health`, `live`, `ready`, `info`, or `*` for all.
    #[serde(default)]
    pub endpoints: EndpointSelection,
    #[serde(default)]
    pub health: HealthConfig,
    #[serde(default)]
    pub security: ActuatorSecurity,
}

/// Controls which registered health checks are evaluated. An empty list means
/// all registered checks.
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct HealthConfig {
    #[serde(default)]
    pub checks: Vec<String>,
}

/// Optional bearer-token protection for actuator endpoints.
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct ActuatorSecurity {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default, skip_serializing)]
    pub bearer_token: Option<String>,
}

/// A health check supplied by an Ember application or adapter.
pub struct HealthCheckRegistration {
    pub name: &'static str,
    pub check: fn() -> bool,
}

inventory::collect!(HealthCheckRegistration);

pub use inventory;

/// Registers a synchronous actuator health check.
#[macro_export]
macro_rules! register_health_check {
    ($name:literal, $check:path) => {
        $crate::inventory::submit! {
            $crate::HealthCheckRegistration { name: $name, check: $check }
        }
    };
}

impl ActuatorConfig {
    /// Enables every actuator endpoint group.
    pub fn all() -> Self {
        Self {
            endpoints: EndpointSelection::Pattern("*".to_owned()),
            health: HealthConfig::default(),
            security: ActuatorSecurity::default(),
        }
    }

    pub fn is_enabled(&self, endpoint: &str) -> bool {
        match &self.endpoints {
            EndpointSelection::Names(names) => {
                names.iter().any(|name| name == "*" || name == endpoint)
            }
            EndpointSelection::Pattern(pattern) => pattern
                .split(',')
                .map(str::trim)
                .any(|name| name == "*" || name == endpoint),
        }
    }

    /// Returns the enabled endpoint groups in stable display order.
    pub fn enabled_endpoints(&self) -> Vec<&'static str> {
        ["health", "live", "ready", "info", "metrics"]
            .into_iter()
            .filter(|endpoint| self.is_enabled(endpoint))
            .collect()
    }
}

#[derive(Debug, Clone, Copy, Serialize)]
struct HealthResponse {
    status: &'static str,
}

#[derive(Debug, Clone, Copy, Serialize)]
struct InfoResponse {
    name: &'static str,
    version: &'static str,
}

fn authorized(headers: &HeaderMap, config: &ActuatorConfig) -> bool {
    if !config.security.enabled {
        return true;
    }
    let Some(token) = config.security.bearer_token.as_deref() else {
        return false;
    };
    headers
        .get("authorization")
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value == format!("Bearer {token}"))
}

fn protected(headers: &HeaderMap, config: &ActuatorConfig) -> Option<Response> {
    (!authorized(headers, config)).then(|| StatusCode::UNAUTHORIZED.into_response())
}

fn health_up(config: &ActuatorConfig) -> bool {
    inventory::iter::<HealthCheckRegistration>().all(|registration| {
        config.health.checks.is_empty()
            || !config
                .health
                .checks
                .iter()
                .any(|name| name != "*" && name == registration.name)
            || (registration.check)()
    })
}

async fn health(headers: HeaderMap, Extension(config): Extension<ActuatorConfig>) -> Response {
    if let Some(response) = protected(&headers, &config) {
        return response;
    }
    let up = health_up(&config);
    (
        if up {
            StatusCode::OK
        } else {
            StatusCode::SERVICE_UNAVAILABLE
        },
        Json(HealthResponse {
            status: if up { "UP" } else { "DOWN" },
        }),
    )
        .into_response()
}

async fn info(headers: HeaderMap, Extension(config): Extension<ActuatorConfig>) -> Response {
    if let Some(response) = protected(&headers, &config) {
        return response;
    }
    (
        StatusCode::OK,
        Json(InfoResponse {
            name: "ember",
            version: env!("CARGO_PKG_VERSION"),
        }),
    )
        .into_response()
}

async fn metrics(headers: HeaderMap, Extension(config): Extension<ActuatorConfig>) -> Response {
    if let Some(response) = protected(&headers, &config) {
        return response;
    }
    (StatusCode::OK, "# TYPE ember_up gauge\nember_up 1\n").into_response()
}

/// Adds the built-in operational routes when the actuator is enabled.
pub fn router(config: &ActuatorConfig) -> Router {
    let mut router = Router::new();
    if config.is_enabled("health") {
        router = router
            .route("/health", get(health))
            .route("/actuator/health", get(health));
    }
    if config.is_enabled("live") {
        router = router
            .route("/live", get(health))
            .route("/health/live", get(health))
            .route("/actuator/health/liveness", get(health));
    }
    if config.is_enabled("ready") {
        router = router
            .route("/ready", get(health))
            .route("/health/ready", get(health))
            .route("/actuator/health/readiness", get(health));
    }
    if config.is_enabled("info") {
        router = router
            .route("/info", get(info))
            .route("/actuator/info", get(info));
    }
    if config.is_enabled("metrics") {
        router = router
            .route("/metrics", get(metrics))
            .route("/actuator/metrics", get(metrics));
    }
    router.layer(Extension(config.clone()))
}

/// Returns the built-in paths that must not be shadowed by application routes.
pub fn reserved_routes(config: &ActuatorConfig) -> Vec<(&'static str, &'static str)> {
    ROUTES
        .iter()
        .copied()
        .filter(|(_, path)| match *path {
            "/health" | "/actuator/health" => config.is_enabled("health"),
            "/live" | "/health/live" | "/actuator/health/liveness" => config.is_enabled("live"),
            "/ready" | "/health/ready" | "/actuator/health/readiness" => config.is_enabled("ready"),
            "/info" | "/actuator/info" => config.is_enabled("info"),
            "/metrics" | "/actuator/metrics" => config.is_enabled("metrics"),
            _ => false,
        })
        .collect()
}

const ROUTES: [(&str, &str); 12] = [
    ("GET", "/health"),
    ("GET", "/health/live"),
    ("GET", "/health/ready"),
    ("GET", "/live"),
    ("GET", "/ready"),
    ("GET", "/info"),
    ("GET", "/actuator/health"),
    ("GET", "/actuator/health/liveness"),
    ("GET", "/actuator/health/readiness"),
    ("GET", "/actuator/info"),
    ("GET", "/metrics"),
    ("GET", "/actuator/metrics"),
];
