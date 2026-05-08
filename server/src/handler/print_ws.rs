use axum::{
    extract::{
        ws::{Message, WebSocket},
        State, WebSocketUpgrade,
    },
    http::HeaderMap,
    response::Response,
};
use tokio::sync::broadcast;
use tracing::{info, warn};

use crate::{is_authorized, unauthorized_response, AppState};

pub async fn handler(
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
