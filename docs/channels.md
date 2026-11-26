# OpenShield channels (Telegram and Discord)

Channels let you talk to the harness from Telegram or Discord the same way
Pocket talks to it: one session, the same approval gate, the same pairing
tokens. They run inside `openshield serve` (or the Termux `openshield serve`
on 127.0.0.1:9310). The TUI does not start bots.

No public server is required. Telegram uses Bot API long polling. Discord
uses the Gateway websocket.

WhatsApp is not supported. Slack and Matrix have config scaffolding only
and are not started by `serve`.

## 1. Create the bots (operator)

These accounts are created by the operator, not by this repo. Do not put
real tokens in `config.toml`, issues, or pull requests. Use environment
variables.

### Telegram

1. In Telegram, talk to [@BotFather](https://t.me/BotFather) and send `/newbot`.
2. Copy the bot token. It looks like `123456:ABC...`.
3. Prefer a private bot. Disable group privacy if you later allow a group,
   which the default `dm_only = true` setting does not.

### Discord

1. Open the [Discord Developer Portal](https://discord.com/developers/applications)
   and create an application.
2. Under **Bot**, add a bot and copy its token.
3. Enable the privileged **Message Content Intent**. Also enable
   **Server Members Intent** only if you need it; OpenShield itself uses
   `GUILDS`, `GUILD_MESSAGES`, `DIRECT_MESSAGES`, and `MESSAGE_CONTENT`.
4. Under **OAuth2 → URL Generator**, select scopes `bot` and
   `applications.commands`, and invite the bot only into guilds you
   control.

## 2. Configure `openshield serve`

In `~/.config/openshield/config.toml`:

```toml
[gateway]
persist_paired_users = true
# allowlist_path = ""   # default: ~/.config/openshield/channels-allowlist.toml

[gateway.telegram]
enabled = true
bot_token = "${TELEGRAM_BOT_TOKEN}"
# allowed_users = []    # empty = deny all except /pair
dm_only = true

[gateway.discord]
enabled = true
bot_token = "${DISCORD_BOT_TOKEN}"
# application_id = "..."
# guild_ids = []        # empty = register slash commands globally
# allowed_channels = [] # empty = DMs and bot-created threads only
# allowed_users = []
thread_per_session = true
```

Export the tokens in the environment (or in a 0600 env file for systemd):

```bash
export TELEGRAM_BOT_TOKEN=...
export DISCORD_BOT_TOKEN=...
```

`openshield serve` will refuse to start an adapter if the token is missing
or still a `${ENV}` placeholder. An empty user allowlist is intentional:
nobody can talk until they pair.

## 3. Pair an account

1. Start the API: `openshield serve` (default `127.0.0.1:9310`).
2. Mint a one-time code (120s, same tokens Pocket uses):

   ```bash
   openshield pair
   ```

   Or `POST /api/v1/pair` with the master `desktop.token`.
3. In a Telegram or Discord **direct message** with the bot, send:

   ```
   /pair <code>
   ```

The user id is appended to `~/.config/openshield/channels-allowlist.toml`
(mode 0600). That file is separate from `config.toml` so a doctor config
rewrite cannot wipe it. The `/pair` message is deleted when the platform
allows it.

## 4. Chat

- Telegram DM: send a message, or `/shield hello`. `/new` starts a fresh
  session. `/status` and `/help` are also available.
- Discord: `/shield [message]` in an allowed guild channel opens a thread
  that is the session. DMs work the same as Telegram. Free-form chat in a
  parent channel is ignored.
- Risky tools post an Approve / Deny / Always button. The 120s timeout
  denies. Pocket, if connected, sees the same session and can also
  approve.
- `/tool` (direct tool execution) is not offered on these surfaces.

## 5. systemd (desktop or VPS)

A unit file lives at `packaging/systemd/openshield.service`. Install it,
create an `openshield` user, put tokens in `/etc/openshield/env` (mode
0600), then:

```bash
sudo systemctl enable --now openshield
```

Do not put tokens in the unit file.

## 6. Termux:Boot (phone)

Channels do not need a second process. The phone build already starts
`openshield serve` from `~/.termux/boot/` after `termux-wake-lock`. Enable
Telegram (and later Discord) in `config.toml` on the phone and restart
serve.

Practical limits:

- Grant Termux a battery exemption and keep the wake lock, or Doze can
  stall long polling or the Discord websocket.
- Only one process should poll a given Telegram token. Do not run serve
  on a desktop and on the phone against the same bot.
- The first Android binary can be
  `--no-default-features --features web-api,telegram`. Add `discord` once
  a rustls build is proven for `aarch64-linux-android`.
- Mint a pairing code with Pocket (it already calls `POST /api/v1/pair`
  on loopback) and paste `/pair <code>` into Telegram on the same phone.

## 7. Security notes

- Empty `allowed_users` plus `enabled = true` denies everyone except
  `/pair`. This is a change from the old TUI adapters, which treated an
  empty list as "everyone."
- Bot tokens must stay in the environment. `Config::save` writes `${ENV}`
  placeholders, never a resolved token. Debug logs redact tokens.
- Outbound replies, tool results, and approval argument previews go
  through secret and PII redaction.
- Group chats are off by default (`dm_only = true`). Discord talks in
  DMs and in bot-created threads.
