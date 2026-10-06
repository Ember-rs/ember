use axum::{
    body::{to_bytes, Body},
    extract::DefaultBodyLimit,
    http::{Request, StatusCode},
    routing::post,
    Json, Router,
};
use ember_web::{build_router, ControllerRegistration, JsonBody, RouteMetadata};
use serde::{Deserialize, Serialize};
use tower::{service_fn, util::ServiceExt};

#[derive(Debug, Deserialize, Serialize)]
struct UserRequest {
    name: String,
}

static ROUTES: &[RouteMetadata] = &[
    RouteMetadata {
        controller: "json-body-controller",
        method: "POST",
        prefix: "",
        path: "/json-body",
    },
    RouteMetadata {
        controller: "json-body-controller",
        method: "POST",
        prefix: "",
        path: "/json-body/custom-limit",
    },
];

async fn json_body_handler(JsonBody(payload): JsonBody<UserRequest>) -> Json<UserRequest> {
    Json(payload)
}

fn register_json_body_route(router: Router) -> Router {
    router.route("/json-body", post(json_body_handler)).route(
        "/json-body/custom-limit",
        post(json_body_handler).layer(DefaultBodyLimit::max(8)),
    )
}

inventory::submit! {
    ControllerRegistration {
        controller: "json-body-controller",
        register: register_json_body_route,
        routes: ROUTES,
    }
}

async fn send_json_body(request: Request<Body>) -> axum::response::Response {
    build_router()
        .expect("JSON body test route should build")
        .oneshot(request)
        .await
        .expect("router should respond")
}

async fn response_body(response: axum::response::Response) -> Vec<u8> {
    to_bytes(response.into_body(), 1024 * 1024 * 2)
        .await
        .expect("response body should be readable")
        .to_vec()
}

async fn assert_json_error(
    response: axum::response::Response,
    status: StatusCode,
    expected_body: &[u8],
) {
    assert_eq!(response.status(), status);
    assert_eq!(
        response
            .headers()
            .get("content-type")
            .and_then(|value| value.to_str().ok()),
        Some("application/json"),
    );
    assert_eq!(response_body(response).await, expected_body);
}

#[tokio::test]
async fn valid_json_body_reaches_the_handler() {
    let response = send_json_body(
        Request::post("/json-body")
            .header("content-type", "application/json")
            .body(Body::from(r#"{"name":"Ada"}"#))
            .expect("request should build"),
    )
    .await;

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response
            .headers()
            .get("content-type")
            .and_then(|value| value.to_str().ok()),
        Some("application/json"),
    );
    assert_eq!(response_body(response).await, br#"{"name":"Ada"}"#);
}

#[tokio::test]
async fn syntax_errors_are_stable_and_redacted() {
    let secret = "syntax-secret";
    let response = send_json_body(
        Request::post("/json-body")
            .header("content-type", "application/json")
            .body(Body::from(format!(r#"{{"name":"{secret}"}} trailing"#)))
            .expect("request should build"),
    )
    .await;

    assert_json_error(
        response,
        StatusCode::BAD_REQUEST,
        br#"{"code":"invalid_json","message":"invalid JSON request"}"#,
    )
    .await;
}

#[tokio::test]
async fn semantic_errors_are_stable_and_redacted() {
    let secret = "semantic-secret";
    let response = send_json_body(
        Request::post("/json-body")
            .header("content-type", "application/json")
            .body(Body::from(format!(r#"{{"name":{{"secret":"{secret}"}}}}"#)))
            .expect("request should build"),
    )
    .await;

    assert_json_error(
        response,
        StatusCode::UNPROCESSABLE_ENTITY,
        br#"{"code":"validation_error","message":"request validation failed"}"#,
    )
    .await;
}

#[tokio::test]
async fn missing_json_content_type_is_stable_and_redacted() {
    let secret = "header-secret";
    let response = send_json_body(
        Request::post("/json-body")
            .header("x-sensitive-header", secret)
            .body(Body::from(format!(r#"{{"name":"{secret}"}}"#)))
            .expect("request should build"),
    )
    .await;

    assert_json_error(
        response,
        StatusCode::UNSUPPORTED_MEDIA_TYPE,
        br#"{"code":"unsupported_media_type","message":"content type must be application/json"}"#,
    )
    .await;
}

#[tokio::test]
async fn non_json_content_type_is_stable_and_redacted() {
    let secret = "media-type-secret";
    let response = send_json_body(
        Request::post("/json-body")
            .header("content-type", "text/plain")
            .body(Body::from(format!(r#"{{"name":"{secret}"}}"#)))
            .expect("request should build"),
    )
    .await;

    assert_json_error(
        response,
        StatusCode::UNSUPPORTED_MEDIA_TYPE,
        br#"{"code":"unsupported_media_type","message":"content type must be application/json"}"#,
    )
    .await;
}

#[tokio::test]
async fn oversized_json_body_is_stable_and_redacted() {
    let secret = b"oversized-secret";
    let mut payload = Vec::with_capacity(1024 * 1024 + secret.len());
    while payload.len() <= 1024 * 1024 {
        payload.extend_from_slice(secret);
    }

    let response = send_json_body(
        Request::post("/json-body")
            .header("content-type", "application/json")
            .body(Body::from(payload))
            .expect("request should build"),
    )
    .await;

    assert_json_error(
        response,
        StatusCode::PAYLOAD_TOO_LARGE,
        br#"{"code":"payload_too_large","message":"request body too large"}"#,
    )
    .await;
}

#[tokio::test]
async fn unknown_body_errors_are_stable_and_redacted() {
    let secret = "body-stream-secret";
    let failing_body = service_fn(move |_| async move {
        Err::<Vec<u8>, _>(std::io::Error::other(format!(
            "request body failed with secret {secret}"
        )))
    })
    .call_all(Body::from("trigger").into_data_stream());
    let response = send_json_body(
        Request::post("/json-body")
            .header("content-type", "application/json")
            .body(Body::from_stream(failing_body))
            .expect("request should build"),
    )
    .await;

    assert_json_error(
        response,
        StatusCode::INTERNAL_SERVER_ERROR,
        br#"{"code":"internal_error","message":"internal server error"}"#,
    )
    .await;
}

#[tokio::test]
async fn application_json_suffix_remains_accepted_by_axum_policy() {
    let response = send_json_body(
        Request::post("/json-body")
            .header("content-type", "application/vnd.api+json")
            .body(Body::from(r#"{"name":"Ada"}"#))
            .expect("request should build"),
    )
    .await;

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response_body(response).await, br#"{"name":"Ada"}"#);
}

#[tokio::test]
async fn json_body_inherits_a_route_specific_body_limit() {
    let response = send_json_body(
        Request::post("/json-body/custom-limit")
            .header("content-type", "application/json")
            .body(Body::from(r#"{"name":"Ada"}"#))
            .expect("request should build"),
    )
    .await;

    assert_json_error(
        response,
        StatusCode::PAYLOAD_TOO_LARGE,
        br#"{"code":"payload_too_large","message":"request body too large"}"#,
    )
    .await;
}
