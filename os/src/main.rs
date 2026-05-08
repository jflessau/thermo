mod printer;

use std::{
    env,
    sync::{Arc, Mutex},
};

use anyhow::{Context, Result, anyhow, bail};
use futures_util::StreamExt;
use printer::Printer;
use tokio_tungstenite::{connect_async, tungstenite::client::IntoClientRequest};
use tracing::{debug, error, info, warn};
use url::Url;

#[tokio::main]
async fn main() -> Result<()> {
    println!("starting thermo printer os");

    dotenv::dotenv().ok();
    init_tracing();
    debug!("env vars loaded");

    info!("starting thermo printer os");

    let printer = Arc::new(Mutex::new(
        Printer::connect().context("failed to connect to printer")?,
    ));

    let server_url = required_env("SERVER_URL")?;
    let basic_auth_user = required_env("BASIC_AUTH_USER")?;
    let basic_auth_password = required_env("BASIC_AUTH_PASSWORD")?;

    tokio::select! {
        res = run_printer_client(printer, &server_url, &basic_auth_user, &basic_auth_password) => {
            if let Err(err) = res {
                error!(error = %err, "printer relay client exited with error");
            }
        }
        _ = tokio::signal::ctrl_c() => {
            info!("received shutdown signal, exiting");
        }
    }

    Ok(())
}

async fn run_printer_client(
    printer: Arc<Mutex<Printer>>,
    server_url: &str,
    basic_auth_user: &str,
    basic_auth_password: &str,
) -> Result<()> {
    let ws_url = websocket_url(server_url)?;
    info!(%ws_url, "connecting printer websocket client");

    loop {
        match connect_and_consume(&printer, &ws_url, basic_auth_user, basic_auth_password).await {
            Ok(()) => {
                warn!("printer websocket connection closed, reconnecting soon");
            }
            Err(err) => {
                warn!(error = %err, "printer websocket client failed, reconnecting soon");
            }
        }

        tokio::time::sleep(std::time::Duration::from_secs(3)).await;
    }
}

async fn connect_and_consume(
    printer: &Arc<Mutex<Printer>>,
    ws_url: &str,
    basic_auth_user: &str,
    basic_auth_password: &str,
) -> Result<()> {
    let auth_value = basic_auth_header(basic_auth_user, basic_auth_password);
    let mut request = ws_url.into_client_request()?;
    request.headers_mut().insert(
        "Authorization",
        auth_value.parse().context("invalid basic auth header")?,
    );

    let (ws_stream, response) = connect_async(request)
        .await
        .with_context(|| format!("failed to connect to printer relay at {ws_url}"))?;

    info!(status = %response.status(), "connected to printer relay websocket");

    let (_, mut read) = ws_stream.split();

    while let Some(message) = read.next().await {
        match message.context("failed to read websocket message")? {
            tokio_tungstenite::tungstenite::Message::Text(text) => {
                info!(len = text.len(), "received print job");
                print_text(printer, &text)?;
            }
            tokio_tungstenite::tungstenite::Message::Binary(_) => {
                warn!("ignoring unexpected binary websocket message");
            }
            tokio_tungstenite::tungstenite::Message::Ping(_) => {}
            tokio_tungstenite::tungstenite::Message::Pong(_) => {}
            tokio_tungstenite::tungstenite::Message::Frame(_) => {}
            tokio_tungstenite::tungstenite::Message::Close(frame) => {
                info!(?frame, "printer websocket closed by server");
                return Ok(());
            }
        }
    }

    Ok(())
}

fn print_text(printer: &Arc<Mutex<Printer>>, text: &str) -> Result<()> {
    let mut printer = printer
        .lock()
        .map_err(|_| anyhow!("printer mutex was poisoned"))?;

    printer.feed(1)?;
    printer.write_text(text)?;
    printer.feed(2)?;
    Ok(())
}

fn websocket_url(server_url: &str) -> Result<String> {
    let mut url =
        Url::parse(server_url).with_context(|| format!("invalid SERVER_URL: {server_url}"))?;

    match url.scheme() {
        "http" => url
            .set_scheme("ws")
            .map_err(|_| anyhow!("failed to convert http URL to ws"))?,
        "https" => url
            .set_scheme("wss")
            .map_err(|_| anyhow!("failed to convert https URL to wss"))?,
        "ws" | "wss" => {}
        scheme => bail!("unsupported SERVER_URL scheme: {scheme}"),
    }

    url.set_path("/print/ws");
    url.set_query(None);
    url.set_fragment(None);

    Ok(url.into())
}

fn basic_auth_header(user: &str, password: &str) -> String {
    use base64::{Engine as _, engine::general_purpose::STANDARD};

    let credentials = format!("{user}:{password}");
    format!("Basic {}", STANDARD.encode(credentials))
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
        .unwrap_or_else(|_| "info,os=debug".into());

    tracing_subscriber::fmt()
        .with_env_filter(env_filter)
        .with_target(false)
        .init();
}
