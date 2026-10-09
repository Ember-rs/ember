use axum::{
    body::{to_bytes, Body},
    http::{Request, StatusCode},
};
use scafra::prelude::*;
use tower::util::ServiceExt;

#[service]
#[derive(Default)]
struct GreetingService;

impl GreetingService {
    fn greet(&self, name: &str) -> String {
        format!("Hello, {name}!")
    }
}

#[controller("/test")]
#[derive(Default)]
struct GreetingController {
    service: GreetingService,
}

#[routes(default)]
impl GreetingController {
    #[get("/hello/{name}")]
    async fn hello(&self, name: Path<String>) -> String {
        self.service.greet(&name)
    }
}

#[tokio::test]
async fn generated_controller_serves_a_request() {
    let app = build_router().expect("test route should be valid");
    let response = app
        .oneshot(
            Request::builder()
                .uri("/test/hello/Alice")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body = to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
    assert_eq!(&body[..], b"Hello, Alice!");
}
