use std::collections::HashMap;
use std::sync::Arc;

const DISCORD_API_BASE: &str = "https://discord.com/api/v10";
const DISCORD_MAX_LEN: usize = 2000;

/// HTTP client for the Discord REST API.
#[derive(Clone)]
pub struct DiscordClient {
    http: reqwest::Client,
    bot_token: String,
    channel_map: HashMap<String, String>,
}

impl DiscordClient {
    pub fn new(bot_token: String, channel_map: HashMap<String, String>) -> Arc<Self> {
        Arc::new(Self {
            http: reqwest::Client::new(),
            bot_token,
            channel_map,
        })
    }

    /// Post a message to a channel by name.
    pub async fn send_message(&self, channel_name: &str, content: &str) -> Result<(), ClientError> {
        let channel_id = self
            .channel_map
            .get(channel_name)
            .ok_or_else(|| ClientError::UnknownChannel(channel_name.to_string()))?;
        self.send_message_by_id(channel_id, content).await
    }

    /// Post a message to a channel by Discord ID. Splits at 2000 chars.
    pub async fn send_message_by_id(
        &self,
        channel_id: &str,
        content: &str,
    ) -> Result<(), ClientError> {
        let chunks = split_message(content);
        for chunk in chunks {
            let url = format!("{}/channels/{}/messages", DISCORD_API_BASE, channel_id);
            let resp = self
                .http
                .post(&url)
                .header("Authorization", format!("Bot {}", self.bot_token))
                .json(&serde_json::json!({ "content": chunk }))
                .send()
                .await
                .map_err(|e| ClientError::Http(e.to_string()))?;

            if !resp.status().is_success() {
                let status = resp.status().as_u16();
                let body = resp.text().await.unwrap_or_default();
                return Err(ClientError::Api { status, body });
            }
        }
        Ok(())
    }

    /// Resolve a channel name to its ID.
    pub fn resolve_channel(&self, name: &str) -> Option<&str> {
        self.channel_map.get(name).map(|s| s.as_str())
    }

    /// List available channel names.
    pub fn channel_names(&self) -> Vec<&str> {
        self.channel_map.keys().map(|k| k.as_str()).collect()
    }
}

/// Split text at Discord's 2000-char limit, preferring newline boundaries.
pub fn split_message(text: &str) -> Vec<&str> {
    if text.len() <= DISCORD_MAX_LEN {
        return vec![text];
    }

    let mut chunks = Vec::new();
    let mut remaining = text;

    while !remaining.is_empty() {
        if remaining.len() <= DISCORD_MAX_LEN {
            chunks.push(remaining);
            break;
        }

        // Find the best split point: last newline before the limit
        let search_range = &remaining[..DISCORD_MAX_LEN];
        let split_at = search_range
            .rfind('\n')
            .map(|pos| pos + 1) // include the newline in the first chunk
            .unwrap_or(DISCORD_MAX_LEN); // hard split if no newline

        chunks.push(&remaining[..split_at]);
        remaining = &remaining[split_at..];
    }

    chunks
}

#[derive(Debug)]
pub enum ClientError {
    UnknownChannel(String),
    Http(String),
    Api { status: u16, body: String },
}

impl std::fmt::Display for ClientError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ClientError::UnknownChannel(name) => write!(f, "Unknown channel: {}", name),
            ClientError::Http(msg) => write!(f, "HTTP error: {}", msg),
            ClientError::Api { status, body } => {
                write!(f, "Discord API error {}: {}", status, body)
            }
        }
    }
}

impl std::error::Error for ClientError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_split_short_message() {
        let chunks = split_message("hello");
        assert_eq!(chunks, vec!["hello"]);
    }

    #[test]
    fn test_split_at_newline() {
        let mut msg = String::new();
        // Build a message with lines that push past 2000 chars
        for i in 0..30 {
            msg.push_str(&format!("Line {} with some content here\n", i));
        }
        // Ensure it's over 2000
        while msg.len() <= DISCORD_MAX_LEN {
            msg.push_str("padding line\n");
        }
        let chunks = split_message(&msg);
        assert!(chunks.len() >= 2);
        // First chunk should end at a newline
        assert!(chunks[0].ends_with('\n'));
        assert!(chunks[0].len() <= DISCORD_MAX_LEN);
    }

    #[test]
    fn test_split_no_newlines() {
        let msg = "x".repeat(DISCORD_MAX_LEN + 500);
        let chunks = split_message(&msg);
        assert_eq!(chunks.len(), 2);
        assert_eq!(chunks[0].len(), DISCORD_MAX_LEN);
        assert_eq!(chunks[1].len(), 500);
    }

    #[test]
    fn test_split_exact_boundary() {
        let msg = "x".repeat(DISCORD_MAX_LEN);
        let chunks = split_message(&msg);
        assert_eq!(chunks.len(), 1);
    }

    #[test]
    fn test_resolve_channel() {
        let client = DiscordClient::new(
            "token".to_string(),
            HashMap::from([("test".to_string(), "123".to_string())]),
        );
        assert_eq!(client.resolve_channel("test"), Some("123"));
        assert_eq!(client.resolve_channel("nope"), None);
    }
}
