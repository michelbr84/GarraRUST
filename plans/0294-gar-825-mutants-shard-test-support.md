# Plan 0294 — GAR-825: Q6.14 Systemic fix — 3-way shard + --features test-support in mutants.yml

## Goal

Add `--features test-support` and 3-way matrix sharding to
`.github/workflows/mutants.yml` so that the 8 testcontainer-backed integration
suites in `garraia-auth` actually run during mutation testing, killing the
security-critical mutations that have survived every weekly run since Q6.11.

## Architecture

### Root cause (from GAR-774 / GAR-824)

`cargo mutants --package garraia-auth` without `--features test-support` silently
skips all 8 test binaries gated by `required-features = ["test-support"]` in
`crates/garraia-auth/Cargo.toml`. This includes `sessions_lifecycle`,
`app_pool_role_guard`, `verify_internal`, `signup_flow`, `extractor`,
`debug_redaction_pools`, `concurrent_upgrade`, `skeleton`.

As a result, mutations in `sessions.rs:115/158`, `signup_pool.rs:139`,
`app_pool.rs:203` survive uncaught because no test exercises those branches.

### Why sharding is required

Adding `--features test-support` causes every mutant invocation to start a
`pgvector/pgvector:pg16` Docker container via `testcontainers-rs`. The harness
uses a process-wide `tokio::sync::OnceCell<Arc<Harness>>`, so each `cargo test`
binary starts exactly one container. With Docker image already cached:

- Container startup: ~3-5 s per mutant
- Current baseline per mutant: ~35 s
- With testcontainers: ~40-45 s per mutant
- 166 mutants × 43 s ≈ 119 min — fits in 150-min window **without sharding**

However, sharding is still recommended to:
1. Provide ~45-min headroom per shard for future test growth.
2. Allow parallel completion (wall-clock ~40 min vs. 120 min serial).
3. Match the approach recommended in the issue for long-term sustainability.

### 3-way matrix strategy

Three parallel jobs, each handles ~55 mutants:

```
shard 0/3 → mutants 0..54
shard 1/3 → mutants 55..109
shard 2/3 → mutants 110..165
```

Each job:
1. Pre-pulls `pgvector/pgvector:pg16` to warm the Docker cache.
2. Runs `cargo mutants --shard ${{ matrix.shard }}/3 --features test-support`.
3. Uploads its own artifact `mutants-report-<run_id>-shard-<N>`.

## Tech stack

- `cargo-mutants ^25` — supports `--shard N/M` (introduced 24.7.0)
- `ubuntu-latest` — Docker preinstalled; testcontainers spawns containers in-process
- GitHub Actions matrix strategy — 3 parallel jobs per workflow run

## Design invariants

- **No `continue-on-error`**: mutation failures surface as red in Actions tab.
- **Off PR path**: schedule (Mon 05:00 UTC) + `workflow_dispatch` only.
- **Concurrency guard unchanged**: prevents concurrent workflow runs; does not
  block parallel shard jobs within a single run.
- **Artifact naming**: `mutants-report-<run_id>-shard-<N>` — unique per shard,
  all uploaded even on partial failure (`if: always()`).
- **`--no-shuffle` preserved**: stable ordering across runs for week-over-week
  comparison. With sharding, the same shard always covers the same mutants.

## Validações pré-plano

- [x] `cargo-mutants ^25` supports `--shard N/M` — confirmed via release notes.
- [x] `ubuntu-latest` has Docker — GHA docs confirm Docker 25+ preinstalled.
- [x] Harness uses `pgvector/pgvector:pg16` image — confirmed in `tests/common/harness.rs:69-70`.
- [x] `OnceCell` ensures one container per `cargo test` invocation — confirmed in `tests/common/harness.rs:26`.
- [x] 8 test binaries are gated by `required-features = ["test-support"]` — confirmed in `Cargo.toml`.
- [x] `--features test-support` enables `test-support` on the crate under test.

## Out of scope

- Enabling `--features test-support` on any crate other than `garraia-auth`.
- Adding testcontainers to other crates' mutation runs.
- Enforcing a mutation score threshold (still report-only).
- Combining shard artifacts into a single report (shard artifacts are independent).

## Rollback

Revert `.github/workflows/mutants.yml` to the version in this PR's parent commit.
No schema or code changes — workflow file only.

## File structure

```
.github/workflows/mutants.yml    ← changed (3-way shard + test-support)
plans/0294-gar-825-mutants-shard-test-support.md  ← this file
plans/README.md                  ← new row
```

## Tasks

### M1 — Implement 3-way sharding + --features test-support

- [x] Read current `.github/workflows/mutants.yml` (done above)
- [x] Add `strategy.matrix.shard: [0, 1, 2]` + `fail-fast: false`
- [x] Add `docker pull pgvector/pgvector:pg16` pre-pull step
- [x] Add `--features test-support` and `--shard ${{ matrix.shard }}/3` to cargo mutants command
- [x] Update artifact name to `mutants-report-${{ github.run_id }}-shard-${{ matrix.shard }}`
- [x] Update job name to include shard context
- [x] Update timeout comment
- [x] Workflow-only change — no Rust code modified, no `cargo check` needed

## Risk register

| Risk | Likelihood | Impact | Mitigation |
|------|-----------|--------|-----------|
| Docker image pull slow on cold runner | Low | Medium | Pre-pull step warms cache before mutation run |
| Container startup adds >40s per mutant | Low | Medium | `OnceCell` shares container within one `cargo test`; only 1 startup per mutant |
| Shard 2 has more complex mutants, times out | Low | Low | `--no-shuffle` gives stable distribution; 150-min budget with sharding leaves ~110-min headroom |
| `--shard` semantics change between cargo-mutants versions | Very low | Medium | Pinned to `^25`; sharding stable since 24.7.0 |

## Acceptance criteria

- `.github/workflows/mutants.yml` passes `--features test-support` with `--shard ${{ matrix.shard }}/3`
- 3 parallel shard jobs run on next `workflow_dispatch` trigger
- Each shard job uploads its artifact `mutants-report-<id>-shard-<N>`
- On the next Monday scheduled run, `sessions.rs:115`, `:158`, `signup_pool.rs:139`, `app_pool.rs:203` appear as CAUGHT
- No `continue-on-error: true` introduced

## Cross-references

- GAR-774 (Q6.11): root cause diagnosed
- GAR-824 (Q6.13): partial fix via pure-fn extraction
- GAR-436 (Q6 epic): parent
- GAR-825: this issue

## Estimativa

- LOC changed: ~40 (workflow file only)
- Complexity: Low
- Time: < 30 min implementation
