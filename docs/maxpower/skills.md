# GarraMaxPower Native Skills

GAR-498 starts the native skills surface for `garra max-power`. These entries
live in `garraia-skills` as Rust registry items instead of loose markdown files.

## Built-ins

| Skill | Purpose | Current output |
|---|---|---|
| `brainstorm` | Explore options, constraints, and the smallest reversible slice. | Deterministic next-step guidance. |
| `write-spec` | Convert a selected idea into acceptance criteria. | Deterministic spec checklist. |
| `write-plan` | Break an accepted spec into safe implementation steps. | Deterministic implementation checklist. |
| `pre-commit` | Prepare safe validation commands before commit. | Safety-gated command plan. |
| `verify` | Delegate to the canonical local validation command. | Safety-gated `garra verify --json` plan. |

## Safety

`pre-commit` and `verify` validate every proposed shell command through
`garraia_common::safety_gate::safety_gate` before returning it. A rejected
command fails the skill run instead of being presented as runnable.

## Runtime Status

This slice provides the first-class registry, metadata, deterministic dry-run
outputs, and unit coverage. CLI routing, direct execution against
`AgentRuntime`, and provider-backed behavior remain for the next GAR-498 slice,
because that wiring is higher risk than the registry foundation.
