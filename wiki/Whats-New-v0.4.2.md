# What's New in v0.4.2

> 🇧🇷 [Versão em português](Novidades-v0.4.2) · 📋 [Full CHANGELOG](https://github.com/michelbr84/GarraRUST/blob/main/CHANGELOG.md) · 📦 [Download](https://github.com/michelbr84/GarraRUST/releases/tag/v0.4.2)

The release where Garra stepped off the screen. The hardware epic shipped in
full — all seven slices, from the `Device` trait to packaged skills — and with
it the agent can read sensors, turn on lights and react to events in your
home, **under the same risk gate that governs every other tool**.

Alongside it: streaming in the mobile app, conversations that survive the
process, an admin panel that no longer needs manual SQL for anything, and a
security pass that closed a secret leak through `/proc`.

---

## Garra in the physical world

Until now the agent saw files, chat and the web. Now it also sees what's
plugged in.

```text
Garra Core   (runtime · memory · automations · security)
     |
garraia-hardware   Device trait · Capability with R0-R5 risk · registry · gate
     |
Adapter / Hardware Skill   mqtt · home_assistant · serial · gpio
     |
Device   the lamp, the sensor, the lock, the board
```

| Transport | What it brings in | Config |
|---|---|---|
| **MQTT** | anything publishing to your broker — DIY ESP32, Tasmota, Shelly, zigbee2mqtt | `hardware.mqtt` |
| **Home Assistant** | the hub's entities: light, climate, sensor, lock, cover — and through them Zigbee, Matter, Z-Wave | `hardware.home_assistant` |
| **Serial/USB** | Arduino and ESP32 over the cable, line-delimited JSON | `hardware.serial` |
| **GPIO** | Raspberry Pi pins | `hardware.gpio` |

**None of this turns itself on.** Without the config section the registry
starts empty and `device_list` lists nothing — no network discovery runs by
default, and every transport is a compile-time feature that ships off.

### Declarative automations

With `hardware.automations`, TOML/JSON rules become
`trigger → condition → action`: state change or cron, conditions over
readings, and the action going through the **same policy** as the runtime,
under a declared risk ceiling.

### R0-R5: what the agent may do on its own

| Level | Example | What happens |
|---|---|---|
| R0 | read temperature | automatic |
| R1 | turn on a light | turn policy |
| R2 | open a blind | policy + rate limit |
| R3 | unlock a door | **human confirmation required** |
| R4 | turn on the oven | explicit approval |
| R5 | industrial actuator | denied unless operator-allowlisted |

Three invariants hold the table together: a read is always R0 and never
writes; without a real confirmation channel, R3/R4/R5 **do not run** (they
never degrade into silent execution); and whatever never passed authentication
— a board on a USB cable — does not classify its own risk: its table is closed
in code.

### Integrations packaged as skills

An integration now fits in a `SKILL.md` with `kind: hardware-adapter` or
`hardware-preset`: transport, capabilities, and "entity → capability" presets
with synonyms in Portuguese and English, so you can say *"turn on the living
room light"* instead of `light.sala_teto` + `power`.

Six official skills ship with the repo — Home Assistant, MQTT,
serial/Arduino, ESP32, Zigbee and Matter. **Zigbee and Matter arrive as
presets over the hub**, not as their own stack: Home Assistant already solved
the coordinator and the pairing.

And the two rules a skill does not get to choose: the transport list is closed
(anything else loads inert — visible, never active), and **a skill may only
raise risk, never lower it**. A manifest trying to downgrade `door_unlock`
from R3 to R1 to dodge the confirmation changes nothing.

📖 [`docs/hardware.md`](https://github.com/michelbr84/GarraRUST/blob/main/docs/hardware.md) · [`docs/hardware-skills.md`](https://github.com/michelbr84/GarraRUST/blob/main/docs/hardware-skills.md)

---

## In the app and in the terminal

- **Garra Mobile's chat now streams.** The app opens the gateway's WebSocket,
  resumes the session it already had and appends deltas into a growing bubble;
  an indicator names the tool currently running, and Stop cancels only the
  addressed turn. If the socket won't open — older gateway, a proxy refusing
  the upgrade, LAN down — the app silently falls back to plain HTTP.
- **`garra chat --persist` and `--resume <SESSION_ID>`.** Conversations no
  longer die with the process. It's explicit opt-in: with neither flag, **no**
  database is opened and nothing hits disk — exactly the previous behavior.
- **`garra about`** stopped emitting ANSI when the output isn't a terminal
  (`about > file`, pipes, `NO_COLOR`, `TERM=dumb`).

---

## Admin panel: no more manual SQL

- **2FA at login.** The TOTP the project already implemented was only
  reachable through the mobile flow; the panel's login now requires the second
  factor when the account has 2FA on — with a lockout that survives restarts
  and a secret encrypted with the vault key.
- **Change your own password from the UI**, re-verifying the current one.
- **Recovery without e-mail:** `garra admin recovery start --username X` mints
  a single-use code that **never comes back in the HTTP response** — the
  gateway stores only its hash and writes the text to a `0600` file, so
  reading the code requires a shell on the machine. The route answers
  identically whether the user exists or not, so it can't become an
  enumeration oracle.

---

## Security

- **The `/proc` channel is closed.** The process now calls
  `prctl(PR_SET_DUMPABLE, 0)`: until now a same-UID child process could read
  the parent's `/proc/<pid>/environ` and reach `GARRAIA_JWT_SECRET`,
  `ANTHROPIC_API_KEY` and friends. Measured with a control: without the fix
  the child reads the secret; with it, `EACCES`.
- **`ask` mode now denies `bash`.** Denying `file_write` was decorative while
  the model could write the file through the shell.
- **The `orchestrator` mode's allowlist now applies** — it was declared and
  ignored.
- **`agent.bash_allowlist`** lets the operator declare trusted commands, with
  a deliberately poor syntax (wildcard only at the end, or an exact command)
  that never forgives a dangerous command or a compound one.
- **Mutating `/api/learning/` routes** now require same-origin and a local
  peer when no API key is configured.

---

## Operations and hygiene

- **The gateway's restart circuit breaker now actually exists:**
  `StartLimitIntervalSec`/`StartLimitBurst` lived in `[Service]`, where
  systemd silently ignores them — a crash loop restarted forever. CI now fails
  on any ignored or misplaced key in the unit.
- **Voice being down no longer goes unnoticed:** `GET /api/diagnostics` gained
  `voice.tts` and `voice.stt` rows carrying the exact start command in
  `next_step` — and the docs stopped citing commands that don't exist on PyPI.
- **Tighter CI:** agent frontmatter YAML lint, clippy+test pairs for each
  hardware feature (`mqtt`, `home-assistant`, `automations`, `skills`), and
  the test that loads the repo's official skills.
- **rmcp 2.2 → 3.3** on both the MCP client and server.

---

## Upgrading

```bash
garra update          # existing installs
```

Or reinstall: `curl -fsSL https://garraia.org/install.sh | sh`
(Windows: `irm https://garraia.org/install.ps1 | iex`).

No migrations, and no existing configuration changes meaning — hardware only
turns on with a new config section.
