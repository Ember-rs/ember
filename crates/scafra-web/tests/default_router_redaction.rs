use axum::{
    body::{to_bytes, Body},
    http::{Request, StatusCode},
    routing::get,
    Router,
};
use scafra_web::{build_router, ControllerRegistration, RouteMetadata};
use std::{
    io::{self, Write},
    sync::{Arc, Mutex},
};
use tower::util::ServiceExt;

#[derive(Clone)]
struct SharedWriter(Arc<Mutex<Vec<u8>>>);

impl Write for SharedWriter {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.0
            .lock()
            .expect("test writer lock should not be poisoned")
            .extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

static ROUTES: &[RouteMetadata] = &[RouteMetadata {
    controller: "default-router-redaction-controller",
    method: "GET",
    prefix: "",
    path: "/audit/{token}",
}];

async fn audit_probe() -> &'static str {
    "ok"
}

fn register_audit_probe(router: Router) -> Router {
    router.route("/audit/{token}", get(audit_probe))
}

inventory::submit! {
    ControllerRegistration {
        controller: "default-router-redaction-controller",
        register: register_audit_probe,
        routes: ROUTES,
    }
}

#[test]
fn default_router_handles_requests_without_logging_sensitive_uri_values() {
    let output = Arc::new(Mutex::new(Vec::new()));
    let writer_output = Arc::clone(&output);
    let subscriber = tracing_subscriber::fmt()
        .with_ansi(false)
        .with_max_level(tracing::Level::DEBUG)
        .with_writer(move || SharedWriter(Arc::clone(&writer_output)))
        .finish();
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("test runtime should build");

    tracing::subscriber::with_default(subscriber, || {
        runtime.block_on(async {
            let response = build_router()
                .expect("default router should build")
                .oneshot(
                    Request::builder()
                        .method("GET")
                        .uri("/audit/path-secret?token=query-secret")
                        .header("authorization", "header-secret")
                        .body(Body::from("request-body-secret"))
                        .expect("request should build"),
                )
                .await
                .expect("router should respond");

            assert_eq!(response.status(), StatusCode::OK);
            let body = to_bytes(response.into_body(), 1024)
                .await
                .expect("response body should be readable");
            assert_eq!(body.as_ref(), b"ok");
        });
    });

    let logs = String::from_utf8(
        output
            .lock()
            .expect("test writer lock should not be poisoned")
            .clone(),
    )
    .expect("trace output should be UTF-8");

    assert!(
        logs.contains("started processing request"),
        "debug request trace was not emitted: {logs}"
    );
    assert!(
        !logs.contains("path-secret"),
        "request path leaked into framework trace output: {logs}"
    );
    assert!(
        !logs.contains("query-secret"),
        "request query leaked into framework trace output: {logs}"
    );
    assert!(
        !logs.contains("header-secret"),
        "request header leaked into framework trace output: {logs}"
    );
    assert!(
        !logs.contains("request-body-secret"),
        "request body leaked into framework trace output: {logs}"
    );
}
