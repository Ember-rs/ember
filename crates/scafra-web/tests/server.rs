use axum::{routing::get, Router};
use scafra_foundation::{ShutdownPolicy, ShutdownReason};
use scafra_web::{
    serve_on, serve_on_with_shutdown, shutdown_channel, ControllerRegistration, RouteMetadata,
    WebError,
};
use std::{
    sync::{Mutex, OnceLock},
    time::Duration,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    sync::oneshot,
};

static ROUTES: &[RouteMetadata] = &[RouteMetadata {
    controller: "server-lifecycle-test-controller",
    method: "GET",
    prefix: "/lifecycle",
    path: "/active",
}];

static REQUEST_STARTED: OnceLock<Mutex<Option<oneshot::Sender<()>>>> = OnceLock::new();
static REQUEST_RELEASE: OnceLock<Mutex<Option<oneshot::Receiver<()>>>> = OnceLock::new();

fn register_empty_routes(router: Router) -> Router {
    router.route("/lifecycle/active", get(wait_for_release))
}

async fn wait_for_release() -> &'static str {
    REQUEST_STARTED
        .get()
        .expect("the test should install a request-started channel")
        .lock()
        .expect("the request-started lock should not be poisoned")
        .take()
        .expect("the test should install one request-started sender")
        .send(())
        .expect("the test should still be waiting for the request to start");

    let release = REQUEST_RELEASE
        .get()
        .expect("the test should install a request-release channel")
        .lock()
        .expect("the request-release lock should not be poisoned")
        .take()
        .expect("the test should install one request-release receiver");
    let _ = release.await;
    "request completed"
}

async fn connect_when_ready(address: std::net::SocketAddr) -> TcpStream {
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            match TcpStream::connect(address).await {
                Ok(connection) => return connection,
                Err(error) if error.kind() == std::io::ErrorKind::ConnectionRefused => {
                    tokio::task::yield_now().await;
                }
                Err(error) => panic!("the server connection should succeed: {error}"),
            }
        }
    })
    .await
    .expect("the public server should bind within the readiness deadline")
}

async fn wait_until_listener_closes(address: std::net::SocketAddr) {
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            match TcpStream::connect(address).await {
                Ok(connection) => drop(connection),
                Err(error)
                    if matches!(
                        error.kind(),
                        std::io::ErrorKind::ConnectionRefused | std::io::ErrorKind::ConnectionReset
                    ) =>
                {
                    return;
                }
                Err(error) => panic!("the server listener should close cleanly: {error}"),
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("the server should stop accepting connections after shutdown is requested");
}

inventory::submit! {
    ControllerRegistration {
        controller: "server-bind-test-controller",
        register: register_empty_routes,
        routes: ROUTES,
    }
}

#[tokio::test]
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

#[tokio::test]
async fn public_server_serves_http_drains_active_requests_and_releases_listener() {
    let (started_sender, started_receiver) = oneshot::channel();
    let (release_sender, release_receiver) = oneshot::channel();
    *REQUEST_STARTED
        .get_or_init(|| Mutex::new(None))
        .lock()
        .expect("the request-started lock should not be poisoned") = Some(started_sender);
    *REQUEST_RELEASE
        .get_or_init(|| Mutex::new(None))
        .lock()
        .expect("the request-release lock should not be poisoned") = Some(release_receiver);

    let reservation = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("the test should be able to bind a loopback listener");
    let address = reservation
        .local_addr()
        .expect("the reserved listener should expose its address");
    drop(reservation);

    let (shutdown_handle, shutdown) = shutdown_channel();
    let policy = ShutdownPolicy::new(Duration::from_secs(2), true);
    let server = tokio::spawn(serve_on_with_shutdown(address, shutdown, policy));

    let mut connection = connect_when_ready(address).await;
    connection
        .write_all(
            b"GET /lifecycle/active HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n",
        )
        .await
        .expect("the HTTP request should be written");
    started_receiver
        .await
        .expect("the active request should reach its handler");

    shutdown_handle
        .request()
        .expect("the application should request graceful shutdown");
    wait_until_listener_closes(address).await;
    assert!(
        !server.is_finished(),
        "the server should keep draining the active request after closing its listener"
    );
    release_sender
        .send(())
        .expect("the active request should be released after graceful draining begins");

    let mut response = Vec::new();
    connection
        .read_to_end(&mut response)
        .await
        .expect("the completed HTTP response should be readable");
    let response = String::from_utf8(response).expect("the response should be valid HTTP text");
    assert!(response.starts_with("HTTP/1.1 200 OK\r\n"));
    assert!(response.ends_with("request completed"));

    let outcome = server
        .await
        .expect("the server task should finish")
        .expect("the active request should drain within the configured policy");
    assert_eq!(outcome.reason(), ShutdownReason::ApplicationRequest);

    let rebound = TcpListener::bind(address)
        .await
        .expect("the listener should be released after server shutdown");
    drop(rebound);
}
