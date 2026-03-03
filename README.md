# discord-echo

[![License: AGPL-3.0](https://img.shields.io/github/license/dnacenta/discord-echo)](LICENSE)
[![Rust](https://img.shields.io/badge/rust-1.80%2B-orange)](https://rustup.rs/)

Discord text integration for [echo-system](https://github.com/dnacenta/echo-system) entities.

Gives an entity the ability to read and write messages in Discord text channels. Connects to the Discord gateway via WebSocket, listens for messages in configured channels, forwards them to the entity's chat handler, and posts responses back. Also provides a `discord_post` tool for entity-initiated messaging.

## Features

- **Gateway**: Discord gateway v10 WebSocket with heartbeat, identify, resume, and reconnect with exponential backoff
- **REST client**: Post messages to any configured channel, automatic splitting at Discord's 2000-character limit
- **Tool**: `discord_post` tool for proactive messaging from scheduled tasks, intents, or conversations
- **Filtering**: Channel-based and user-based message filtering
- **Config**: Channel name-to-ID mapping so the entity uses friendly names, not raw IDs

## Usage

discord-echo is used as a dependency of echo-system. Enable it with the `discord-text` feature:

```bash
cargo build --features discord-text
```

Configure in `echo-system.toml`:

```toml
[plugins.discord-text-echo]
bot_token = "your-bot-token"
guild_id = "your-guild-id"
listen_channels = ["constellation"]
chat_endpoint = "http://127.0.0.1:3100/chat"

[plugins.discord-text-echo.channels]
constellation = "channel-id"
```

## License

AGPL-3.0 — see [LICENSE](LICENSE).
