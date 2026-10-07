use axum::Router;
use scafra_web::{serve_on, ControllerRegistration, RouteMetadata, WebError};
use tokio::net::TcpListener;

static ROUTES: &[RouteMetadata] = &[];

fn register_empty_routes(router: Router) -> Router {
    router
}

inventory::submit! {
    ControllerRegistration {
        controller: "server-bind-test-controller",
        register: register_empty_routes,
        routes: ROUTES,
    }
}

#[tokio::test]
#[ignore = "requires loopback listener permission; run with --include-ignored"]
async fn serve_on_reports_the_requested_address_when_binding_fails() {
    let listener = match TcpListener::bind("127.0.0.1:0").await {
        Ok(listener) => listener,
        Err(error) if error.kind() == std::io::ErrorKind::PermissionDenied => {
            panic!(
                "loopback listener permission is denied; this test is explicitly ignored in the default test run"
            );
        }
        Err(error) => panic!("the test must be able to reserve a local listener: {error}"),
    };
    let address = listener
        .local_addr()
        .expect("the reserved listener must expose its address");

    let error = serve_on(address)
        .await
        .expect_err("serve_on must report an occupied address");

    assert!(matches!(
        error,
        WebError::Bind {
            address: reported,
            ..
        } if reported == address
    ));
}
