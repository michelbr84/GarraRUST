# What's New in v0.5.3

> 🇧🇷 [Versão em português](Novidades-v0.5.3) · 📋 [Full CHANGELOG](https://github.com/michelbr84/GarraRUST/blob/main/CHANGELOG.md) · 📦 [Download](https://github.com/michelbr84/GarraRUST/releases/tag/v0.5.3)

Stable release of November 27, 2026 — replaces v0.5.2 as the `latest` release.

v0.5.3 is a fix release, almost entirely security. It fixes advisory
[GHSA-g6c8-mx6x-5g92](https://github.com/michelbr84/GarraRUST/security/advisories/GHSA-g6c8-mx6x-5g92),
which affects v0.4.1 through v0.5.2. **If you have an IRC channel configured,
update**, and read [Heads-up when updating](#heads-up-when-updating) first.

## IRC

- **Allowlist (GHSA-g6c8-mx6x-5g92, #2015).** There is one allowlist for every
  channel, and the other channels store their ids in it without a prefix. A
  nick equal to the id of someone admitted through another channel got in as
  that person, owner included: a Slack id (`U01ABCDEF`) is a valid nick. IRC
  now only accepts `irc:<nick>` entries, and the nick is compared using the
  server's `CASEMAPPING`: an admitted `Ana` comes back as `ana` and, under
  `rfc1459`, `[bia]` comes back as `{bia}`.
- **One private conversation per person (#2011).** Everyone's private
  conversations landed together in the bot nick's session, and the model saw
  one person's messages in another person's turn. Each person now has their
  own session, in each IRC connection of the config.
- **Model replies never become commands (#2007).** A `\r` in the model's text,
  which prompt injection can influence, reached the socket raw and turned
  into an IRC command (`QUIT`, `JOIN`). Every write of model text now goes
  through a single point that splits lines and stays within the protocol's
  512 bytes.

Details: [docs/hardening-gateway.md](https://github.com/michelbr84/GarraRUST/blob/main/docs/hardening-gateway.md).

## Fail-closed admin console (#1992, #2000, #2013)

- **Unsafe or wrong master key.** This happens with an unreadable or deleted
  KDF row, a changed passphrase or a swapped `master.key`. In those cases
  `/admin` answers 503 `{"error":"admin_unavailable"}` and the log gets an
  `ERROR` line with the reason. Before, the boot derived a key from the
  legacy salt or encrypted secrets under the wrong key. Chat, channels and
  `/api/*` stay up.
- **An `admin.db` that exists but does not open refuses the boot.** Before, it
  became an empty in-memory console, and whoever reached `/admin` first
  created the admin.
- **A new `master.key` is born with mode 0600** from its first byte.

Guide: [docs/security.md](https://github.com/michelbr84/GarraRUST/blob/main/docs/security.md), section "Admin console answering 503".

## Scheduled tasks reply on more channels (#2002)

A scheduled task's reply now reaches Discord, Slack, Matrix and linked
WhatsApp, as well as Telegram. The other channels stay without delivery; on
IRC and iMessage, on purpose. `garraia runs list`, `--json` and
`GET /api/runs` show the outcome in the `delivery` column: `entregue`,
`sem_entrega` or `nao_entregue:<reason>`.

## Installers (#2016, #2023)

- **Windows from the latest release.** The command downloads the
  `install.ps1` that shipped with the binaries it installs. The `main` raw
  file stays as a mirror, and `garraia.org/install.ps1` does not work with
  `irm`.
- **jsDelivr is gone.** It is no longer a mirror for the installers, because
  it refuses repositories over 50 MB.

## Heads-up when updating

- **Paired over IRC up to v0.5.2:** the allowlist entry has no `irc:` prefix
  and no longer works on IRC. The person needs a new `/pair` code, which the
  owner generates on Telegram or Discord, on a channel where they are the
  owner. The bot stays silent with anyone it turns away.
- **Nowhere to generate a code:** this happens when the owner was paired over
  IRC, because the other channels' `/pair` only answers that channel's owner,
  and on an IRC-only installation. The CLI and the API do not generate codes,
  so the way out is the file. With the gateway stopped, change every
  `"<nick>"` to `"irc:<nick>"` in `allowlist.json`, then start it again. That
  means `users`, for each affected person and for the owner too, and `owner`.
  Changing `owner` alone is not enough: on IRC, `users` is what lets people
  in. Only do this for nicks you know: `irc:<nick>` admits whoever uses that
  nick on the server. The welcome message IRC sends the new owner points to
  the same path.
- **`admin.db` that does not open:** the boot is refused, naming the file. Fix
  the permission or the disk space, or restore from backup.
- **`/admin` answering 503:** see the guide above.
- **Old IRC private conversations:** they stay in the old session, which gets
  no new turns. Each person starts a new conversation.
- **Admin panel (`/admin`) browser:** now requires Chrome 93, Firefox 92 or
  Safari 15.4. The chat at `/` does not change.

## Install or update

```bash
# Linux/macOS
curl -fsSL https://garraia.org/install.sh | sh
```

```powershell
# Windows
irm https://github.com/michelbr84/GarraRUST/releases/latest/download/install.ps1 | iex
```

Already on v0.5.2? `garra update`.

Docker image: `ghcr.io/michelbr84/garraia:0.5.3`.
