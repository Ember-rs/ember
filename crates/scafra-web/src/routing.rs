use std::collections::HashMap;

use axum::{extract::DefaultBodyLimit, Router};
use tower_http::trace::TraceLayer;

use crate::{errors::WebError, registration::ControllerRegistration};
use scafra_actuator::ActuatorConfig;

/// Joins a controller prefix and method path while preserving Axum route
/// syntax, including path parameters such as {id}.
pub fn join_paths(prefix: &str, path: &str) -> String {
    let prefix = prefix.trim_end_matches('/');
    let path = path.trim_start_matches('/');
    match (prefix.is_empty(), path.is_empty()) {
        (true, true) => "/".to_owned(),
        (true, false) => format!("/{path}"),
        (false, true) => prefix.to_owned(),
        (false, false) => format!("{prefix}/{path}"),
    }
}

/// Builds the application router from all statically linked controllers.
pub fn build_router() -> Result<Router, WebError> {
    build_router_with_actuator(&ActuatorConfig::default())
}

/// Builds the application router with configurable operational endpoints.
pub fn build_router_with_actuator(actuator: &ActuatorConfig) -> Result<Router, WebError> {
    build_router_with_actuator_and_security(actuator, &scafra_security::SecurityConfig::default())
}

pub fn build_router_with_actuator_and_security(
    actuator: &ActuatorConfig,
    security: &scafra_security::SecurityConfig,
) -> Result<Router, WebError> {
    let mut seen = HashMap::<(String, String), &'static str>::new();
    for (method, path) in scafra_actuator::reserved_routes(actuator) {
        seen.insert(
            ((*method).to_owned(), (*path).to_owned()),
            "scafra-actuator",
        );
    }
    for registration in inventory::iter::<ControllerRegistration> {
        for route in registration.routes {
            let key = (
                route.method.to_owned(),
                join_paths(route.prefix, route.path),
            );
            if let Some(first) = seen.insert(key.clone(), route.controller) {
                return Err(WebError::DuplicateRoute {
                    method: key.0,
                    path: key.1,
                    first,
                    second: route.controller,
                });
            }
        }
    }

    let router = inventory::iter::<ControllerRegistration>()
        .fold(scafra_actuator::router(actuator), |router, registration| {
            (registration.register)(router)
        })
        .layer(DefaultBodyLimit::max(1024 * 1024))
        .layer(
            TraceLayer::new_for_http().make_span_with(|request: &axum::http::Request<_>| {
                tracing::debug_span!(
                    "request",
                    method = %request.method(),
                    version = ?request.version(),
                )
            }),
        );

    Ok(scafra_security::layer(router, security.clone()))
}
