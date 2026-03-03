use std::sync::Arc;

use crate::client::DiscordClient;

/// Discord posting tool. Provides the logic for posting messages
/// to Discord channels. Does NOT implement echo-system's Tool trait
/// directly (different crate) — the plugin adapter wraps this.
pub struct DiscordPostTool {
    client: Arc<DiscordClient>,
}

impl DiscordPostTool {
    pub fn new(client: Arc<DiscordClient>) -> Self {
        Self { client }
    }

    pub fn name() -> &'static str {
        "discord_post"
    }

    pub fn description() -> &'static str {
        "Post a message to a Discord channel. Use channel names from your config (e.g. 'constellation', 'self-evolution'), not raw IDs."
    }

    pub fn input_schema() -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "channel": {
                    "type": "string",
                    "description": "Channel name (e.g. 'constellation', 'notes')"
                },
                "message": {
                    "type": "string",
                    "description": "Message content to post"
                }
            },
            "required": ["channel", "message"]
        })
    }

    pub async fn execute(&self, input: serde_json::Value) -> Result<String, String> {
        let channel = input["channel"]
            .as_str()
            .ok_or_else(|| "Missing 'channel' parameter".to_string())?;
        let message = input["message"]
            .as_str()
            .ok_or_else(|| "Missing 'message' parameter".to_string())?;

        if message.is_empty() {
            return Err("Message cannot be empty".to_string());
        }

        self.client
            .send_message(channel, message)
            .await
            .map(|_| format!("Message posted to #{}", channel))
            .map_err(|e| format!("Failed to post to #{}: {}", channel, e))
    }

    /// List available channel names for the tool description.
    pub fn available_channels(&self) -> Vec<&str> {
        self.client.channel_names()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tool_metadata() {
        assert_eq!(DiscordPostTool::name(), "discord_post");
        assert!(!DiscordPostTool::description().is_empty());

        let schema = DiscordPostTool::input_schema();
        assert_eq!(schema["type"], "object");
        let required = schema["required"].as_array().unwrap();
        assert!(required.contains(&serde_json::json!("channel")));
        assert!(required.contains(&serde_json::json!("message")));
    }
}
