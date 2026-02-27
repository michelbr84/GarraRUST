# Channel Integrations

GarraIA supports multiple messaging channels out of the box.

## Telegram

### Setup

1. Create a bot via [@BotFather](https://t.me/BotFather)
2. Get your bot token
3. Configure in `config.yml`:

```yaml
channels:
  telegram:
    enabled: true
    bot_token: "YOUR_BOT_TOKEN"
```

### Features

- Streaming responses
- MarkdownV2 formatting
- Slash commands (/help, /clear, /model, etc.)
- Typing indicators
- Group chat support
- User allowlisting

### Commands

All built-in commands work in Telegram:
- `/help` - Show available commands
- `/clear` - Clear conversation history
- `/model [name]` - Switch LLM model
- `/pair` - Generate pairing code
- `/voz` or `/voice` - Toggle voice responses

## Discord

### Setup

1. Create an application at [Discord Developer Portal](https://discord.com/developers/applications)
2. Create a bot and get the token
3. Enable required intents (Message Content, Guilds)
4. Invite bot with appropriate permissions

```yaml
channels:
  discord:
    enabled: true
    bot_token: "YOUR_DISCORD_TOKEN"
    application_id: "YOUR_APP_ID"
```

### Features

- Slash commands
- Event-based message handling
- Session management

### Slash Commands

GarraIA registers slash commands automatically:
- `/help` - Show help
- `/clear` - Clear history
- `/model` - Switch model

## Slack

### Setup

1. Create an app at [Slack API](https://api.slack.com/apps)
2. Enable Socket Mode
3. Get bot and app tokens
4. Add required scopes: `chat:write`, `commands`, `channels:history`

```yaml
channels:
  slack:
    enabled: true
    bot_token: "xoxb-..."
    app_token: "xapp-..."
```

### Features

- Socket Mode (no public endpoints needed)
- Streaming responses
- Allowlist management

## WhatsApp

### Setup

1. Set up Meta Cloud API
2. Get phone number ID and access token
3. Configure webhook verification

```yaml
channels:
  whatsapp:
    enabled: true
    phone_number_id: "123456789"
    access_token: "YOUR_ACCESS_TOKEN"
    verify_token: "YOUR_VERIFY_TOKEN"
    webhook_verify: true
```

### Features

- Webhook-based integration
- Message verification
- Allowlist management

## iMessage (macOS only)

### Setup

1. Ensure macOS with Messages app
2. Enable necessary permissions

```yaml
channels:
  imessage:
    enabled: true
```

### Features

- Native macOS polling from chat.db
- Group chat support
- AppleScript for sending

## User Allowlisting

All channels support allowlisting:

```yaml
channels:
  telegram:
    enabled: true
    bot_token: "..."
    allowed_users:
      - 123456789  # User IDs
      - 987654321
```

## Custom Channel

You can add custom HTTP webhooks:

```yaml
channels:
  custom:
    type: http
    endpoint: "http://localhost:8080/webhook"
    auth_header: "X-API-Key"
```

## Switching Between Channels

Messages are automatically routed to the active agent session. Users on different channels maintain separate conversations by default.

Use session management commands to bridge channels:
- `/session` - View current session
- `/session bridge <user_id>` - Bridge sessions
