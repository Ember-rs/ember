use axum::{
    body::{to_bytes, Body},
    http::{Request, StatusCode},
    Json,
};
use ember::prelude::*;
use serde::Deserialize;
use tower::util::ServiceExt;

#[derive(Debug, Deserialize)]
struct GreetingRequest {
    name: String,
}

#[controller("/facade-json")]
struct FacadeJsonController;

#[routes]
impl FacadeJsonController {
    #[post("/greet")]
    async fn greet(&self, payload: JsonBody<GreetingRequest>) -> String {
        format!("Hello, {}!", payload.into_inner().name)
    }

    #[post("/raw")]
    async fn raw(&self, payload: Json<GreetingRequest>) -> String {
        format!("Hello, {}!", payload.0.name)
    }
}

#[tokio::test]
async fn facade_and_prelude_export_json_body() {
    let response = build_router()
        .expect("facade route should build")
        .oneshot(
            Request::post("/facade-json/greet")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"name":"Ada"}"#))
                .expect("request should build"),
        )
        .await
        .expect("router should respond");

    assert_eq!(response.status(), StatusCode::OK);
    let body = to_bytes(response.into_body(), 1024)
        .await
        .expect("response body should be readable");
    assert_eq!(body.as_ref(), b"Hello, Ada!");
}

#[tokio::test]
async fn raw_axum_json_remains_compatible() {
    let response = build_router()
        .expect("facade route should build")
        .oneshot(
            Request::post("/facade-json/raw")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"name":"Ada"}"#))
                .expect("request should build"),
        )
        .await
        .expect("router should respond");

    assert_eq!(response.status(), StatusCode::OK);
    let body = to_bytes(response.into_body(), 1024)
        .await
        .expect("response body should be readable");
    assert_eq!(body.as_ref(), b"Hello, Ada!");
}

#[test]
fn facade_reexports_json_body_and_prelude_imports_it() {
    let payload = ember::JsonBody::from(GreetingRequest {
        name: "Ada".to_owned(),
    });
    assert_eq!(payload.into_inner().name, "Ada");
}
