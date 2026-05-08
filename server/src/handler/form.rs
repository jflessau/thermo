use axum::{
    extract::State,
    http::{HeaderMap, StatusCode},
    response::{Html, IntoResponse, Response},
    Form,
};
use serde::Deserialize;
use tracing::{info, warn};

use crate::{is_authorized, unauthorized_response, AppState};

const MAX_FORM_CHARS: usize = 244;

#[derive(Deserialize)]
pub struct FormRequest {
    text: String,
}

pub async fn handler(State(state): State<AppState>, headers: HeaderMap) -> Response {
    if !is_authorized(
        &headers,
        state.basic_auth_user.as_str(),
        state.basic_auth_password.as_str(),
    ) {
        return unauthorized_response();
    }

    Html(render_form(None)).into_response()
}

pub async fn submit_handler(
    State(state): State<AppState>,
    headers: HeaderMap,
    Form(payload): Form<FormRequest>,
) -> Response {
    if !is_authorized(
        &headers,
        state.basic_auth_user.as_str(),
        state.basic_auth_password.as_str(),
    ) {
        return unauthorized_response();
    }

    if payload.text.chars().count() > MAX_FORM_CHARS {
        return (
            StatusCode::PAYLOAD_TOO_LARGE,
            Html(render_form(Some(
                "print job exceeds maximum length of 244 characters",
            ))),
        )
            .into_response();
    }

    match state.broadcaster.send(payload.text) {
        Ok(subscriber_count) => {
            info!(subscriber_count, "queued print message from form");
            Html(render_form(Some("print job submitted"))).into_response()
        }
        Err(err) => {
            warn!(error = %err, "failed to queue print message from form");
            (
                StatusCode::SERVICE_UNAVAILABLE,
                Html(render_form(Some("no printer clients connected"))),
            )
                .into_response()
        }
    }
}

fn render_form(message: Option<&str>) -> String {
    let message_html = message
        .map(|message| format!("<p class=\"message\">{message}</p>"))
        .unwrap_or_default();

    format!(
        r#"<!DOCTYPE html>
<html lang="en">
<head>
  <meta charset="utf-8">
  <meta name="viewport" content="width=device-width, initial-scale=1">
  <title>Print Job</title>
  <style>
    :root {{
      color-scheme: dark;
    }}

    * {{
      box-sizing: border-box;
    }}

    body {{
      margin: 0;
      min-height: 100vh;
      background: #000;
      color: #fff;
      font-family: monospace;
      display: flex;
      align-items: center;
      justify-content: center;
      padding: 24px;
    }}

    main {{
      width: min(100%, 720px);
    }}

    form {{
      display: flex;
      flex-direction: column;
      gap: 12px;
    }}

    label {{
      color: #fff;
      font-family: monospace;
      font-size: 16px;
    }}

    textarea {{
      width: 100%;
      min-height: 280px;
      resize: vertical;
      padding: 16px;
      border: 1px solid #fff;
      background: #000;
      color: #fff;
      font: inherit;
      outline: none;
    }}

    textarea:focus {{
      border: 2px solid #fff;
    }}

    .hint,
    .message {{
      color: #fff;
      font-size: 14px;
      margin: 0;
    }}

    button {{
      align-self: flex-start;
      border: 1px solid #fff;
      background: #fff;
      color: #000;
      padding: 12px 18px;
      font: inherit;
      cursor: pointer;
    }}

    button:disabled {{
      opacity: 0.5;
      cursor: not-allowed;
    }}

    button:active:not(:disabled) {{
      background: #808080;
    }}
  </style>
</head>
<body>
  <main>
    <form method="post" action="/form" id="print-form">
      <label for="text">message</label>
      <textarea id="text" name="text" maxlength="244" required></textarea>
      <p class="hint"><span id="chars-left">244</span> characters left</p>
      {message_html}
      <button type="submit" id="submit-button">send</button>
    </form>
    <script>
      const maxChars = 244;
      const textarea = document.getElementById("text");
      const charsLeft = document.getElementById("chars-left");
      const submitButton = document.getElementById("submit-button");

      const updateFormState = () => {{
        const remaining = maxChars - Array.from(textarea.value).length;
        charsLeft.textContent = String(remaining);
        submitButton.disabled = remaining < 0;
      }};

      textarea.addEventListener("input", updateFormState);
      updateFormState();
    </script>
  </main>
</body>
</html>
"#
    )
}
