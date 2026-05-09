use axum::{
    extract::State,
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use serde::Deserialize;
use tracing::{info, warn};

use crate::{enqueue_or_broadcast, is_authorized, unauthorized_response, AppState};

const MAX_PRINT_JOB_CHARS: usize = 2_500;

#[derive(Deserialize)]
pub struct PrintRequest {
    text: String,
}

pub async fn handler(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(payload): Json<PrintRequest>,
) -> Response {
    if payload.text.chars().count() > MAX_PRINT_JOB_CHARS {
        return (
            StatusCode::PAYLOAD_TOO_LARGE,
            format!("print job exceeds maximum length of {MAX_PRINT_JOB_CHARS} characters"),
        )
            .into_response();
    }

    if !is_authorized(
        &headers,
        state.basic_auth_user.as_str(),
        state.basic_auth_password.as_str(),
    ) {
        return unauthorized_response();
    }

    match enqueue_or_broadcast(&state, payload.text).await {
        Ok(subscriber_count) => {
            info!(subscriber_count, "accepted print message");
            StatusCode::ACCEPTED.into_response()
        }
        Err(err) => {
            warn!(error = %err, "failed to queue print message");
            (StatusCode::SERVICE_UNAVAILABLE, "failed to queue print job").into_response()
        }
    }
}
