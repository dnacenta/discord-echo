use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use tokio::sync::{mpsc, Notify};
use tokio::time;
use tokio_tungstenite::connect_async;
use tokio_tungstenite::tungstenite::Message as WsMessage;

use crate::config::Config;
use crate::types::*;

const MAX_RECONNECT_BACKOFF_SECS: u64 = 30;
const IDENTIFY_DELAY: Duration = Duration::from_secs(5);

/// Run the gateway connection loop. Sends filtered messages to the mpsc channel.
/// Returns when shutdown is notified.
pub async fn run_gateway(
    config: Arc<Config>,
    message_tx: mpsc::Sender<IncomingMessage>,
    shutdown: Arc<Notify>,
) {
    let mut session_id: Option<String> = None;
    let mut resume_url: Option<String> = None;
    let mut self_bot_id: Option<String> = None;
    let sequence = Arc::new(AtomicU64::new(0));
    let mut backoff_secs = 1u64;

    loop {
        let url = resume_url.as_deref().unwrap_or(GATEWAY_URL).to_string();

        tracing::info!("Connecting to Discord gateway: {}", url);

        let ws = match connect_async(&url).await {
            Ok((stream, _)) => {
                backoff_secs = 1;
                stream
            }
            Err(e) => {
                tracing::error!("Gateway connect failed: {e}");
                let delay = Duration::from_secs(backoff_secs);
                tokio::select! {
                    _ = time::sleep(delay) => {}
                    _ = shutdown.notified() => return,
                }
                backoff_secs = (backoff_secs * 2).min(MAX_RECONNECT_BACKOFF_SECS);
                continue;
            }
        };

        let (write, mut read) = ws.split();

        // Wait for Hello (op 10)
        let hello_event = match read.next().await {
            Some(Ok(WsMessage::Text(text))) => match serde_json::from_str::<GatewayEvent>(&text) {
                Ok(event) if event.op == OP_HELLO => event,
                _ => {
                    tracing::error!("Expected Hello, got: {}", text);
                    continue;
                }
            },
            _ => {
                tracing::error!("Expected Hello message from gateway");
                continue;
            }
        };

        let heartbeat_interval = hello_event.d["heartbeat_interval"]
            .as_u64()
            .unwrap_or(41250);
        tracing::info!("Heartbeat interval: {}ms", heartbeat_interval);

        // Channel for sending messages to the write half
        let (write_tx, mut write_rx) = mpsc::channel::<WsMessage>(32);

        // Heartbeat task
        let hb_tx = write_tx.clone();
        let hb_seq = Arc::clone(&sequence);
        let hb_shutdown = Arc::clone(&shutdown);
        let heartbeat_handle = tokio::spawn(async move {
            let interval = Duration::from_millis(heartbeat_interval);
            loop {
                tokio::select! {
                    _ = time::sleep(interval) => {
                        let s = hb_seq.load(Ordering::Relaxed);
                        let payload = serde_json::json!({"op": OP_HEARTBEAT, "d": s});
                        if hb_tx.send(WsMessage::Text(payload.to_string().into())).await.is_err() {
                            return;
                        }
                    }
                    _ = hb_shutdown.notified() => return,
                }
            }
        });

        // Write task — drains write_rx and sends to WebSocket
        let wr_shutdown = Arc::clone(&shutdown);
        let write_handle = tokio::spawn(async move {
            let mut write = write;
            loop {
                tokio::select! {
                    msg = write_rx.recv() => {
                        match msg {
                            Some(m) => { let _ = write.send(m).await; }
                            None => break,
                        }
                    }
                    _ = wr_shutdown.notified() => {
                        let _ = write.close().await;
                        return;
                    }
                }
            }
        });

        // Send Identify or Resume
        let auth_payload = if let Some(ref sid) = session_id {
            let s = sequence.load(Ordering::Relaxed);
            tracing::info!("Resuming session {}", sid);
            serde_json::json!({
                "op": OP_RESUME,
                "d": {
                    "token": config.bot_token,
                    "session_id": sid,
                    "seq": s
                }
            })
        } else {
            tracing::info!("Identifying new session");
            serde_json::json!({
                "op": OP_IDENTIFY,
                "d": {
                    "token": config.bot_token,
                    "intents": INTENT_GUILD_MESSAGES | INTENT_MESSAGE_CONTENT,
                    "properties": {
                        "os": "linux",
                        "browser": "discord-echo",
                        "device": "discord-echo"
                    }
                }
            })
        };

        if write_tx
            .send(WsMessage::Text(auth_payload.to_string().into()))
            .await
            .is_err()
        {
            tracing::error!("Failed to send identify/resume");
            heartbeat_handle.abort();
            continue;
        }

        // Message read loop
        let mut should_backoff = true;
        loop {
            tokio::select! {
                frame = read.next() => {
                    match frame {
                        Some(Ok(WsMessage::Text(text))) => {
                            let event = match serde_json::from_str::<GatewayEvent>(&text) {
                                Ok(e) => e,
                                Err(e) => {
                                    tracing::warn!("Failed to parse gateway event: {e}");
                                    continue;
                                }
                            };

                            if let Some(s) = event.s {
                                sequence.store(s, Ordering::Relaxed);
                            }

                            match event.op {
                                OP_DISPATCH => {
                                    if let Some(ref t) = event.t {
                                        match t.as_str() {
                                            "READY" => {
                                                if let Ok(ready) = serde_json::from_value::<ReadyData>(event.d) {
                                                    session_id = Some(ready.session_id);
                                                    resume_url = Some(ready.resume_gateway_url);
                                                    self_bot_id = Some(ready.user.id.clone());
                                                    tracing::info!("Gateway ready as {}", ready.user.username);
                                                }
                                            }
                                            "RESUMED" => {
                                                tracing::info!("Session resumed");
                                            }
                                            "MESSAGE_CREATE" => {
                                                if let Ok(msg) = serde_json::from_value::<MessageCreateData>(event.d) {
                                                    // Skip own messages (don't respond to self)
                                                    if let Some(ref own_id) = self_bot_id {
                                                        if msg.author.id == *own_id {
                                                            continue;
                                                        }
                                                    }
                                                    if !config.is_listen_channel(&msg.channel_id) {
                                                        continue;
                                                    }
                                                    if !config.is_allowed_user(&msg.author.id) {
                                                        continue;
                                                    }

                                                    let channel_name = config.channel_name(&msg.channel_id).map(|s| s.to_string());
                                                    let incoming = IncomingMessage {
                                                        channel_id: msg.channel_id,
                                                        channel_name,
                                                        author_id: msg.author.id,
                                                        author_name: msg.author.username,
                                                        content: msg.content,
                                                    };

                                                    if message_tx.send(incoming).await.is_err() {
                                                        tracing::error!("Message receiver dropped");
                                                        heartbeat_handle.abort();
                                                        drop(write_tx);
                                                        let _ = write_handle.await;
                                                        return;
                                                    }
                                                }
                                            }
                                            _ => {}
                                        }
                                    }
                                }
                                OP_HEARTBEAT_ACK => {}
                                OP_HEARTBEAT => {
                                    let s = sequence.load(Ordering::Relaxed);
                                    let payload = serde_json::json!({"op": OP_HEARTBEAT, "d": s});
                                    let _ = write_tx.send(WsMessage::Text(payload.to_string().into())).await;
                                }
                                OP_RECONNECT => {
                                    tracing::info!("Server requested reconnect");
                                    should_backoff = false;
                                    break;
                                }
                                OP_INVALID_SESSION => {
                                    let resumable = event.d.as_bool().unwrap_or(false);
                                    if !resumable {
                                        tracing::warn!("Invalid session, re-identifying after delay");
                                        session_id = None;
                                        resume_url = None;
                                        sequence.store(0, Ordering::Relaxed);
                                    }
                                    tokio::time::sleep(IDENTIFY_DELAY).await;
                                    should_backoff = false;
                                    break;
                                }
                                _ => {}
                            }
                        }
                        Some(Ok(WsMessage::Close(_))) => {
                            tracing::info!("Gateway connection closed");
                            break;
                        }
                        Some(Err(e)) => {
                            tracing::error!("WebSocket error: {e}");
                            break;
                        }
                        None => {
                            tracing::info!("WebSocket stream ended");
                            break;
                        }
                        _ => {} // ping/pong
                    }
                }
                _ = shutdown.notified() => {
                    tracing::info!("Shutdown signal received");
                    heartbeat_handle.abort();
                    drop(write_tx);
                    let _ = write_handle.await;
                    return;
                }
            }
        }

        // Cleanup
        heartbeat_handle.abort();
        drop(write_tx);
        let _ = write_handle.await;

        if should_backoff {
            let delay = Duration::from_secs(backoff_secs);
            tokio::select! {
                _ = time::sleep(delay) => {}
                _ = shutdown.notified() => return,
            }
            backoff_secs = (backoff_secs * 2).min(MAX_RECONNECT_BACKOFF_SECS);
        }
        // Continue outer loop for reconnect
    }
}
