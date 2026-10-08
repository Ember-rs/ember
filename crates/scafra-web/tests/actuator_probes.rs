use axum::{
    body::{to_bytes, Body},
    http::{Request, StatusCode},
};
use scafra_web::{build_router_with_actuator, ActuatorConfig, ActuatorSecurity, HealthConfig};
use tower::util::ServiceExt;

fn unavailable_dependency() -> bool {
    false
}

// Keep this failing registration isolated in its own integration-test binary.
// The inventory is immutable, so parallel tests all observe the same check.
scafra_web::register_health_check!("dependency", unavailable_dependency);

async fn get(config: &ActuatorConfig, path: &str) -> (StatusCode, String) {
    let response = build_router_with_actuator(config)
        .expect("the actuator router should build")
        .oneshot(
            Request::builder()
                .uri(path)
                .body(Body::empty())
                .expect("request should build"),
        )
        .await
        .expect("router should respond");
    let status = response.status();
    let body = to_bytes(response.into_body(), 1024)
        .await
        .expect("response body should be readable");
    (
        status,
        String::from_utf8(body.to_vec()).expect("body should be UTF-8"),
    )
}

fn config_with_dependency_check() -> ActuatorConfig {
    ActuatorConfig {
        health: HealthConfig {
            checks: vec!["dependency".to_owned()],
        },
        ..ActuatorConfig::all()
    }
}

#[tokio::test]
async fn failed_registered_check_marks_every_readiness_alias_down() {
    let config = config_with_dependency_check();

    for path in ["/ready", "/health/ready", "/actuator/health/readiness"] {
        let (status, body) = get(&config, path).await;
        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE, "{path}");
        assert_eq!(body, r#"{"status":"DOWN"}"#, "{path}");
    }
}

#[tokio::test]
async fn failed_registered_check_does_not_mark_liveness_aliases_down() {
    let config = config_with_dependency_check();

    for path in ["/live", "/health/live", "/actuator/health/liveness"] {
        let (status, body) = get(&config, path).await;
        assert_eq!(status, StatusCode::OK, "{path}");
        assert_eq!(body, r#"{"status":"UP"}"#, "{path}");
    }
}

#[tokio::test]
async fn aggregate_health_keeps_evaluating_the_configured_checks() {
    let config = config_with_dependency_check();

    for path in ["/health", "/actuator/health"] {
        let (status, body) = get(&config, path).await;
        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE, "{path}");
        assert_eq!(body, r#"{"status":"DOWN"}"#, "{path}");
    }
}

#[tokio::test]
async fn readiness_respects_health_check_selection() {
    let config = ActuatorConfig {
        health: HealthConfig {
            checks: vec!["another-check".to_owned()],
        },
        ..ActuatorConfig::all()
    };

    let (status, body) = get(&config, "/ready").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, r#"{"status":"UP"}"#);
}

#[tokio::test]
async fn bearer_authentication_covers_aggregate_liveness_and_readiness_routes() {
    let config = ActuatorConfig {
        security: ActuatorSecurity {
            enabled: true,
            bearer_token: Some("probe-token".to_owned()),
        },
        ..config_with_dependency_check()
    };

    for path in [
        "/health",
        "/actuator/health",
        "/live",
        "/health/live",
        "/actuator/health/liveness",
        "/ready",
        "/health/ready",
        "/actuator/health/readiness",
    ] {
        let (status, _) = get(&config, path).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED, "{path}");
    }
}
