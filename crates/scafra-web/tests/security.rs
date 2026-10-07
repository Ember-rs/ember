use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use jsonwebtoken::{encode, EncodingKey, Header};
use scafra_web::{build_router_with_actuator_and_security, ActuatorConfig, SecurityConfig};
use serde::Serialize;
use tower::util::ServiceExt;

#[tokio::test]
async fn security_can_hide_unauthorized_routes_as_not_found() {
    let security = SecurityConfig {
        enabled: true,
        hide_unauthorized: true,
        bearer_token: Some("secret".to_owned()),
        ..SecurityConfig::default()
    };
    let response = build_router_with_actuator_and_security(&ActuatorConfig::all(), &security)
        .unwrap()
        .oneshot(Request::builder().uri("/info").body(Body::empty()).unwrap())
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[derive(Serialize)]
struct Claims {
    sub: String,
    exp: usize,
    iss: String,
    aud: String,
    scope: String,
}

#[tokio::test]
async fn jwt_requires_valid_claims_and_scopes() {
    let security = SecurityConfig {
        enabled: true,
        jwt: scafra_web::JwtConfig {
            enabled: true,
            secret: Some("test-secret".to_owned()),
            issuer_uri: Some("https://issuer.example".to_owned()),
            audiences: vec!["scafra-api".to_owned()],
            required_scopes: vec!["read".to_owned()],
            ..Default::default()
        },
        ..SecurityConfig::default()
    };
    let router =
        build_router_with_actuator_and_security(&ActuatorConfig::all(), &security).unwrap();
    let token = encode(
        &Header::default(),
        &Claims {
            sub: "user-1".to_owned(),
            exp: 4_000_000_000,
            iss: "https://issuer.example".to_owned(),
            aud: "scafra-api".to_owned(),
            scope: "read write".to_owned(),
        },
        &EncodingKey::from_secret(b"test-secret"),
    )
    .unwrap();

    let response = router
        .oneshot(
            Request::builder()
                .uri("/info")
                .header("authorization", format!("Bearer {token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
}
