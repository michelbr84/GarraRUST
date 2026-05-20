# Handoff Schema — `.garra-estado.md`

The file `.garra-estado.md` persists the GarraMaxPower pipeline state between
sessions.  Despite the `.md` extension (for human discoverability), the file
format is **TOML**.

## Purpose

When `garra max-power` starts, it reads this file and prints a one-line
summary:

```
[handoff] Última ação: Plan: wrote plan 0157 | Próxima: Implement: implement the module
```

This allows a new session to continue where the previous one left off without
re-reading the entire session transcript.

## Format (TOML)

```toml
schema_version = 1
branch = "routine/202005200619-gar-500-auto-dream-handoff"
linear_issue = "GAR-500"
current_plan = "plans/0157-gar-500-auto-dream-handoff.md"
updated_at = "2026-05-20T06:30:00Z"

[last_action]
kind = "plan"
description = "wrote plan 0157 for GAR-500"
timestamp = "2026-05-20T06:20:00Z"

[next_action]
kind = "implement"
description = "implement handoff module in garraia-common"
```

## Fields

| Field | Type | Description |
|---|---|---|
| `schema_version` | `u32` | Schema compatibility guard. Currently `1`. |
| `branch` | `string?` | Git branch name at time of last write. |
| `linear_issue` | `string?` | Current Linear issue identifier (e.g. `GAR-500`). |
| `current_plan` | `string?` | Relative path to the current plan file. |
| `updated_at` | `string?` | ISO 8601 UTC timestamp of last save. |
| `last_action.kind` | `enum` | One of: `brainstorm`, `spec`, `plan`, `implement`, `review`, `verify`, `merge`, `other`. |
| `last_action.description` | `string` | Short description (auto-redacted, max 500 chars). |
| `last_action.timestamp` | `string?` | ISO 8601 UTC. |
| `next_action.kind` | `enum` | Same variants as `last_action.kind`. |
| `next_action.description` | `string` | Short description (auto-redacted, max 500 chars). |
| `next_action.timestamp` | `string?` | ISO 8601 UTC. |

## Privacy / PII policy

The `description` fields in `last_action` and `next_action` pass through
`garraia_common::handoff::redact()` before storage.  Redaction rules:

1. Email-shaped tokens replaced with `<email>`.
2. JWT-shaped tokens (three base64url segments) replaced with `<token>`.
3. Unix home paths (`/home/…` or `~/`) replaced with `<path>`.
4. Strings truncated to 500 characters.

**No message bodies are stored.**  Only pipeline metadata (branch, issue,
plan reference, action kind + short description) is persisted.

## Git tracking

`.garra-estado.md` is listed in `.gitignore` by default.  To opt in to
tracking it (e.g. in a team repository where all members should share
pipeline state), remove it from `.gitignore` and commit the file.

## Writer

`garraia_common::handoff::save(state, path)` writes the file atomically
(`.tmp` sibling, then rename).

## Reader

`garraia_common::handoff::load(path)` returns `HandoffState::default()` when
the file is missing and `Err(HandoffError)` when it exists but is malformed.
The CLI ignores both the default case and errors at startup (fail-closed).
