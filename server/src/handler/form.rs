use axum::{
    extract::State,
    http::{HeaderMap, StatusCode},
    response::{Html, IntoResponse, Response},
    Form,
};
use chrono::Local;
use serde::Deserialize;
use tracing::{info, warn};

use crate::{enqueue_or_broadcast, is_authorized, unauthorized_response, AppState};

const MAX_FORM_CHARS: usize = 244;

#[derive(Deserialize)]
pub struct FormRequest {
    text: String,
}

#[derive(Clone, Copy)]
enum MessageKind {
    Success,
    Error,
}

pub async fn handler(State(state): State<AppState>, headers: HeaderMap) -> Response {
    if !is_authorized(
        &headers,
        state.basic_auth_user.as_str(),
        state.basic_auth_password.as_str(),
    ) {
        return unauthorized_response();
    }

    Html(render_form(None, None, false)).into_response()
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
            Html(render_form(
                Some("print job exceeds maximum length of 244 characters"),
                Some(MessageKind::Error),
                false,
            )),
        )
            .into_response();
    }

    let formatted_text = format_print_job(&payload.text);
    info!("received print job from form:\n {formatted_text}");

    match enqueue_or_broadcast(&state, formatted_text).await {
        Ok(subscriber_count) => {
            info!(subscriber_count, "accepted print message from form");
            Html(render_form(
                Some("print job submitted"),
                Some(MessageKind::Success),
                true,
            ))
            .into_response()
        }
        Err(err) => {
            warn!(error = %err, "failed to queue print message from form");
            (
                StatusCode::SERVICE_UNAVAILABLE,
                Html(render_form(
                    Some("failed to queue print job"),
                    Some(MessageKind::Error),
                    false,
                )),
            )
                .into_response()
        }
    }
}

fn format_print_job(text: &str) -> String {
    let trimmed = text.trim();
    let collapsed = collapse_repeated_newlines(trimmed);
    let timestamp = Local::now().format("%Y-%m-%d %H:%M:%S");

    format!("{timestamp}\n{collapsed}\n------------")
}

fn collapse_repeated_newlines(text: &str) -> String {
    let mut result = String::with_capacity(text.len());
    let mut previous_was_newline = false;

    for ch in text.chars() {
        if ch == '\n' {
            if previous_was_newline {
                continue;
            }
            previous_was_newline = true;
        } else {
            previous_was_newline = false;
        }

        result.push(ch);
    }

    result
}

fn render_form(
    message: Option<&str>,
    message_kind: Option<MessageKind>,
    submitted: bool,
) -> String {
    let message_class = match message_kind {
        Some(MessageKind::Success) => "message message-success",
        Some(MessageKind::Error) => "message message-error",
        None => "message",
    };

    let message_html = message
        .map(|message| format!("<p class=\"{message_class}\">{message}</p>"))
        .unwrap_or_default();

    let textarea_attrs = if submitted {
        " maxlength=\"244\" required disabled"
    } else {
        " maxlength=\"244\" required"
    };
    let button_attrs = if submitted { " disabled" } else { "" };

    format!(
        r#"<!DOCTYPE html>
<html lang="en">
<head>
  <meta charset="utf-8">
  <meta name="viewport" content="width=device-width, initial-scale=1, maximum-scale=1, user-scalable=no">
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
      font-size: 14px;
      margin: 0;
    }}

    .hint {{
      color: #fff;
    }}

    .disclaimer {{
      color: #9ca3af;
    }}

    .message-success {{
      color: #22c55e;
      font-weight: 700;
    }}

    .message-error {{
      color: #f59e0b;
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
      <textarea id="text" name="text"{textarea_attrs}></textarea>
      <p class="hint disclaimer">I may upload photos of the printed text to my blog, so please consider everything you send here public.</p>
      <p class="hint"><span id="chars-left">244</span> characters left</p>
      {message_html}
      <button type="submit" id="submit-button"{button_attrs}>send</button>
    </form>
    <script>
      const maxChars = 244;
      const textarea = document.getElementById("text");
      const charsLeft = document.getElementById("chars-left");
      const submitButton = document.getElementById("submit-button");

      let isSubmitting = false;
      const submitted = textarea.disabled;

      const updateFormState = () => {{
        const remaining = maxChars - Array.from(textarea.value).length;
        charsLeft.textContent = String(remaining);
        submitButton.disabled = submitted || isSubmitting || remaining < 0;
      }};

      textarea.addEventListener("input", updateFormState);
      document.getElementById("print-form").addEventListener("submit", () => {{
        isSubmitting = true;
        submitButton.disabled = true;
      }});
      updateFormState();
    </script>
  </main>
</body>
</html>
"#
    )
}
