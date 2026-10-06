use super::*;
use axum::{body::Body, http::Request, routing::get, Router};
use ember_foundation::{ShutdownPolicy, ShutdownReason};
use std::{sync::Arc, time::Duration};
use tokio::{
    io::AsyncWriteExt,
    net::{TcpListener, TcpStream},
    sync::Notify,
};
use tower::ServiceExt;

static COMPATIBILITY_ROUTES: &[RouteMetadata] = &[RouteMetadata {
    controller: "compatibility-controller",
    method: "GET",
    prefix: "/compatibility",
    path: "/probe",
}];

fn register_compatibility_route(router: Router) -> Router {
    router
}

async fn bind_test_listener() -> TcpListener {
    match TcpListener::bind("127.0.0.1:0").await {
        Ok(listener) => listener,
        Err(error) if error.kind() == std::io::ErrorKind::PermissionDenied => {
            panic!(
                "loopback listener permission is denied; this test is explicitly ignored in the default test run"
            )
        }
        Err(error) => panic!("the lifecycle test must be able to bind a local listener: {error}"),
    }
}

inventory::submit! {
    ControllerRegistration {
        controller: "compatibility-controller",
        register: register_compatibility_route,
        routes: COMPATIBILITY_ROUTES,
    }
}

inventory::submit! {
    ControllerRegistration {
        controller: "compatibility-controller-duplicate",
        register: register_compatibility_route,
        routes: COMPATIBILITY_ROUTES,
    }
}

#[test]
fn joins_controller_and_route_paths() {
    assert_eq!(join_paths("/api/", "/users/{id}"), "/api/users/{id}");
    assert_eq!(join_paths("", "/health"), "/health");
    assert_eq!(join_paths("/", "/health"), "/health");
}

#[tokio::test]
async fn internal_errors_do_not_expose_details_in_http_responses() {
    use axum::{body::to_bytes, response::IntoResponse};

    let response = AppError::Internal("database password".to_owned()).into_response();
    let body = to_bytes(response.into_body(), 1024).await.unwrap();
    let body = String::from_utf8(body.to_vec()).unwrap();
    assert!(!body.contains("database password"));
    assert!(body.contains("internal server error"));
}

#[test]
fn internal_error_logs_do_not_expose_details() {
    use axum::response::IntoResponse;
    use std::{
        io::{self, Write},
        sync::{Arc, Mutex},
    };

    #[derive(Clone, Default)]
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

    tracing::subscriber::with_default(subscriber, || {
        let _ = AppError::Internal("database password".to_owned()).into_response();
    });

    let logs = String::from_utf8(
        output
            .lock()
            .expect("writer lock should not be poisoned")
            .clone(),
    )
    .expect("diagnostics should be UTF-8");
    assert!(
        !logs.contains("database password"),
        "secret leaked in: {logs}"
    );
    assert!(
        logs.contains("request failed"),
        "missing stable event in: {logs}"
    );
}

#[tokio::test]
async fn textual_log_level_runner_remains_a_compatible_entry_point() {
    let error = run_on_with_log_level(
        "127.0.0.1:0".parse().expect("test address should parse"),
        "info",
    )
    .await
    .expect_err("the test registrations should stop before binding");

    assert!(matches!(error, WebError::DuplicateRoute { .. }));
}

#[tokio::test]
async fn shutdown_handle_maps_an_application_request() {
    let (handle, shutdown) = shutdown_channel();
    handle
        .request()
        .expect("the application request should be delivered");

    assert_eq!(shutdown.await, ShutdownReason::ApplicationRequest);
}

#[tokio::test]
async fn shutdown_handle_preserves_an_explicit_signal_reason() {
    let (handle, shutdown) = shutdown_channel();
    handle
        .request_with_reason(ShutdownReason::Signal)
        .expect("the explicit signal reason should be delivered");

    assert_eq!(shutdown.await, ShutdownReason::Signal);
}

#[test]
fn graceful_timeout_preserves_reason_and_maps_to_a_timed_out_legacy_error() {
    let error = ServerError::GracefulShutdownTimeout {
        reason: ShutdownReason::RuntimeFailure,
        policy: ShutdownPolicy::new(Duration::from_millis(20), true),
    };

    assert_eq!(error.timeout_reason(), Some(ShutdownReason::RuntimeFailure));
    assert_eq!(error.timeout_duration(), Some(Duration::from_millis(20)));

    let legacy = error.into_web_error();
    assert!(matches!(
        legacy,
        WebError::Server(source)
            if source.kind() == std::io::ErrorKind::TimedOut
                && source.to_string() == "Ember graceful shutdown exceeded its configured deadline"
    ));
}

#[tokio::test]
async fn in_process_http_remains_an_axum_escape_hatch() {
    let router = Router::new().route("/probe", get(|| async { "ok" }));
    let response = router
        .oneshot(
            Request::builder()
                .uri("/probe")
                .body(Body::empty())
                .expect("the request should be valid"),
        )
        .await
        .expect("the router should produce a response");

    assert_eq!(response.status(), axum::http::StatusCode::OK);
}

#[tokio::test]
#[ignore = "requires loopback listener permission; run with --include-ignored"]
async fn active_requests_drain_before_a_clean_shutdown() {
    let started = Arc::new(Notify::new());
    let route_started = Arc::clone(&started);
    let router = Router::new().route(
        "/slow",
        get(move || {
            let route_started = Arc::clone(&route_started);
            async move {
                route_started.notify_one();
                tokio::time::sleep(Duration::from_millis(20)).await;
                "ok"
            }
        }),
    );
    let listener = bind_test_listener().await;
    let address = listener
        .local_addr()
        .expect("the test listener should have an address");
    let (handle, shutdown) = shutdown_channel();
    let server = tokio::spawn(crate::server::serve_listener(
        listener,
        router,
        async move { Ok(shutdown.await) },
        ShutdownPolicy::new(Duration::from_secs(1), true),
    ));

    let mut connection = TcpStream::connect(address)
        .await
        .expect("the server should accept an in-process request");
    let request_started = started.notified();
    connection
        .write_all(b"GET /slow HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")
        .await
        .expect("the request should be written");
    request_started.await;
    handle
        .request()
        .expect("the application request should be delivered");

    let outcome = server
        .await
        .expect("the server task should finish")
        .expect("the active request should drain cleanly");
    assert_eq!(outcome.reason(), ShutdownReason::ApplicationRequest);
}

#[tokio::test]
#[ignore = "requires loopback listener permission; run with --include-ignored"]
async fn active_requests_report_a_distinct_graceful_timeout() {
    let started = Arc::new(Notify::new());
    let route_started = Arc::clone(&started);
    let router = Router::new().route(
        "/slow",
        get(move || {
            let route_started = Arc::clone(&route_started);
            async move {
                route_started.notify_one();
                tokio::time::sleep(Duration::from_secs(30)).await;
                "never returned"
            }
        }),
    );
    let listener = bind_test_listener().await;
    let address = listener
        .local_addr()
        .expect("the test listener should have an address");
    let (handle, shutdown) = shutdown_channel();
    let server = tokio::spawn(crate::server::serve_listener(
        listener,
        router,
        async move { Ok(shutdown.await) },
        ShutdownPolicy::new(Duration::from_millis(20), true),
    ));

    let mut connection = TcpStream::connect(address)
        .await
        .expect("the server should accept an in-process request");
    let request_started = started.notified();
    connection
        .write_all(b"GET /slow HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")
        .await
        .expect("the request should be written");
    request_started.await;
    handle
        .request()
        .expect("the application request should be delivered");

    let error = server
        .await
        .expect("the server task should finish")
        .expect_err("the active request should exceed the drain deadline");
    assert!(matches!(
        error,
        ServerError::GracefulShutdownTimeout {
            reason: ShutdownReason::ApplicationRequest,
            policy,
        } if policy.grace_period() == Duration::from_millis(20)
    ));
}
