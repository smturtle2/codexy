use crate::mapping::{RequestError, RequestErrorKind};
use axum::{
    Json,
    extract::{FromRequest, Request, rejection::JsonRejection},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde_json::{Value, json};

/// Reuse Axum's parser and request body limit, changing only its error response.
pub struct ApiJson(pub Value);
impl<S: Send + Sync> FromRequest<S> for ApiJson {
    type Rejection = ApiError;

    async fn from_request(req: Request, state: &S) -> Result<Self, Self::Rejection> {
        Json::<Value>::from_request(req, state)
            .await
            .map(|Json(value)| Self(value))
            .map_err(ApiError::from)
    }
}

pub struct ApiError {
    status: StatusCode,
    kind: String,
    code: Option<String>,
    message: String,
    param: Option<String>,
}
impl From<JsonRejection> for ApiError {
    fn from(error: JsonRejection) -> Self {
        let status = error.status();
        let message = match &error {
            JsonRejection::JsonSyntaxError(_) => "Request body contains invalid JSON",
            JsonRejection::JsonDataError(_) => "Request JSON could not be decoded",
            JsonRejection::MissingJsonContentType(_) => {
                "Content-Type must be application/json or application/*+json"
            }
            _ if status == StatusCode::PAYLOAD_TOO_LARGE => "Request body exceeds the size limit",
            _ => "Request body could not be read",
        };
        Self {
            status,
            kind: "invalid_request_error".into(),
            code: None,
            message: message.into(),
            param: None,
        }
    }
}
impl From<RequestError> for ApiError {
    fn from(error: RequestError) -> Self {
        Self {
            status: StatusCode::BAD_REQUEST,
            kind: "invalid_request_error".into(),
            code: match error.kind {
                RequestErrorKind::Invalid => None,
                RequestErrorKind::Unsupported => Some("unsupported_parameter".into()),
            },
            message: error.message,
            param: error.param,
        }
    }
}
impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (
            self.status,
            Json(json!({"error": {
                "type": self.kind,
                "code": self.code,
                "message": self.message,
                "param": self.param,
            }})),
        )
            .into_response()
    }
}

pub(crate) fn error(status: StatusCode, code: &str, message: &str) -> Response {
    ApiError {
        status,
        kind: code.into(),
        code: Some(code.into()),
        message: message.into(),
        param: None,
    }
    .into_response()
}
