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
- Proactive sends (`telegram_send` tool) — see below

### Proactive messages (`telegram_send`)

From `v0.3.9` the agent can *start* a Telegram message instead of only
replying: a scheduled reminder, "the backup finished", the answer to a
long-running task, or a notification another agent asked for over MCP.

**Replying into the current chat needs no configuration.** With no `chat_id`,
the tool sends into the chat the conversation already belongs to — the person
on the other end started it and is already receiving messages there.

**Sending to a chat the model names explicitly is deny-by-default.** The
operator has to list those chats:

```yaml
channels:
  telegram:
    enabled: true
    bot_token: "YOUR_BOT_TOKEN"
    proactive_chat_ids: [123456789, -1001234567890]
```

Group and supergroup ids are negative. The list is read on every send, so
removing an id takes effect without restarting the gateway. Entries that are
not numbers are ignored rather than coerced — a typo narrows the list, never
widens it.

This list is deliberately **separate** from the user allowlist
(`~/.garraia/allowlist.json`). That one decides who may talk *to* the bot and
has an `open` mode meaning "everyone"; a mode like that would be dangerous for
deciding who the bot may message unprompted. An empty `proactive_chat_ids`
means "refuse", never "allow all".

**Rate limit:** 5 proactive messages per conversation per minute. The agent's
generic tool budget allows far more calls than that, and its loop detector only
catches *identical* repeated calls — so a confused or manipulated agent could
otherwise send dozens of distinct messages. A refused send does not consume the
quota.

Scheduled tasks (`schedule_heartbeat`, `schedule_recurring`) use the same
delivery path. Before `v0.3.9` their Telegram delivery silently failed, because
nothing ever wrote the chat address into the outgoing message (issue #921).

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

## Google Chat

Webhook-driven: the gateway exposes `POST /webhooks/google-chat` and Google
calls it.

### Setup

1. Create a Chat app in the [Google Cloud console] and set its **Connection
   settings** to *HTTP endpoint URL*, pointing at
   `https://<your-gateway>/webhooks/google-chat`.
2. Note the **Audience** the console shows for the app — either your Cloud
   project number or the app URL. This is not optional; see below.
3. Obtain an OAuth2 bearer token for the service account that will post
   replies.

```yaml
channels:
  google_chat:
    type: google_chat
    enabled: true
    audience: "1234567890"
    service_account_token: "YOUR_OAUTH2_BEARER_TOKEN"
```

Both can come from the environment instead — `GOOGLE_CHAT_AUDIENCE` and
`GOOGLE_CHAT_SERVICE_ACCOUNT_TOKEN`. `garra config check` reports either one
missing, and says which failure you get.

### Features

- Webhook-based integration; replies go to the originating space
- RS256 JWT verification on every request, against Google's published keys
- Allowlist management and 6-digit pairing, same as the other channels
- One session per *space*, not per user — a space is a room, and the
  conversation in it is one conversation

### The audience is mandatory

Without `audience` the channel is **refused at boot**. This is the least
obvious of the required fields and the most important one.

Every Google Chat webhook, for every app in the world, is signed by the same
Google service account. Verifying only the signature would therefore accept
a perfectly valid token that Google issued for *somebody else's* app — and
that somebody can simply forward their token to your endpoint. The `aud`
claim is the only thing in the token that says "this one is for you".

Every rejection answers the same `401` with the same body, on purpose: a
different message per failure mode would tell whoever is probing how close
they got. The reason goes to the log only.

### Known limitation

`service_account_key_path` exists in the config and is **not used**. Minting
a bearer token from a service account key requires the OAuth2 JWT bearer
flow, which is not implemented yet — supply `service_account_token`
directly for now.

[Google Cloud console]: https://console.cloud.google.com/apis/api/chat.googleapis.com

## Microsoft Teams

Webhook-driven: the gateway exposes `POST /webhooks/teams` and the Bot
Framework calls it.

### Setup

1. Register a bot in Azure and note its **Microsoft App ID**, secret and
   tenant.
2. Set the bot's messaging endpoint to
   `https://<your-gateway>/webhooks/teams`.

```yaml
channels:
  teams:
    type: teams
    enabled: true
    app_id: "YOUR_APP_ID"
    app_secret: "YOUR_APP_SECRET"
    tenant_id: "YOUR_TENANT_ID"
```

All three can come from the environment instead — `TEAMS_APP_ID`,
`TEAMS_APP_SECRET`, `TEAMS_TENANT_ID`. `garra config check` reports any that
are missing and says which failure each one causes.

### Features

- Webhook-based integration; replies go back to the originating conversation
- RS256 JWT verification on every request, against the Bot Framework's
  published keys
- Allowlist management and 6-digit pairing, same as the other channels
- One session per *conversation* — a Teams conversation is a room, and its
  history is one history

### The app ID is mandatory

Without `app_id` the channel is **refused at boot**. It looks like a plain
identifier and it is in fact what holds the authentication up: every Bot
Framework token is signed by the same Microsoft keys, so the audience claim
is the only thing distinguishing a token issued for *your* bot from one
issued for anybody else's.

### Where the reply goes is checked, not trusted

Unlike the other channels, Teams tells the gateway where to send the reply:
the `serviceUrl` field of the incoming activity. That URL receives the bot's
bearer token, so an unchecked value would let whoever sends an activity
choose which server gets that credential.

Two independent barriers stop that:

1. The Bot Framework puts `serviceurl` in the **signed token**. The gateway
   requires the body's `serviceUrl` to match it, so forging the destination
   would mean forging Microsoft's signature.
2. The outgoing request still goes through the SSRF guard — https only,
   publicly routable addresses only, resolved IPs pinned. Even a legitimately
   signed URL cannot point at `169.254.169.254` or your LAN.

Every rejection answers the same `401` with the same body; the reason goes to
the log only.

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
