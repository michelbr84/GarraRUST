# What's New in v0.4.7

> 🇧🇷 [Versão em português](Novidades-v0.4.7) · 📋 [Full CHANGELOG](https://github.com/michelbr84/GarraRUST/blob/main/CHANGELOG.md) · 📦 [Download](https://github.com/michelbr84/GarraRUST/releases/tag/v0.4.7)

A **short fix release**. It exists for one reason: a bug seen in production on
v0.4.6 killed whole agent turns *after* all the work had already been done. If
you are on v0.4.6, this is the update that matters to you.

Riding along are two things that were already finished and green on `main`:
**named permission presets** for personal WhatsApp, and a dependency-family
update.

---

## Parallel tool batches stop blowing the budget and killing the turn (#1523)

**What used to happen.** The model can ask for several tools in a single reply —
a "parallel batch". The execution budget, however, was only checked **before
each call to the model**, never between the tools inside one batch. A batch of
15 tools ran in full against a ceiling of 10.

The effect was worse than overspending. In `search` mode — the floor for
personal WhatsApp, 10 calls per task — the turn **died** with
`execution budget exceeded` after everything had already run, without the model
seeing a single result. On the user's side, the chat replied "try again in a
moment" for a request that would fail identically the second time.

**What changed.** The budget is now checked **per call**, inside the batch:

- the tool that would cross the ceiling **does not run**, and the model gets the
  reason back — it learns it hit the limit instead of finding out too late;
- once the budget is spent, the model gets **one final, announced turn** to
  answer with what it already collected. Nothing it asks for in that turn runs.

In practice: a large request over WhatsApp now ends with an answer based on
whatever could be gathered, instead of ending in an error.

## Named permission presets (#1434)

One more piece of the v0.4.6 [Access Policy v2](Seguranca-e-Operacao). Instead
of combining `level` and `write` by hand every time, you use a name:

```bash
garra whatsapp preset <number> chat_only    # chat only, no tools
garra whatsapp preset <number> read         # read-only
garra whatsapp preset <number> developer    # full
garra whatsapp preset <number> full_pod     # full
```

The same name works for the default of anyone not on the list, and for groups:

```bash
garra whatsapp access default --preset read
garra whatsapp group <id> --preset chat_only
```

Three design details that matter:

- a preset is an **alias for a canonical combination**, not a new field in
  `config.yml`. There is no stored label that can drift from what the gate
  actually enforces — it writes `level` and `write` through the same single
  mutation path as the rest of the policy;
- `developer` and `full_pod` compile to the **same** `full`. What decides real
  power is still the session mode plus `execution.profile`
  ([ADR 0024](https://github.com/michelbr84/GarraRUST/blob/main/docs/adr/0024-perfis-de-execucao-isolated-pod.md)),
  which is why both remain **rejected** in `access.default` — an unknown sender
  does not start at `full`;
- `garra whatsapp access --json` and `GET /admin/api/whatsapp/access` now return
  a `preset` label **computed live** from `level`/`write`, with `custom` for
  anything no preset covers.

## Dependencies: `utoipa` 6 and the `opentelemetry` 0.33 family (#1526, #1527)

Both moved **as a block**, because they do not move any other way: `utoipa` and
`utoipa-swagger-ui` exchange the `OpenApi` type with each other, and the
`opentelemetry` crates share a facade — bumping one alone puts two incompatible
versions of the same facade in the graph and the build breaks. No code changes
were needed.

So it does not happen again, `.github/dependabot.yml` gained `utoipa` and
`opentelemetry` groups, in the same shape as the existing `wasmtime` group.

---

## How to update

```bash
garra update            # CLI; keeps the previous version and supports `garra rollback`
```

Fresh installs go through the [usual installer](Instalacao-e-Primeiros-Passos).

## v0.4.6 stays exactly where it is

The v0.4.6 Release, its tag and its assets were **not touched**. The project
runbook is explicit: never reuse a published tag, because `garra update` verifies
SHA-256 and clients may have cached the binaries
([`docs/releasing.md`](https://github.com/michelbr84/GarraRUST/blob/main/docs/releasing.md)
§Rollback). The fix arrives on top, in this version.

## Still open

- The **global** Agents & Permissions page (#1433) and policy **import/export**
  (#1435) remain open, deferred by a scope decision — permission territory does
  not belong in a fix release.
- Windows installers and desktop bundles are still **unsigned**.
- The Tauri desktop auto-updater is still off by design; the supported update
  path is the CLI's `garra update`.
