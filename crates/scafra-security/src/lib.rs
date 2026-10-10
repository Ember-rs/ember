//! Configurable HTTP authentication and route authorization for Scafra.

use axum::{
    http::{Method, Request, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
    Router,
};
use base64::{engine::general_purpose::STANDARD, Engine as _};
use jsonwebtoken::{decode, decode_header, Algorithm, DecodingKey, Validation};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{collections::HashSet, sync::Arc};
use tokio::sync::RwLock;

/// Whether a controller or route inherits the application-wide policy,
/// explicitly allows anonymous access, or requires authentication.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum AuthorizationMode {
    #[default]
    Inherit,
    Public,
    Protected,
}

/// Authorization requirements declared on a controller or route.
///
/// Every role group must match at least one role. Every listed scope is
/// required. An empty policy with [`AuthorizationMode::Inherit`] uses the
/// application-wide security setting.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct AuthorizationPolicy {
    pub mode: AuthorizationMode,
    pub role_groups: &'static [&'static [&'static str]],
    pub required_scopes: &'static [&'static str],
}

impl AuthorizationPolicy {
    /// Inherit the application-wide security setting.
    pub const fn inherit() -> Self {
        Self {
            mode: AuthorizationMode::Inherit,
            role_groups: &[],
            required_scopes: &[],
        }
    }

    /// Explicitly allow anonymous access.
    pub const fn public() -> Self {
        Self {
            mode: AuthorizationMode::Public,
            role_groups: &[],
            required_scopes: &[],
        }
    }

    /// Require authentication.
    pub const fn protected() -> Self {
        Self {
            mode: AuthorizationMode::Protected,
            role_groups: &[],
            required_scopes: &[],
        }
    }

    /// Returns whether this policy explicitly requires authentication.
    pub fn is_protected(self) -> bool {
        self.mode == AuthorizationMode::Protected
    }
}

/// Authorization metadata for one macro-generated controller route.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RouteAuthorizationMetadata {
    pub controller: &'static str,
    pub method: &'static str,
    pub prefix: &'static str,
    pub path: &'static str,
    pub controller_policy: AuthorizationPolicy,
    pub route_policy: AuthorizationPolicy,
}

/// Link-time route policy metadata emitted by the `#[routes]` macro.
pub struct ControllerAuthorizationRegistration {
    pub controller: &'static str,
    pub routes: &'static [RouteAuthorizationMetadata],
}

inventory::collect!(ControllerAuthorizationRegistration);

/// HTTP security defaults for a Scafra application.
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct SecurityConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub hide_unauthorized: bool,
    #[serde(default)]
    pub bearer_token: Option<String>,
    #[serde(default)]
    pub basic: BasicAuthConfig,
    #[serde(default)]
    pub permit_all: Vec<String>,
    #[serde(default)]
    pub jwt: JwtConfig,
}

/// JWT resource-server configuration.
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct JwtConfig {
    #[serde(default)]
    pub enabled: bool,
    /// HMAC secret for local HS256 validation.
    #[serde(default, skip_serializing)]
    pub secret: Option<String>,
    /// OIDC issuer used for discovery and `iss` validation.
    #[serde(default)]
    pub issuer_uri: Option<String>,
    /// Direct JWKS endpoint; avoids issuer discovery when supplied.
    #[serde(default)]
    pub jwk_set_uri: Option<String>,
    #[serde(default)]
    pub audiences: Vec<String>,
    /// Scopes required on every authenticated JWT request unless a route is
    /// explicitly public.
    #[serde(default)]
    pub required_scopes: Vec<String>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct BasicAuthConfig {
    #[serde(default)]
    pub username: Option<String>,
    #[serde(default, skip_serializing)]
    pub password: Option<String>,
}

impl SecurityConfig {
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.enabled
            && self.bearer_token.is_none()
            && (self.basic.username.is_none() || self.basic.password.is_none())
            && !self.jwt.enabled
        {
            return Err("a bearer token or complete basic credentials are required");
        }
        if self.enabled && self.bearer_token.as_deref() == Some("") {
            return Err("the bearer token must not be empty");
        }
        if self.enabled
            && ((self.basic.username.is_some() && self.basic.username.as_deref() == Some(""))
                || (self.basic.password.is_some() && self.basic.password.as_deref() == Some("")))
        {
            return Err("basic credentials must not be empty");
        }
        if self.jwt.enabled
            && self.jwt.secret.is_none()
            && self.jwt.issuer_uri.is_none()
            && self.jwt.jwk_set_uri.is_none()
        {
            return Err("JWT requires secret, issuer_uri, or jwk_set_uri");
        }
        if self.jwt.enabled && self.jwt.secret.as_deref() == Some("") {
            return Err("the JWT secret must not be empty");
        }
        Ok(())
    }

    pub fn is_permitted(&self, path: &str) -> bool {
        self.permit_all.iter().any(|permitted| permitted == path)
    }
}

/// Applies application-wide security to an application router.
pub fn layer(router: Router, config: SecurityConfig) -> Router {
    layer_with_route_policies(router, config, Vec::new())
}

/// Applies application-wide and generated route-level authorization policies.
pub fn layer_with_route_policies(
    router: Router,
    config: SecurityConfig,
    policies: Vec<RouteAuthorizationMetadata>,
) -> Router {
    if !config.enabled && policies.is_empty() {
        return router;
    }
    let state = SecurityState {
        config,
        policies: Arc::new(policies),
        jwks: Arc::new(RwLock::new(None)),
    };
    router.layer(axum::middleware::from_fn(move |request, next| {
        let state = state.clone();
        async move { authorize_request(state, request, next).await }
    }))
}

#[derive(Clone)]
struct SecurityState {
    config: SecurityConfig,
    policies: Arc<Vec<RouteAuthorizationMetadata>>,
    jwks: Arc<RwLock<Option<JwkSet>>>,
}

#[derive(Debug, Clone, Deserialize)]
struct JwkSet {
    keys: Vec<JwkKey>,
}

#[derive(Debug, Clone, Deserialize)]
struct JwkKey {
    kid: Option<String>,
    kty: String,
    n: Option<String>,
    e: Option<String>,
}

#[derive(Debug, Default)]
struct Principal {
    roles: HashSet<String>,
    scopes: HashSet<String>,
    is_jwt: bool,
}

async fn authorize_request(
    state: SecurityState,
    request: Request<axum::body::Body>,
    next: Next,
) -> Response {
    let path = request.uri().path();
    let policy = matching_policy(&state.policies, request.method(), path);
    let is_public = policy.is_some_and(ResolvedPolicy::is_public);

    if is_public || state.config.is_permitted(path) && policy.is_none() {
        return next.run(request).await;
    }

    let globally_protected = state.config.enabled && !state.config.is_permitted(path);
    let route_protected = policy.is_some_and(ResolvedPolicy::is_protected);
    if !globally_protected && !route_protected {
        return next.run(request).await;
    }

    let mut authorization_values = request
        .headers()
        .get_all(axum::http::header::AUTHORIZATION)
        .iter();
    let authorization = match (authorization_values.next(), authorization_values.next()) {
        (Some(_), Some(_)) => return denied(&state, StatusCode::UNAUTHORIZED),
        (Some(value), None) => value.to_str().ok(),
        (None, _) => None,
    };
    let Some(principal) = authenticate(&state, authorization).await else {
        return denied(&state, StatusCode::UNAUTHORIZED);
    };

    let policy = policy.unwrap_or_default();
    let mut scopes = policy
        .controller_scopes
        .iter()
        .chain(policy.route_scopes.iter())
        .copied()
        .chain(
            principal
                .is_jwt
                .then_some(state.config.jwt.required_scopes.iter().map(String::as_str))
                .into_iter()
                .flatten(),
        );
    let authorized = policy
        .controller_role_groups
        .iter()
        .chain(policy.route_role_groups.iter())
        .all(|group| group.iter().any(|role| principal.roles.contains(*role)))
        && scopes.all(|scope| principal.scopes.contains(scope));
    if !authorized {
        return denied(&state, StatusCode::FORBIDDEN);
    }

    next.run(request).await
}

fn denied(state: &SecurityState, status: StatusCode) -> Response {
    if state.config.hide_unauthorized {
        StatusCode::NOT_FOUND.into_response()
    } else if status == StatusCode::UNAUTHORIZED {
        let challenge = if state.config.jwt.enabled || state.config.bearer_token.is_some() {
            "Bearer"
        } else {
            "Basic realm=\"Scafra\""
        };
        (status, [("www-authenticate", challenge)]).into_response()
    } else {
        status.into_response()
    }
}

#[derive(Debug, Clone, Copy, Default)]
struct ResolvedPolicy {
    is_public: bool,
    is_protected: bool,
    controller_role_groups: &'static [&'static [&'static str]],
    route_role_groups: &'static [&'static [&'static str]],
    controller_scopes: &'static [&'static str],
    route_scopes: &'static [&'static str],
}

impl ResolvedPolicy {
    fn is_public(self) -> bool {
        self.is_public
    }

    fn is_protected(self) -> bool {
        self.is_protected
    }
}

fn matching_policy(
    policies: &[RouteAuthorizationMetadata],
    method: &Method,
    path: &str,
) -> Option<ResolvedPolicy> {
    let method = if method == Method::HEAD {
        "GET"
    } else {
        method.as_str()
    };
    let matches = policies
        .iter()
        .filter(|policy| route_path_matches(policy, path))
        .collect::<Vec<_>>();
    if matches.is_empty() {
        return None;
    }

    let method_matches = matches
        .iter()
        .copied()
        .filter(|policy| policy.method.eq_ignore_ascii_case(method))
        .collect::<Vec<_>>();
    if !method_matches.is_empty() {
        // Axum prefers static path segments over parameters. Matching the most
        // specific declaration avoids a broad parameter policy overriding a
        // more specific static route.
        return method_matches
            .into_iter()
            .max_by(|left, right| path_specificity(left).cmp(&path_specificity(right)))
            .map(resolve_policy);
    }

    // A method with no matching route must not bypass a protected path and
    // then reach a different fallback. Public access stays method-specific.
    matches
        .into_iter()
        .map(resolve_policy)
        .filter(|policy| policy.is_protected)
        .max_by_key(|policy| {
            (
                policy.controller_role_groups.len() + policy.route_role_groups.len(),
                policy.controller_scopes.len() + policy.route_scopes.len(),
            )
        })
}

fn resolve_policy(metadata: &RouteAuthorizationMetadata) -> ResolvedPolicy {
    let controller = metadata.controller_policy;
    let route = metadata.route_policy;
    if route.mode == AuthorizationMode::Public {
        return ResolvedPolicy {
            is_public: true,
            ..ResolvedPolicy::default()
        };
    }
    if route.mode == AuthorizationMode::Inherit && controller.mode == AuthorizationMode::Public {
        return ResolvedPolicy {
            is_public: true,
            ..ResolvedPolicy::default()
        };
    }

    let is_protected = route.mode == AuthorizationMode::Protected
        || controller.mode == AuthorizationMode::Protected;
    ResolvedPolicy {
        is_public: false,
        is_protected,
        controller_role_groups: if is_protected {
            controller.role_groups
        } else {
            &[]
        },
        route_role_groups: if is_protected { route.role_groups } else { &[] },
        controller_scopes: if is_protected {
            controller.required_scopes
        } else {
            &[]
        },
        route_scopes: if is_protected {
            route.required_scopes
        } else {
            &[]
        },
    }
}

fn route_path_matches(metadata: &RouteAuthorizationMetadata, request_path: &str) -> bool {
    let pattern = join_paths(metadata.prefix, metadata.path);
    // Axum distinguishes `/path` from `/path/`. Keep the empty final segment
    // instead of normalizing it away so a public route cannot grant access to
    // a different route (or a fallback) with a trailing slash.
    let pattern_segments = path_segments(&pattern);
    let request_segments = path_segments(request_path);
    let mut request_index = 0;
    for (pattern_index, segment) in pattern_segments.iter().enumerate() {
        if let Some(rest) = segment
            .strip_prefix("{*")
            .and_then(|value| value.strip_suffix('}'))
        {
            let Some(remaining) = request_segments.get(request_index..) else {
                return false;
            };
            return !rest.is_empty()
                && !remaining.is_empty()
                && (remaining.len() > 1 || !remaining[0].is_empty());
        }
        let Some(request_segment) = request_segments.get(request_index) else {
            return false;
        };
        if segment.starts_with('{') {
            if request_segment.is_empty() {
                return false;
            }
        } else if segment != request_segment {
            return false;
        }
        request_index += 1;
        if pattern_index + 1 == pattern_segments.len() && request_index != request_segments.len() {
            return false;
        }
    }
    request_index == request_segments.len()
}

fn path_segments(path: &str) -> Vec<&str> {
    path.strip_prefix('/').unwrap_or(path).split('/').collect()
}

fn path_specificity(metadata: &RouteAuthorizationMetadata) -> Vec<u8> {
    let path = join_paths(metadata.prefix, metadata.path);
    path_segments(&path)
        .into_iter()
        .map(|segment| {
            if segment.starts_with("{*") {
                0
            } else if segment.starts_with('{') {
                1
            } else {
                2
            }
        })
        .collect()
}

fn join_paths(prefix: &str, path: &str) -> String {
    let prefix = prefix.trim_end_matches('/');
    let path = path.trim_start_matches('/');
    match (prefix.is_empty(), path.is_empty()) {
        (true, true) => "/".to_owned(),
        (true, false) => format!("/{path}"),
        (false, true) => prefix.to_owned(),
        (false, false) => format!("{prefix}/{path}"),
    }
}

async fn authenticate(state: &SecurityState, authorization: Option<&str>) -> Option<Principal> {
    let authorization = authorization?;
    let (scheme, credential) = authorization.split_once(' ')?;
    if credential.is_empty() || credential.bytes().any(|byte| byte.is_ascii_whitespace()) {
        return None;
    }

    if scheme.eq_ignore_ascii_case("bearer") {
        if state.config.jwt.enabled {
            return validate_jwt(state, credential).await;
        }
        if state.config.bearer_token.as_deref() == Some(credential) {
            return Some(Principal::default());
        }
        return None;
    }

    if scheme.eq_ignore_ascii_case("basic") {
        let encoded = credential;
        let decoded = STANDARD.decode(encoded).ok()?;
        let credentials = String::from_utf8(decoded).ok()?;
        let (username, password) = credentials.split_once(':')?;
        if state.config.basic.username.as_deref() == Some(username)
            && state.config.basic.password.as_deref() == Some(password)
        {
            return Some(Principal::default());
        }
    }
    None
}

async fn validate_jwt(state: &SecurityState, token: &str) -> Option<Principal> {
    let header = decode_header(token).ok()?;
    let config = &state.config.jwt;
    let key = if let Some(secret) = config.secret.as_deref() {
        if header.alg != Algorithm::HS256 {
            return None;
        }
        DecodingKey::from_secret(secret.as_bytes())
    } else {
        let kid = header.kid.as_deref()?;
        let jwks = match state.jwks.read().await.clone() {
            Some(jwks) => jwks,
            None => {
                let jwks = fetch_jwks(config).await?;
                *state.jwks.write().await = Some(jwks.clone());
                jwks
            }
        };
        let key = jwks
            .keys
            .iter()
            .find(|key| key.kid.as_deref() == Some(kid))?;
        if key.kty != "RSA"
            || !matches!(
                header.alg,
                Algorithm::RS256 | Algorithm::RS384 | Algorithm::RS512
            )
        {
            return None;
        }
        let (n, e) = (key.n.as_deref()?, key.e.as_deref()?);
        DecodingKey::from_rsa_components(n, e).ok()?
    };

    let mut validation = Validation::new(header.alg);
    validation.set_required_spec_claims(&["exp"]);
    if let Some(issuer) = config.issuer_uri.as_deref() {
        validation.set_issuer(&[issuer]);
    }
    if !config.audiences.is_empty() {
        validation.set_audience(&config.audiences);
    }
    let data = decode::<Value>(token, &key, &validation).ok()?;
    Some(principal_from_claims(&data.claims))
}

fn principal_from_claims(claims: &Value) -> Principal {
    let mut principal = Principal {
        is_jwt: true,
        ..Principal::default()
    };
    if let Some(roles) = claims.get("roles").and_then(Value::as_array) {
        principal.roles.extend(roles.iter().filter_map(|value| {
            value
                .as_str()
                .filter(|role| !role.is_empty())
                .map(str::to_owned)
        }));
    }
    if let Some(role) = claims.get("role") {
        match role {
            Value::String(role) if !role.is_empty() => {
                principal.roles.insert(role.clone());
            }
            Value::Array(roles) => principal.roles.extend(roles.iter().filter_map(|value| {
                value
                    .as_str()
                    .filter(|role| !role.is_empty())
                    .map(str::to_owned)
            })),
            _ => {}
        }
    }
    if let Some(scope) = claims.get("scope").and_then(Value::as_str) {
        principal
            .scopes
            .extend(scope.split_whitespace().map(str::to_owned));
    }
    if let Some(scopes) = claims.get("scp").and_then(Value::as_array) {
        principal.scopes.extend(scopes.iter().filter_map(|value| {
            value
                .as_str()
                .filter(|scope| !scope.is_empty())
                .map(str::to_owned)
        }));
    }
    principal
}

async fn fetch_jwks(config: &JwtConfig) -> Option<JwkSet> {
    let uri = if let Some(uri) = config.jwk_set_uri.as_deref() {
        uri.to_owned()
    } else {
        let issuer = config.issuer_uri.as_deref()?;
        let metadata = format!(
            "{}/.well-known/openid-configuration",
            issuer.trim_end_matches('/')
        );
        reqwest::get(metadata)
            .await
            .ok()?
            .json::<Value>()
            .await
            .ok()?
            .get("jwks_uri")?
            .as_str()?
            .to_owned()
    };
    reqwest::get(uri).await.ok()?.json::<JwkSet>().await.ok()
}
