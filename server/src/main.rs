use std::{env, net::SocketAddr, sync::Arc};

use anyhow::{Context, Result, bail};
use axum::{
    Json, Router,
    extract::{
        State, WebSocketUpgrade,
        ws::{Message, WebSocket},
    },
    http::{HeaderMap, StatusCode, header},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use serde::Deserialize;
use tokio::sync::broadcast;
use tracing::{info, warn};

#[derive(Clone)]
struct AppState {
    broadcaster: broadcast::Sender<String>,
    basic_auth_user: Arc<String>,
    basic_auth_password: Arc<String>,
}

#[derive(Deserialize)]
struct PrintRequest {
    text: String,
}

#[tokio::main]
async fn main() -> Result<()> {
    dotenv::dotenv().ok();
    init_tracing();

    let basic_auth_user = Arc::new(required_env("BASIC_AUTH_USER")?);
    let basic_auth_password = Arc::new(required_env("BASIC_AUTH_PASSWORD")?);
    let bind_addr = env::var("BIND_ADDR")
        .unwrap_or_else(|_| "0.0.0.0:3000".to_string())
        .parse::<SocketAddr>()
        .context("BIND_ADDR must be a valid socket address")?;

    let (broadcaster, _) = broadcast::channel(256);

    let app = Router::new()
        .route("/print", post(print_handler))
        .route("/print/ws", get(print_ws_handler))
        .with_state(AppState {
            broadcaster,
            basic_auth_user: Arc::clone(&basic_auth_user),
            basic_auth_password: Arc::clone(&basic_auth_password),
        });

    let listener = tokio::net::TcpListener::bind(bind_addr)
        .await
        .with_context(|| format!("failed to bind server to {bind_addr}"))?;

    info!(%bind_addr, user = %basic_auth_user, "print relay server listening");

    axum::serve(listener, app)
        .await
        .context("server exited unexpectedly")?;

    Ok(())
}

async fn print_handler(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(payload): Json<PrintRequest>,
) -> Response {
    if !is_authorized(
        &headers,
        state.basic_auth_user.as_str(),
        state.basic_auth_password.as_str(),
    ) {
        return unauthorized_response();
    }

    match state.broadcaster.send(payload.text) {
        Ok(subscriber_count) => {
            info!(subscriber_count, "queued print message");
            StatusCode::ACCEPTED.into_response()
        }
        Err(err) => {
            warn!(error = %err, "failed to queue print message");
            (
                StatusCode::SERVICE_UNAVAILABLE,
                "no printer clients connected",
            )
                .into_response()
        }
    }
}

async fn print_ws_handler(
    ws: WebSocketUpgrade,
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Response {
    if !is_authorized(
        &headers,
        state.basic_auth_user.as_str(),
        state.basic_auth_password.as_str(),
    ) {
        return unauthorized_response();
    }

    ws.on_upgrade(move |socket| handle_ws_connection(socket, state.broadcaster.subscribe()))
}

async fn handle_ws_connection(mut socket: WebSocket, mut rx: broadcast::Receiver<String>) {
    info!("printer websocket connected");

    loop {
        tokio::select! {
            result = rx.recv() => {
                match result {
                    Ok(text) => {
                        if let Err(err) = socket.send(Message::Text(text)).await {
                            warn!(error = %err, "failed to deliver print message to websocket client");
                            break;
                        }
                    }
                    Err(broadcast::error::RecvError::Lagged(skipped)) => {
                        warn!(skipped, "websocket client lagged behind print broadcast");
                    }
                    Err(broadcast::error::RecvError::Closed) => {
                        warn!("print broadcaster closed");
                        break;
                    }
                }
            }
            message = socket.recv() => {
                match message {
                    Some(Ok(Message::Close(_))) => break,
                    Some(Ok(Message::Ping(payload))) => {
                        if let Err(err) = socket.send(Message::Pong(payload)).await {
                            warn!(error = %err, "failed to respond to websocket ping");
                            break;
                        }
                    }
                    Some(Ok(Message::Pong(_))) => {}
                    Some(Ok(Message::Text(_))) | Some(Ok(Message::Binary(_))) => {
                        // ignore messages from printer clients
                    }
                    Some(Err(err)) => {
                        warn!(error = %err, "websocket receive error");
                        break;
                    }
                    None => break,
                }
            }
        }
    }

    info!("printer websocket disconnected");
}

fn unauthorized_response() -> Response {
    (
        StatusCode::UNAUTHORIZED,
        [(
            header::WWW_AUTHENTICATE,
            r#"Basic realm=\"thermo\", charset=\"UTF-8\""#,
        )],
        "unauthorized",
    )
        .into_response()
}

fn is_authorized(headers: &HeaderMap, expected_user: &str, expected_password: &str) -> bool {
    let Some(value) = headers.get(header::AUTHORIZATION) else {
        return false;
    };

    let Ok(value) = value.to_str() else {
        return false;
    };

    let Some(encoded) = value.strip_prefix("Basic ") else {
        return false;
    };

    let Ok(decoded) = STANDARD.decode(encoded) else {
        return false;
    };

    let Ok(decoded) = String::from_utf8(decoded) else {
        return false;
    };

    let Some((user, password)) = decoded.split_once(':') else {
        return false;
    };

    user == expected_user && password == expected_password
}

fn required_env(name: &str) -> Result<String> {
    match env::var(name) {
        Ok(value) if !value.trim().is_empty() => Ok(value),
        Ok(_) => bail!("{name} is set but empty"),
        Err(_) => bail!("{name} is not set"),
    }
}

fn init_tracing() {
    let env_filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| "info,server=debug".into());

    tracing_subscriber::fmt()
        .with_env_filter(env_filter)
        .with_target(false)
        .init();
}
