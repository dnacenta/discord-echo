use serde::{Deserialize, Serialize};

// Gateway opcodes
pub const OP_DISPATCH: u8 = 0;
pub const OP_HEARTBEAT: u8 = 1;
pub const OP_IDENTIFY: u8 = 2;
pub const OP_RESUME: u8 = 6;
pub const OP_RECONNECT: u8 = 7;
pub const OP_INVALID_SESSION: u8 = 9;
pub const OP_HELLO: u8 = 10;
pub const OP_HEARTBEAT_ACK: u8 = 11;

// Gateway intents
pub const INTENT_GUILD_MESSAGES: u64 = 1 << 9;
pub const INTENT_MESSAGE_CONTENT: u64 = 1 << 15;

pub const GATEWAY_URL: &str = "wss://gateway.discord.gg/?v=10&encoding=json";

/// Outbound gateway payload.
#[derive(Debug, Serialize)]
pub struct GatewayPayload {
    pub op: u8,
    pub d: serde_json::Value,
}

/// Inbound gateway event.
#[derive(Debug, Deserialize)]
pub struct GatewayEvent {
    pub op: u8,
    pub t: Option<String>,
    pub s: Option<u64>,
    pub d: serde_json::Value,
}

/// Data from the READY event.
#[derive(Debug, Deserialize)]
pub struct ReadyData {
    pub session_id: String,
    pub resume_gateway_url: String,
}

/// Data from a MESSAGE_CREATE event.
#[derive(Debug, Deserialize)]
pub struct MessageCreateData {
    pub id: String,
    pub channel_id: String,
    pub content: String,
    pub author: MessageAuthor,
}

/// Message author info.
#[derive(Debug, Deserialize)]
pub struct MessageAuthor {
    pub id: String,
    pub username: String,
    #[serde(default)]
    pub bot: bool,
}

/// A received Discord message ready for forwarding.
pub struct IncomingMessage {
    pub channel_id: String,
    pub channel_name: Option<String>,
    pub author_id: String,
    pub author_name: String,
    pub content: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_gateway_payload_serialize() {
        let payload = GatewayPayload {
            op: OP_HEARTBEAT,
            d: serde_json::json!(42),
        };
        let json = serde_json::to_string(&payload).unwrap();
        assert!(json.contains("\"op\":1"));
        assert!(json.contains("\"d\":42"));
    }

    #[test]
    fn test_gateway_event_deserialize() {
        let json = r#"{"op":0,"t":"MESSAGE_CREATE","s":5,"d":{}}"#;
        let event: GatewayEvent = serde_json::from_str(json).unwrap();
        assert_eq!(event.op, OP_DISPATCH);
        assert_eq!(event.t.as_deref(), Some("MESSAGE_CREATE"));
        assert_eq!(event.s, Some(5));
    }

    #[test]
    fn test_message_create_deserialize() {
        let json = r#"{
            "id": "123",
            "channel_id": "456",
            "content": "hello",
            "author": {"id": "789", "username": "test", "bot": false}
        }"#;
        let msg: MessageCreateData = serde_json::from_str(json).unwrap();
        assert_eq!(msg.content, "hello");
        assert_eq!(msg.author.username, "test");
        assert!(!msg.author.bot);
    }

    #[test]
    fn test_bot_author_default() {
        let json = r#"{"id": "1", "username": "user"}"#;
        let author: MessageAuthor = serde_json::from_str(json).unwrap();
        assert!(!author.bot);
    }
}
