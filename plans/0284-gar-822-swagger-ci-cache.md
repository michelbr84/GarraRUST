# Plan 0284 — GAR-822: Fix utoipa-swagger-ui intermittent CI download failure

**Health run:** 97 — 2026-06-08 ~09:00 ET  
**Linear:** [GAR-822](https://linear.app/chatgpt25/issue/GAR-822)  
**Branch:** `health/202606080900-swagger-ci-fix`  
**Priority:** (g) — CI failure on main within last 24h (MSRV check)

---

## Goal

Eliminate the intermittent CI failure caused by `utoipa-swagger-ui v9.0.2` build.rs
downloading the Swagger UI zip via reqwest, which returns a corrupt archive
(`InvalidArchive: "Could not find EOCD"`). This blocks the MSRV gate consistently
and the Windows test job on cache-miss runs.

## Root cause

`utoipa-swagger-ui 9.0.2`'s `build.rs` uses `reqwest` (bundled TLS) to download
`https://github.com/swagger-api/swagger-ui/archive/refs/tags/v5.17.14.zip` at
compile time. When GitHub's CDN returns a truncated/redirect response (intermittent
network issue), reqwest saves it as a non-ZIP file that then fails to unzip with
`InvalidArchive("Could not find EOCD")`.

Evidence:
- Main CI run `27123420182` (2026-06-08 07:48 UTC): MSRV + Windows both failed
- PR #681 MSRV job `80040353069` (2026-06-08 07:15 UTC): same error
- The MSRV job has **no** `target/` cache → every run hits fresh build → consistent failure
- The Test (windows-latest) job fails on cache-miss → intermittent failure

## Architecture

The [CLAUDE.md local workaround](../CLAUDE.md) already documents this pattern:
```
SWAGGER_UI_DOWNLOAD_URL=file:///tmp/swagger-ui-cache/v5.17.14.zip
```
Apply the same pattern in CI:
1. Pre-download with `curl --retry 5` (reliable, handles redirects correctly)
2. Cache the zip via `actions/cache@v5` (avoids re-download across runs)
3. Set `SWAGGER_UI_DOWNLOAD_URL=file://...` so build.rs reads from the local file

## Tech stack

- GitHub Actions `actions/cache@v5`
- `curl` (available on all GHA runners: ubuntu, windows, macos)
- Python3 `pathlib.Path.as_uri()` for cross-platform `file://` URI generation

## Design invariants

- No secrets in CI env vars (the swagger zip is public)
- Cross-platform: uses `$RUNNER_TEMP` (GHA built-in) + Python3 for Windows path conversion
- Idempotent: cache hit skips download; cache miss downloads fresh
- No changes to Cargo.toml / Cargo.lock (version stays at 9.0.2)

## Out of scope

- Upgrading utoipa-swagger-ui beyond 9.0.2 (separate ADR if needed)
- Bundling the zip in the repo (increases repo size ~5 MB)
- Fixing the macOS Test job (it has target/ cache and hasn't shown the failure)

## Rollback

Revert the two hunks in `.github/workflows/ci.yml`. The swagger download reverts to
reqwest. Pre-existing behaviour restored.

---

## Tasks

### M1 — MSRV job fix (T1)

- [x] Add `Cache Swagger UI zip` step (`actions/cache@v5`, key `swagger-ui-v5.17.14`)
- [x] Add `Download Swagger UI zip` step (curl with `--retry 5`, conditional on cache miss)
- [x] Add `SWAGGER_UI_DOWNLOAD_URL: file:///tmp/swagger-ui-cache/v5.17.14.zip` env to `cargo +1.93 check` step

### M2 — Test matrix job fix (T2)

- [x] Add `Set SWAGGER_UI_DOWNLOAD_URL` step (Python3 `pathlib.as_uri()` for cross-platform)
- [x] Add `Cache Swagger UI zip` step (key `swagger-ui-v5.17.14-${{ runner.os }}`)
- [x] Add `Download Swagger UI zip` step (curl, conditional on cache miss)
- [x] Env var flows into `Build workspace` + `Run tests` via `$GITHUB_ENV`

### M3 — Plan + bookkeeping (T3)

- [x] Create `plans/0284-gar-822-swagger-ci-cache.md`
- [x] Update `plans/README.md`
- [x] Push + open PR (base=main) — PR #682
- [x] Merge after CI green — `5f0dfeb` (2026-06-08)

---

## Risk register

| Risk | Likelihood | Mitigation |
|------|-----------|------------|
| Windows Python3 path conversion bug | Low | pathlib.as_uri() is well-tested cross-platform |
| curl download fails in CI too | Very Low | --retry 5 with delay; curl handles redirects correctly unlike reqwest |
| actions/cache quota exceeded | Very Low | zip is ~5MB, well under 10GB quota |
| Cache key collision with other repos | None | key includes repo-specific `v5.17.14` version |

## Acceptance criteria

- [x] MSRV check (1.93) conclusion = success in PR CI
- [x] Test (ubuntu-latest) conclusion = success
- [x] Test (windows-latest) conclusion = success
- [x] Test (macos-latest) conclusion = success
- [x] No new failures introduced

## Cross-references

- GAR-441: Prior MSRV fix (version alignment, Done)
- Plans 0280-0282: Prior health runs that noted this as transient
- CLAUDE.md §"LOCAL SANDBOX NOTES": documents the local workaround

## Estimativa

1 tarefa, ~30min implementação + CI wait.
