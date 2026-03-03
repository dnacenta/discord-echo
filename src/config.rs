use serde::Deserialize;
use std::collections::HashMap;

#[derive(Debug, Deserialize, Clone)]
pub struct Config {
    pub bot_token: String,
    pub guild_id: String,
    #[serde(default)]
    pub listen_channels: Vec<String>,
    #[serde(default)]
    pub allowed_user_ids: Vec<String>,
    #[serde(default = "default_chat_endpoint")]
    pub chat_endpoint: String,
    #[serde(default)]
    pub chat_secret: Option<String>,
    #[serde(default = "default_chat_channel")]
    pub chat_channel_name: String,
    #[serde(default)]
    pub channels: HashMap<String, String>,
}

fn default_chat_endpoint() -> String {
    "http://127.0.0.1:3100/chat".to_string()
}

fn default_chat_channel() -> String {
    "discord".to_string()
}

impl Config {
    /// Resolve a channel name to its Discord channel ID.
    pub fn channel_id(&self, name: &str) -> Option<&str> {
        self.channels.get(name).map(|s| s.as_str())
    }

    /// Reverse lookup: Discord channel ID to channel name.
    pub fn channel_name(&self, id: &str) -> Option<&str> {
        self.channels
            .iter()
            .find(|(_, v)| v.as_str() == id)
            .map(|(k, _)| k.as_str())
    }

    /// Check if a channel ID is in the listen list.
    pub fn is_listen_channel(&self, channel_id: &str) -> bool {
        if self.listen_channels.is_empty() {
            return true;
        }
        let name = self.channel_name(channel_id);
        match name {
            Some(n) => self.listen_channels.iter().any(|l| l == n),
            None => false,
        }
    }

    /// Check if a user ID is allowed (empty = allow all).
    pub fn is_allowed_user(&self, user_id: &str) -> bool {
        if self.allowed_user_ids.is_empty() {
            return true;
        }
        self.allowed_user_ids.iter().any(|id| id == user_id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_config() -> Config {
        Config {
            bot_token: "test-token".to_string(),
            guild_id: "123".to_string(),
            listen_channels: vec!["constellation".to_string()],
            allowed_user_ids: vec![],
            chat_endpoint: default_chat_endpoint(),
            chat_secret: None,
            chat_channel_name: "discord".to_string(),
            channels: HashMap::from([
                ("constellation".to_string(), "111".to_string()),
                ("notes".to_string(), "222".to_string()),
            ]),
        }
    }

    #[test]
    fn test_channel_id_lookup() {
        let config = test_config();
        assert_eq!(config.channel_id("constellation"), Some("111"));
        assert_eq!(config.channel_id("unknown"), None);
    }

    #[test]
    fn test_channel_name_lookup() {
        let config = test_config();
        assert_eq!(config.channel_name("111"), Some("constellation"));
        assert_eq!(config.channel_name("999"), None);
    }

    #[test]
    fn test_is_listen_channel() {
        let config = test_config();
        assert!(config.is_listen_channel("111")); // constellation
        assert!(!config.is_listen_channel("222")); // notes not in listen list
        assert!(!config.is_listen_channel("999")); // unknown
    }

    #[test]
    fn test_empty_listen_allows_all() {
        let mut config = test_config();
        config.listen_channels.clear();
        assert!(config.is_listen_channel("111"));
        assert!(config.is_listen_channel("222"));
    }

    #[test]
    fn test_allowed_users_empty_allows_all() {
        let config = test_config();
        assert!(config.is_allowed_user("anyone"));
    }

    #[test]
    fn test_allowed_users_filter() {
        let mut config = test_config();
        config.allowed_user_ids = vec!["user1".to_string()];
        assert!(config.is_allowed_user("user1"));
        assert!(!config.is_allowed_user("user2"));
    }
}
