use axum::{
    body::{to_bytes, Body},
    http::{HeaderValue, Method, Request, StatusCode},
    response::Response,
    routing::get,
    Router,
};
use jsonwebtoken::{encode, EncodingKey, Header};
use scafra::prelude::*;
use scafra::{ActuatorConfig, BasicAuthConfig, JwtConfig, RouteMetadata, SecurityConfig};
use serde::Serialize;
use tower::util::ServiceExt;

#[controller("/policy", authenticated)]
#[derive(Default)]
struct AuthenticatedController;

#[routes(default)]
impl AuthenticatedController {
    #[get("/inherited")]
    async fn inherited(&self) -> &'static str {
        "controller policy"
    }

    #[get("/alias")]
    async fn alias(&self) -> &'static str {
        "controller alias"
    }

    #[get("/users/{id}")]
    async fn user(&self, _id: Path<String>) -> &'static str {
        "protected user"
    }

    #[get("/users/me")]
    #[public]
    async fn current_user(&self) -> &'static str {
        "public current user"
    }

    #[get("/open")]
    #[public]
    async fn open(&self) -> &'static str {
        "explicitly public"
    }
}

#[controller("/public", public)]
#[derive(Default)]
struct PublicController;

#[routes(default)]
impl PublicController {
    #[get("/open")]
    async fn open(&self) -> &'static str {
        "public controller"
    }

    #[get("/private")]
    #[authenticated]
    async fn private(&self) -> &'static str {
        "route override"
    }
}

#[controller("/role", roles("admin", "operator"))]
#[derive(Default)]
struct RoleController;

#[routes(default)]
impl RoleController {
    #[get("/inherited")]
    async fn inherited(&self) -> &'static str {
        "role policy"
    }

    #[get("/scoped")]
    #[scopes("reports.read")]
    async fn scoped(&self) -> &'static str {
        "role and route scope"
    }
}

#[controller("/route", authenticated)]
#[derive(Default)]
struct RoutePolicyController;

#[routes(default)]
impl RoutePolicyController {
    #[get("/role")]
    #[roles("reviewer")]
    async fn role(&self) -> &'static str {
        "route role"
    }
}

#[controller("/slash-policy")]
#[derive(Default)]
struct SlashPolicyController;

#[routes(default)]
impl SlashPolicyController {
    #[get("/resource")]
    #[public]
    async fn public_resource(&self) -> &'static str {
        "public resource"
    }

    #[get("/resource/")]
    #[authenticated]
    async fn protected_resource(&self) -> &'static str {
        "protected resource"
    }
}

#[controller("/public-fallback")]
#[derive(Default)]
struct PublicFallbackController;

#[routes(default)]
impl PublicFallbackController {
    #[get("/known")]
    #[public]
    async fn known(&self) -> &'static str {
        "known public route"
    }
}

#[derive(Serialize)]
struct Claims {
    exp: usize,
    iss: String,
    aud: String,
    roles: Vec<String>,
    scope: String,
}

#[derive(Serialize)]
struct ClaimsWithoutExpiration {
    iss: String,
    aud: String,
    scope: String,
}

#[derive(Serialize)]
struct AlternateClaimShape {
    exp: usize,
    iss: String,
    aud: String,
    role: String,
    scp: Vec<String>,
}

fn security() -> SecurityConfig {
    SecurityConfig {
        enabled: true,
        jwt: JwtConfig {
            enabled: true,
            secret: Some("policy-test-secret".to_owned()),
            issuer_uri: Some("https://policy-issuer.example".to_owned()),
            audiences: vec!["policy-api".to_owned()],
            required_scopes: vec!["api.base".to_owned()],
            ..JwtConfig::default()
        },
        ..SecurityConfig::default()
    }
}

fn token(roles: &[&str], scopes: &str, expiration: usize) -> String {
    encode(
        &Header::default(),
        &Claims {
            exp: expiration,
            iss: "https://policy-issuer.example".to_owned(),
            aud: "policy-api".to_owned(),
            roles: roles.iter().map(|role| (*role).to_owned()).collect(),
            scope: scopes.to_owned(),
        },
        &EncodingKey::from_secret(b"policy-test-secret"),
    )
    .unwrap()
}

fn router() -> Router {
    scafra::web::build_router_with_actuator_and_security(&ActuatorConfig::default(), &security())
        .expect("protected routes should build with JWT security configured")
}

async fn send(
    router: &Router,
    method: Method,
    path: &str,
    authorization: Option<&str>,
) -> Response {
    let mut request = Request::builder().method(method).uri(path);
    if let Some(authorization) = authorization {
        request = request.header("authorization", authorization);
    }
    router
        .clone()
        .oneshot(request.body(Body::empty()).unwrap())
        .await
        .unwrap()
}

#[tokio::test]
async fn controller_and_route_policies_inherit_and_override_explicitly() {
    let router = router();
    let bearer = format!("Bearer {}", token(&[], "api.base", 4_000_000_000));

    assert_eq!(
        send(&router, Method::GET, "/policy/inherited", None)
            .await
            .status(),
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        send(&router, Method::GET, "/policy/inherited", Some(&bearer))
            .await
            .status(),
        StatusCode::OK
    );
    assert_eq!(
        send(&router, Method::GET, "/policy/open", None)
            .await
            .status(),
        StatusCode::OK
    );
    assert_eq!(
        send(&router, Method::GET, "/policy/users/42", None)
            .await
            .status(),
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        send(&router, Method::GET, "/policy/users/42", Some(&bearer))
            .await
            .status(),
        StatusCode::OK
    );
    // The more specific public route wins over the protected parameter route.
    assert_eq!(
        send(&router, Method::GET, "/policy/users/me", None)
            .await
            .status(),
        StatusCode::OK
    );

    // A public controller opts out of the application-wide requirement, while
    // a route-level `authenticated` annotation opts that one route back in.
    assert_eq!(
        send(&router, Method::GET, "/public/open", None)
            .await
            .status(),
        StatusCode::OK
    );
    assert_eq!(
        send(&router, Method::GET, "/public/private", None)
            .await
            .status(),
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        send(&router, Method::GET, "/public/private", Some(&bearer))
            .await
            .status(),
        StatusCode::OK
    );
}

#[tokio::test]
async fn roles_and_scopes_return_forbidden_after_successful_authentication() {
    let router = router();
    let no_role = format!(
        "Bearer {}",
        token(&["reader"], "api.base reports.read", 4_000_000_000)
    );
    let operator = format!("Bearer {}", token(&["operator"], "api.base", 4_000_000_000));
    let full_access = format!(
        "Bearer {}",
        token(
            &["operator", "reviewer"],
            "api.base reports.read",
            4_000_000_000
        )
    );
    let missing_global_scope = format!(
        "Bearer {}",
        token(&["operator"], "reports.read", 4_000_000_000)
    );
    let alternate_claims = format!(
        "Bearer {}",
        encode(
            &Header::default(),
            &AlternateClaimShape {
                exp: 4_000_000_000,
                iss: "https://policy-issuer.example".to_owned(),
                aud: "policy-api".to_owned(),
                role: "reviewer".to_owned(),
                scp: vec!["api.base".to_owned()],
            },
            &EncodingKey::from_secret(b"policy-test-secret"),
        )
        .unwrap()
    );

    assert_eq!(
        send(&router, Method::GET, "/role/inherited", None)
            .await
            .status(),
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        send(&router, Method::GET, "/role/inherited", Some(&no_role))
            .await
            .status(),
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        send(&router, Method::GET, "/role/inherited", Some(&operator))
            .await
            .status(),
        StatusCode::OK
    );
    assert_eq!(
        send(
            &router,
            Method::GET,
            "/role/inherited",
            Some(&missing_global_scope)
        )
        .await
        .status(),
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        send(&router, Method::GET, "/role/scoped", Some(&operator))
            .await
            .status(),
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        send(&router, Method::GET, "/role/scoped", Some(&full_access))
            .await
            .status(),
        StatusCode::OK
    );
    assert_eq!(
        send(&router, Method::GET, "/route/role", Some(&operator))
            .await
            .status(),
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        send(&router, Method::GET, "/route/role", Some(&full_access))
            .await
            .status(),
        StatusCode::OK
    );
    assert_eq!(
        send(&router, Method::GET, "/route/role", Some(&alternate_claims))
            .await
            .status(),
        StatusCode::OK
    );

    let hidden_security = SecurityConfig {
        hide_unauthorized: true,
        ..security()
    };
    let hidden_router = scafra::web::build_router_with_actuator_and_security(
        &ActuatorConfig::default(),
        &hidden_security,
    )
    .expect("the explicitly hidden policy should still build");
    assert_eq!(
        send(
            &hidden_router,
            Method::GET,
            "/role/inherited",
            Some(&no_role)
        )
        .await
        .status(),
        StatusCode::NOT_FOUND
    );
}

#[tokio::test]
async fn malformed_invalid_expired_and_missing_tokens_are_unauthorized() {
    let router = router();
    let expired = token(&[], "api.base", 1);
    let missing_expiration = encode(
        &Header::default(),
        &ClaimsWithoutExpiration {
            iss: "https://policy-issuer.example".to_owned(),
            aud: "policy-api".to_owned(),
            scope: "api.base".to_owned(),
        },
        &EncodingKey::from_secret(b"policy-test-secret"),
    )
    .unwrap();
    let invalid_signature = encode(
        &Header::default(),
        &Claims {
            exp: 4_000_000_000,
            iss: "https://policy-issuer.example".to_owned(),
            aud: "policy-api".to_owned(),
            roles: vec![],
            scope: "api.base".to_owned(),
        },
        &EncodingKey::from_secret(b"wrong-secret"),
    )
    .unwrap();
    let expired = format!("Bearer {expired}");
    let missing_expiration = format!("Bearer {missing_expiration}");
    let invalid_signature = format!("Bearer {invalid_signature}");
    let credentials = [
        None,
        Some("Bearer"),
        Some("Bearer not-a-jwt"),
        Some(missing_expiration.as_str()),
        Some(expired.as_str()),
        Some(invalid_signature.as_str()),
    ];

    for credential in credentials {
        let response = send(&router, Method::GET, "/policy/inherited", credential).await;
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        let body = to_bytes(response.into_body(), 1024).await.unwrap();
        let body = String::from_utf8(body.to_vec()).unwrap();
        assert!(!body.contains("policy-test-secret"));
        assert!(!body.contains("wrong-secret"));
        assert!(!body.contains("not-a-jwt"));
    }
}

#[tokio::test]
async fn aliases_and_alternate_methods_do_not_bypass_controller_policy() {
    let router = router();
    let reader = format!("Bearer {}", token(&["reader"], "api.base", 4_000_000_000));

    // Each declared alias inherits the controller policy. HEAD inherits GET,
    // and an undeclared method cannot use the path to skip authorization.
    assert_eq!(
        send(&router, Method::GET, "/policy/alias", Some(&reader))
            .await
            .status(),
        StatusCode::OK
    );
    assert_eq!(
        send(&router, Method::HEAD, "/policy/alias", None)
            .await
            .status(),
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        send(&router, Method::POST, "/role/inherited", Some(&reader))
            .await
            .status(),
        StatusCode::FORBIDDEN
    );
    // A public annotation applies to its declared method only.
    assert_eq!(
        send(&router, Method::GET, "/policy/open", None)
            .await
            .status(),
        StatusCode::OK
    );
    assert_eq!(
        send(&router, Method::POST, "/policy/open", None)
            .await
            .status(),
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        send(&router, Method::GET, "/role/inherited", Some(&reader))
            .await
            .status(),
        StatusCode::FORBIDDEN
    );
}

#[tokio::test]
async fn trailing_slashes_keep_distinct_route_policies() {
    let router = router();
    let bearer = format!("Bearer {}", token(&[], "api.base", 4_000_000_000));

    assert_eq!(
        send(&router, Method::GET, "/slash-policy/resource", None)
            .await
            .status(),
        StatusCode::OK
    );
    assert_eq!(
        send(&router, Method::GET, "/slash-policy/resource/", None)
            .await
            .status(),
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        send(
            &router,
            Method::GET,
            "/slash-policy/resource/",
            Some(&bearer)
        )
        .await
        .status(),
        StatusCode::OK
    );
}

#[tokio::test]
async fn public_policy_does_not_apply_to_a_fallback_path_variant() {
    let routes = <PublicFallbackController as scafra::web::ControllerRoutes>::route_metadata();
    let router =
        <PublicFallbackController as scafra::web::ControllerRoutes>::register_routes(Router::new())
            .fallback(|| async { "custom fallback" });
    let router = scafra::web::finish_router(
        router,
        routes,
        &ActuatorConfig::default(),
        &security(),
        None,
    )
    .expect("the public route and its fallback should build");

    assert_eq!(
        send(&router, Method::GET, "/public-fallback/known/", None)
            .await
            .status(),
        StatusCode::UNAUTHORIZED
    );
}

#[tokio::test]
async fn protected_route_policies_require_security_to_be_enabled() {
    let error = scafra::web::build_router_with_actuator_and_security(
        &ActuatorConfig::default(),
        &SecurityConfig::default(),
    )
    .expect_err("protected routes must not silently become public");
    assert!(matches!(
        error,
        scafra::WebError::Server(source)
            if source.kind() == std::io::ErrorKind::InvalidInput
    ));
}

#[tokio::test]
async fn disabled_application_security_keeps_unannotated_routes_public() {
    let routes = [RouteMetadata {
        controller: "handwritten-public-controller",
        method: "GET",
        prefix: "",
        path: "/default-public",
    }];
    let router = Router::new().route("/default-public", get(|| async { "public by default" }));
    let router = scafra::web::finish_router(
        router,
        &routes,
        &ActuatorConfig::default(),
        &SecurityConfig::default(),
        None,
    )
    .expect("an unprotected route should build with security disabled");
    let response = router
        .oneshot(
            Request::builder()
                .uri("/default-public")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
async fn shared_bearer_and_basic_credentials_authenticate_http_requests() {
    let bearer_router = scafra::web::build_router_with_actuator_and_security(
        &ActuatorConfig::default(),
        &SecurityConfig {
            enabled: true,
            bearer_token: Some("shared-secret".to_owned()),
            ..SecurityConfig::default()
        },
    )
    .expect("the bearer scheme should configure the router");
    assert_eq!(
        send(
            &bearer_router,
            Method::GET,
            "/policy/inherited",
            Some("Bearer wrong")
        )
        .await
        .status(),
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        send(
            &bearer_router,
            Method::GET,
            "/policy/inherited",
            Some("Bearer shared-secret")
        )
        .await
        .status(),
        StatusCode::OK
    );

    let mut duplicate_headers = Request::builder()
        .method(Method::GET)
        .uri("/policy/inherited")
        .body(Body::empty())
        .unwrap();
    duplicate_headers.headers_mut().append(
        "authorization",
        HeaderValue::from_static("Bearer shared-secret"),
    );
    duplicate_headers
        .headers_mut()
        .append("authorization", HeaderValue::from_static("Bearer wrong"));
    assert_eq!(
        bearer_router
            .clone()
            .oneshot(duplicate_headers)
            .await
            .unwrap()
            .status(),
        StatusCode::UNAUTHORIZED
    );

    let basic_router = scafra::web::build_router_with_actuator_and_security(
        &ActuatorConfig::default(),
        &SecurityConfig {
            enabled: true,
            basic: BasicAuthConfig {
                username: Some("user".to_owned()),
                password: Some("pass".to_owned()),
            },
            ..SecurityConfig::default()
        },
    )
    .expect("the basic scheme should configure the router");
    assert_eq!(
        send(
            &basic_router,
            Method::GET,
            "/policy/inherited",
            Some("Basic dXNlcjpwYXNz")
        )
        .await
        .status(),
        StatusCode::OK
    );
    assert_eq!(
        send(
            &basic_router,
            Method::GET,
            "/role/inherited",
            Some("Basic dXNlcjpwYXNz")
        )
        .await
        .status(),
        StatusCode::FORBIDDEN
    );
}
