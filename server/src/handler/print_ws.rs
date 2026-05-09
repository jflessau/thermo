use std::time::{Duration, Instant};

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

const MAX_CLIENT_PING_AGE: Duration = Duration::from_secs(10);

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

    ws.on_upgrade(move |socket| handle_ws_connection(socket, state))
}

async fn handle_ws_connection(mut socket: WebSocket, state: AppState) {
    let mut rx = state.broadcaster.subscribe();
    {
        let mut connected_printers = state.connected_printers.lock().await;
        *connected_printers += 1;
    }

    info!("printer websocket connected");
    flush_pending_jobs(&state).await;

    let mut last_client_ping_at = Instant::now();
    let mut liveness_interval = tokio::time::interval(Duration::from_secs(1));

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
            _ = liveness_interval.tick() => {
                let elapsed = last_client_ping_at.elapsed();
                if elapsed > MAX_CLIENT_PING_AGE {
                    warn!(elapsed_secs = elapsed.as_secs_f32(), "printer websocket timed out waiting for client ping");
                    break;
                }
            }
            message = socket.recv() => {
                match message {
                    Some(Ok(Message::Close(_))) => break,
                    Some(Ok(Message::Ping(payload))) => {
                        last_client_ping_at = Instant::now();
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

    {
        let mut connected_printers = state.connected_printers.lock().await;
        *connected_printers = connected_printers.saturating_sub(1);
    }

    info!("printer websocket disconnected");
}

async fn flush_pending_jobs(state: &AppState) {
    let mut pending_jobs = state.pending_jobs.lock().await;

    if pending_jobs.is_empty() {
        return;
    }

    info!(
        count = pending_jobs.len(),
        "flushing queued print jobs to connected printer"
    );

    while let Some(job) = pending_jobs.pop_front() {
        if let Err(err) = state.broadcaster.send(job) {
            warn!(error = %err, "failed to flush queued print job");
            break;
        }
    }
}
