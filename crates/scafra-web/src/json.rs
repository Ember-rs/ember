use axum::extract::{
    rejection::{BytesRejection, FailedToBufferBody, JsonRejection},
    FromRequest, Json,
};
use axum::{http::StatusCode, response::Response};
use serde::de::DeserializeOwned;

use crate::errors::error_response;

/// A typed JSON request body with Scafra's stable rejection responses.
///
/// This extractor delegates deserialization and body buffering to Axum. It is
/// request-only; use Axum's [`Json`] or an application-owned response type for
/// response serialization.
#[derive(Debug, Clone, Copy, Default)]
pub struct JsonBody<T>(pub T);

impl<T> JsonBody<T> {
    /// Returns the deserialized request body.
    pub fn into_inner(self) -> T {
        self.0
    }
}

impl<T> From<T> for JsonBody<T> {
    fn from(value: T) -> Self {
        Self(value)
    }
}

impl<T, S> FromRequest<S> for JsonBody<T>
where
    T: DeserializeOwned,
    S: Send + Sync,
{
    type Rejection = Response;

    async fn from_request(
        request: axum::extract::Request,
        state: &S,
    ) -> Result<Self, Self::Rejection> {
        Json::<T>::from_request(request, state)
            .await
            .map(|Json(value)| Self(value))
            .map_err(json_rejection_response)
    }
}

fn json_rejection_response(rejection: JsonRejection) -> Response {
    match rejection {
        JsonRejection::JsonSyntaxError(_) => error_response(
            StatusCode::BAD_REQUEST,
            "invalid_json",
            "invalid JSON request",
        ),
        JsonRejection::JsonDataError(_) => error_response(
            StatusCode::UNPROCESSABLE_ENTITY,
            "validation_error",
            "request validation failed",
        ),
        JsonRejection::MissingJsonContentType(_) => error_response(
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
            "unsupported_media_type",
            "content type must be application/json",
        ),
        JsonRejection::BytesRejection(BytesRejection::FailedToBufferBody(
            FailedToBufferBody::LengthLimitError(_),
        )) => error_response(
            StatusCode::PAYLOAD_TOO_LARGE,
            "payload_too_large",
            "request body too large",
        ),
        JsonRejection::BytesRejection(rejection) => match rejection {
            BytesRejection::FailedToBufferBody(FailedToBufferBody::LengthLimitError(_)) => {
                error_response(
                    StatusCode::PAYLOAD_TOO_LARGE,
                    "payload_too_large",
                    "request body too large",
                )
            }
            BytesRejection::FailedToBufferBody(FailedToBufferBody::UnknownBodyError(_)) => {
                error_response(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "internal_error",
                    "internal server error",
                )
            }
            _ => error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal_error",
                "internal server error",
            ),
        },
        _ => error_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            "internal_error",
            "internal server error",
        ),
    }
}
