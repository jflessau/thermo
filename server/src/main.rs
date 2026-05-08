mod handler;

use std::{env, net::SocketAddr, sync::OnceLock};

#[cfg(unix)]
use tokio::signal::unix::{signal, SignalKind};

use anyhow::{Context, Result};
use axum::{
    http::{header, HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
    Router,
};
use base64::{engine::general_purpose::STANDARD, Engine as _};
use tokio::sync::broadcast;
use tracing::{error, info, warn};

static BASIC_AUTH_USER: OnceLock<String> = OnceLock::new();
static BASIC_AUTH_PASSWORD: OnceLock<String> = OnceLock::new();

#[derive(Clone)]
pub struct AppState {
    pub broadcaster: broadcast::Sender<String>,
    pub basic_auth_user: String,
    pub basic_auth_password: String,
}

#[tokio::main]
async fn main() -> Result<()> {
    dotenv::dotenv().ok();

    let env_filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| "info,server=debug".into());

    tracing_subscriber::fmt()
        .with_env_filter(env_filter)
        .with_target(false)
        .init();

    if let Err(err) = serve().await {
        error!("server exited with error: {err:#}");
    }

    info!("exiting");

    Ok(())
}

async fn serve() -> Result<()> {
    let basic_auth_user = basic_auth_user()?;
    let basic_auth_password = basic_auth_password()?;
    let bind_addr = env::var("BIND_ADDR")
        .unwrap_or_else(|_| "0.0.0.0:3000".to_string())
        .parse::<SocketAddr>()
        .context("BIND_ADDR must be a valid socket address")?;

    let (broadcaster, _) = broadcast::channel(256);

    let app = Router::new()
        .route(
            "/form",
            get(handler::form::handler).post(handler::form::submit_handler),
        )
        .route("/print", post(handler::print::handler))
        .route("/print/ws", get(handler::print_ws::handler))
        .with_state(AppState {
            broadcaster,
            basic_auth_user,
            basic_auth_password,
        });

    let listener = tokio::net::TcpListener::bind(bind_addr)
        .await
        .with_context(|| format!("failed to bind server to {bind_addr}"))?;

    info!("print relay server listening on {bind_addr}");

    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await
        .context("server exited unexpectedly")?;

    Ok(())
}

async fn shutdown_signal() {
    let ctrl_c = async {
        tokio::signal::ctrl_c()
            .await
            .expect("failed to install Ctrl+C signal handler");
    };

    #[cfg(unix)]
    let terminate = async {
        let mut sigterm =
            signal(SignalKind::terminate()).expect("failed to install SIGTERM signal handler");
        sigterm.recv().await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {
            info!("received Ctrl+C, starting graceful shutdown");
        }
        _ = terminate => {
            info!("received SIGTERM, starting graceful shutdown");
        }
    }
}

fn basic_auth_user() -> Result<String> {
    if let Some(value) = BASIC_AUTH_USER.get() {
        return Ok(value.clone());
    }

    let value =
        env::var("BASIC_AUTH_USER").context("BASIC_AUTH_USER environment variable is required")?;
    let _ = BASIC_AUTH_USER.set(value.clone());
    Ok(value)
}

fn basic_auth_password() -> Result<String> {
    if let Some(value) = BASIC_AUTH_PASSWORD.get() {
        return Ok(value.clone());
    }

    let value = env::var("BASIC_AUTH_PASSWORD")
        .context("BASIC_AUTH_PASSWORD environment variable is required")?;
    let _ = BASIC_AUTH_PASSWORD.set(value.clone());
    Ok(value)
}

pub fn unauthorized_response() -> Response {
    (
        StatusCode::UNAUTHORIZED,
        [(
            header::WWW_AUTHENTICATE,
            r#"Basic realm="thermo", charset="UTF-8""#,
        )],
        "unauthorized",
    )
        .into_response()
}

pub fn is_authorized(headers: &HeaderMap, expected_user: &str, expected_password: &str) -> bool {
    let Some(value) = headers.get(header::AUTHORIZATION) else {
        warn!("missing Authorization header");
        return false;
    };

    let Ok(value) = value.to_str() else {
        warn!("invalid Authorization header value");
        return false;
    };

    let Some(encoded) = value.strip_prefix("Basic ") else {
        warn!("unsupported Authorization scheme");
        return false;
    };

    let Ok(decoded) = STANDARD.decode(encoded) else {
        warn!("invalid base64 in Authorization header");
        return false;
    };

    let Ok(decoded) = String::from_utf8(decoded) else {
        warn!("decoded Authorization header is not valid UTF-8");
        return false;
    };

    let Some((user, password)) = decoded.split_once(':') else {
        warn!("invalid format of decoded Authorization header");
        return false;
    };

    let r#match = user == expected_user && password == expected_password;

    if !r#match {
        warn!("invalid credentials in Authorization header: {user}:******");
    }

    r#match
}
