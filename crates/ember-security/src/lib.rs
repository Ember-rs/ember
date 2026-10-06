//! Configurable HTTP security for Ember applications.

use axum::{
    http::{Request, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
    Router,
};
use base64::{engine::general_purpose::STANDARD, Engine as _};
use jsonwebtoken::{decode, decode_header, Algorithm, DecodingKey, Validation};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::Arc;
use tokio::sync::RwLock;

/// HTTP security defaults for an Ember application.
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
        if self.jwt.enabled
            && self.jwt.secret.is_none()
            && self.jwt.issuer_uri.is_none()
            && self.jwt.jwk_set_uri.is_none()
        {
            return Err("JWT requires secret, issuer_uri, or jwk_set_uri");
        }
        Ok(())
    }

    pub fn is_permitted(&self, path: &str) -> bool {
        self.permit_all.iter().any(|permitted| permitted == path)
    }
}

/// Applies Ember's security middleware to an application router.
pub fn layer(router: Router, config: SecurityConfig) -> Router {
    if !config.enabled {
        router
    } else {
        let state = SecurityState {
            config,
            jwks: Arc::new(RwLock::new(None)),
        };
        router.layer(axum::middleware::from_fn(move |request, next| {
            let state = state.clone();
            async move { authenticate(state, request, next).await }
        }))
    }
}

#[derive(Clone)]
struct SecurityState {
    config: SecurityConfig,
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

async fn authenticate(
    state: SecurityState,
    request: Request<axum::body::Body>,
    next: Next,
) -> Response {
    let path = request.uri().path().to_owned();
    let authorization = request
        .headers()
        .get("authorization")
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned);
    if state.config.is_permitted(&path) || authenticated(&state, authorization.as_deref()).await {
        return next.run(request).await;
    }

    if state.config.hide_unauthorized {
        StatusCode::NOT_FOUND.into_response()
    } else {
        (StatusCode::UNAUTHORIZED, [("www-authenticate", "Bearer")]).into_response()
    }
}

async fn authenticated(state: &SecurityState, authorization: Option<&str>) -> bool {
    let Some(value) = authorization else {
        return false;
    };
    if state.config.jwt.enabled {
        return validate_jwt(state, value.strip_prefix("Bearer ").unwrap_or(value)).await;
    }
    if let Some(token) = state.config.bearer_token.as_deref() {
        return value == format!("Bearer {token}");
    }
    let Some(encoded) = value.strip_prefix("Basic ") else {
        return false;
    };
    let Ok(decoded) = STANDARD.decode(encoded) else {
        return false;
    };
    let Ok(credentials) = String::from_utf8(decoded) else {
        return false;
    };
    let Some((username, password)) = credentials.split_once(':') else {
        return false;
    };
    state.config.basic.username.as_deref() == Some(username)
        && state.config.basic.password.as_deref() == Some(password)
}

async fn validate_jwt(state: &SecurityState, token: &str) -> bool {
    let Ok(header) = decode_header(token) else {
        return false;
    };
    let config = &state.config.jwt;
    let key = if let Some(secret) = config.secret.as_deref() {
        if header.alg != Algorithm::HS256 {
            return false;
        }
        DecodingKey::from_secret(secret.as_bytes())
    } else {
        let Some(kid) = header.kid.as_deref() else {
            return false;
        };
        let jwks = match state.jwks.read().await.clone() {
            Some(jwks) => jwks,
            None => {
                let Some(jwks) = fetch_jwks(config).await else {
                    return false;
                };
                *state.jwks.write().await = Some(jwks.clone());
                jwks
            }
        };
        let Some(key) = jwks.keys.iter().find(|key| key.kid.as_deref() == Some(kid)) else {
            return false;
        };
        if key.kty != "RSA"
            || !matches!(
                header.alg,
                Algorithm::RS256 | Algorithm::RS384 | Algorithm::RS512
            )
        {
            return false;
        }
        let (Some(n), Some(e)) = (key.n.as_deref(), key.e.as_deref()) else {
            return false;
        };
        let Ok(key) = DecodingKey::from_rsa_components(n, e) else {
            return false;
        };
        key
    };

    let mut validation = Validation::new(header.alg);
    if let Some(issuer) = config.issuer_uri.as_deref() {
        validation.set_issuer(&[issuer]);
    }
    if !config.audiences.is_empty() {
        validation.set_audience(&config.audiences);
    }
    let Ok(data) = decode::<Value>(token, &key, &validation) else {
        return false;
    };
    config.required_scopes.iter().all(|required| {
        data.claims
            .get("scope")
            .and_then(Value::as_str)
            .is_some_and(|scope| scope.split_whitespace().any(|value| value == required))
            || data
                .claims
                .get("scp")
                .and_then(Value::as_array)
                .is_some_and(|scopes| scopes.iter().any(|value| value.as_str() == Some(required)))
    })
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
