use crate::config::{TelemetrySnapshot, TransmitterConfig};
use crate::pipeline::PipelineCommand;
use std::sync::{Arc, Mutex};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::sync::mpsc;

/// Lightweight localhost HTTP webhook (`127.0.0.1:9944`) for Stream Deck / OpenDeck triggers:
/// - `/toggle` -> toggles master stream ON/OFF
/// - `/slate`  -> toggles Privacy Slate ON/OFF
/// - `/status` -> returns JSON telemetry snapshot
pub async fn run_webhook_server(
    bind_addr: String,
    cmd_tx: mpsc::Sender<PipelineCommand>,
    telemetry: Arc<Mutex<TelemetrySnapshot>>,
    latest_cfg: Arc<Mutex<TransmitterConfig>>,
) {
    let listener = match TcpListener::bind(&bind_addr).await {
        Ok(l) => l,
        Err(e) => {
            eprintln!("[webhook] Failed to bind {bind_addr}: {e}");
            return;
        }
    };

    loop {
        let Ok((mut socket, _)) = listener.accept().await else {
            continue;
        };
        let cmd_tx = cmd_tx.clone();
        let telemetry = telemetry.clone();
        let latest_cfg = latest_cfg.clone();

        tokio::spawn(async move {
            let mut buf = [0u8; 1024];
            let Ok(n) = socket.read(&mut buf).await else {
                return;
            };
            let req = String::from_utf8_lossy(&buf[..n]);
            let path = req
                .lines()
                .next()
                .and_then(|line| line.split_whitespace().nth(1))
                .unwrap_or("/");

            let (status, body) = match path {
                "/toggle" => {
                    let cfg = latest_cfg.lock().unwrap().clone();
                    let _ = cmd_tx.send(PipelineCommand::ToggleStream(cfg)).await;
                    let snap = telemetry.lock().unwrap().clone();
                    ("200 OK", serde_json::to_string(&snap).unwrap_or_default())
                }
                "/slate" => {
                    let _ = cmd_tx.send(PipelineCommand::ToggleSlate).await;
                    let snap = telemetry.lock().unwrap().clone();
                    ("200 OK", serde_json::to_string(&snap).unwrap_or_default())
                }
                "/status" => {
                    let snap = telemetry.lock().unwrap().clone();
                    ("200 OK", serde_json::to_string(&snap).unwrap_or_default())
                }
                _ => ("404 Not Found", r#"{"error":"not_found"}"#.to_string()),
            };

            let resp = format!(
                "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            let _ = socket.write_all(resp.as_bytes()).await;
        });
    }
}
