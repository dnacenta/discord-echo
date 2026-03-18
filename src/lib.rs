pub mod client;
pub mod config;
pub mod gateway;
pub mod tool;
pub mod types;

use std::any::Any;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use pulse_system_types::plugin::{Plugin, PluginContext, PluginResult, PluginRole};
use pulse_system_types::{HealthStatus, PluginMeta, SetupPrompt};
use tokio::sync::{mpsc, Notify};

use crate::client::DiscordClient;
use crate::config::Config;
use crate::types::IncomingMessage;

/// Discord text integration for echo-system entities.
///
/// Manages the gateway WebSocket connection (reading messages)
/// and provides a REST client (posting messages).
pub struct DiscordEcho {
    config: Arc<Config>,
    client: Arc<DiscordClient>,
    shutdown: Arc<Notify>,
    gateway_handle: Option<tokio::task::JoinHandle<()>>,
    forwarder_handle: Option<tokio::task::JoinHandle<()>>,
}

impl DiscordEcho {
    /// Create a new DiscordEcho instance from config.
    pub fn new(config: Config) -> Self {
        let client = DiscordClient::new(config.bot_token.clone(), config.channels.clone());
        let config = Arc::new(config);
        Self {
            config,
            client,
            shutdown: Arc::new(Notify::new()),
            gateway_handle: None,
            forwarder_handle: None,
        }
    }

    /// Get a reference to the Discord client for tool use.
    pub fn client(&self) -> Arc<DiscordClient> {
        Arc::clone(&self.client)
    }

    /// Health check.
    fn health_check(&self) -> HealthStatus {
        if self.gateway_handle.is_some() {
            HealthStatus::Healthy
        } else {
            HealthStatus::Down("Not started".to_string())
        }
    }

    /// Setup prompts for first-time configuration.
    fn get_setup_prompts() -> Vec<SetupPrompt> {
        vec![
            SetupPrompt {
                key: "bot_token".to_string(),
                question: "Discord bot token:".to_string(),
                default: None,
                required: true,
                secret: true,
            },
            SetupPrompt {
                key: "guild_id".to_string(),
                question: "Discord server (guild) ID:".to_string(),
                default: None,
                required: true,
                secret: false,
            },
        ]
    }
}

/// Factory function — creates a fully initialized discord-echo plugin.
pub async fn create(
    config: &serde_json::Value,
    _ctx: &PluginContext,
) -> Result<Box<dyn Plugin>, Box<dyn std::error::Error + Send + Sync>> {
    let cfg: Config = serde_json::from_value(config.clone())?;
    Ok(Box::new(DiscordEcho::new(cfg)))
}

impl Plugin for DiscordEcho {
    fn meta(&self) -> PluginMeta {
        PluginMeta {
            name: "discord-echo".into(),
            version: env!("CARGO_PKG_VERSION").into(),
            description: "Discord text integration".into(),
        }
    }

    fn role(&self) -> PluginRole {
        PluginRole::Interface
    }

    fn start(&mut self) -> PluginResult<'_> {
        Box::pin(async move {
            if self.gateway_handle.is_some() {
                return Err("Already running".into());
            }

            let (message_tx, message_rx) = mpsc::channel::<IncomingMessage>(64);

            let gw_config = Arc::clone(&self.config);
            let gw_shutdown = Arc::clone(&self.shutdown);
            self.gateway_handle = Some(tokio::spawn(async move {
                gateway::run_gateway(gw_config, message_tx, gw_shutdown).await;
            }));

            let fwd_client = Arc::clone(&self.client);
            let fwd_config = Arc::clone(&self.config);
            let fwd_shutdown = Arc::clone(&self.shutdown);
            self.forwarder_handle = Some(tokio::spawn(async move {
                message_forwarder(message_rx, fwd_client, fwd_config, fwd_shutdown).await;
            }));

            tracing::info!("Discord text integration started");
            Ok(())
        })
    }

    fn stop(&mut self) -> PluginResult<'_> {
        Box::pin(async move {
            self.shutdown.notify_waiters();

            if let Some(h) = self.gateway_handle.take() {
                let _ = tokio::time::timeout(std::time::Duration::from_secs(5), h).await;
            }
            if let Some(h) = self.forwarder_handle.take() {
                let _ = tokio::time::timeout(std::time::Duration::from_secs(5), h).await;
            }

            self.shutdown = Arc::new(Notify::new());

            tracing::info!("Discord text integration stopped");
            Ok(())
        })
    }

    fn health(&self) -> Pin<Box<dyn Future<Output = HealthStatus> + Send + '_>> {
        Box::pin(async move { self.health_check() })
    }

    fn setup_prompts(&self) -> Vec<SetupPrompt> {
        Self::get_setup_prompts()
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// Silent response markers. If the entity's response starts with any of these,
/// the forwarder will not post it to Discord. This lets the entity decide
/// on a per-message basis whether to respond or stay quiet.
const SILENT_MARKERS: &[&str] = &["[SILENT]", "[NO_RESPONSE]", "No response requested"];

/// Check if a response indicates the entity chose not to respond.
fn is_silent(response: &str) -> bool {
    let trimmed = response.trim();
    SILENT_MARKERS
        .iter()
        .any(|marker| trimmed.starts_with(marker))
}

/// Receives messages from the gateway, forwards to the entity's chat endpoint,
/// and posts responses back to Discord.
async fn message_forwarder(
    mut rx: mpsc::Receiver<IncomingMessage>,
    client: Arc<DiscordClient>,
    config: Arc<Config>,
    shutdown: Arc<Notify>,
) {
    let http = reqwest::Client::new();

    loop {
        tokio::select! {
            msg = rx.recv() => {
                let msg = match msg {
                    Some(m) => m,
                    None => return, // gateway dropped
                };

                let channel_label = msg.channel_name.as_deref().unwrap_or("discord");
                tracing::info!(
                    "Message from {} in #{}: {}",
                    msg.author_name,
                    channel_label,
                    if msg.content.len() > 80 { &msg.content[..80] } else { &msg.content }
                );

                // Forward to chat endpoint
                let mut req = http
                    .post(&config.chat_endpoint)
                    .json(&serde_json::json!({
                        "message": msg.content,
                        "channel": config.chat_channel_name,
                        "sender": msg.author_name,
                    }));

                if let Some(ref secret) = config.chat_secret {
                    req = req.header("X-Echo-Secret", secret);
                }

                match req.send().await {
                    Ok(resp) if resp.status().is_success() => {
                        if let Ok(data) = resp.json::<serde_json::Value>().await {
                            let response_text = data["response"]
                                .as_str()
                                .or_else(|| data["text"].as_str())
                                .unwrap_or("");

                            if !response_text.is_empty() && !is_silent(response_text) {
                                if let Err(e) = client.send_message_by_id(&msg.channel_id, response_text).await {
                                    tracing::error!("Failed to reply in Discord: {e}");
                                }
                            } else if is_silent(response_text) {
                                tracing::debug!(
                                    "Silent response for message from {} in #{}",
                                    msg.author_name,
                                    channel_label,
                                );
                            }
                        }
                    }
                    Ok(resp) => {
                        tracing::warn!(
                            "Chat endpoint returned {}",
                            resp.status()
                        );
                    }
                    Err(e) => {
                        tracing::error!("Failed to forward to chat endpoint: {e}");
                    }
                }
            }
            _ = shutdown.notified() => return,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[tokio::test]
    async fn test_health_down_before_start() {
        let config = Config {
            bot_token: "test".to_string(),
            guild_id: "123".to_string(),
            listen_channels: vec![],
            allowed_user_ids: vec![],
            chat_endpoint: "http://localhost:3100/chat".to_string(),
            chat_secret: None,
            chat_channel_name: "discord".to_string(),
            channels: HashMap::new(),
        };
        let echo = DiscordEcho::new(config);
        let health = Plugin::health(&echo).await;
        assert!(matches!(health, HealthStatus::Down(_)));
    }

    #[test]
    fn test_setup_prompts_not_empty() {
        let config = Config {
            bot_token: "test".to_string(),
            guild_id: "123".to_string(),
            listen_channels: vec![],
            allowed_user_ids: vec![],
            chat_endpoint: "http://localhost:3100/chat".to_string(),
            chat_secret: None,
            chat_channel_name: "discord".to_string(),
            channels: HashMap::new(),
        };
        let echo = DiscordEcho::new(config);
        let prompts = Plugin::setup_prompts(&echo);
        assert!(!prompts.is_empty());
        assert!(prompts.iter().any(|p| p.key == "bot_token"));
        assert!(prompts.iter().any(|p| p.key == "guild_id"));
    }

    #[test]
    fn test_is_silent() {
        assert!(is_silent("[SILENT]"));
        assert!(is_silent("[SILENT] I have nothing to add"));
        assert!(is_silent("[NO_RESPONSE]"));
        assert!(is_silent("No response requested"));
        assert!(is_silent("No response requested."));
        assert!(is_silent("  [SILENT]  ")); // trimmed
        assert!(!is_silent("Hello, how are you?"));
        assert!(!is_silent(""));
        assert!(!is_silent("I think [SILENT] is interesting")); // not at start
    }

    #[test]
    fn test_client_reference() {
        let config = Config {
            bot_token: "test".to_string(),
            guild_id: "123".to_string(),
            listen_channels: vec![],
            allowed_user_ids: vec![],
            chat_endpoint: "http://localhost:3100/chat".to_string(),
            chat_secret: None,
            chat_channel_name: "discord".to_string(),
            channels: HashMap::from([("test".to_string(), "456".to_string())]),
        };
        let echo = DiscordEcho::new(config);
        let client = echo.client();
        assert_eq!(client.resolve_channel("test"), Some("456"));
    }
}
