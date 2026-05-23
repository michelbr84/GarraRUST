# GAR-498 Native Skills Registry Slice

## Goal

Start the GAR-498 Skills MVP by making the initial `brainstorm`, `write-spec`,
`write-plan`, `pre-commit`, and `verify` skills first-class Rust entries in
`garraia-skills`.

## Scope

- Add a native skill trait and registry in `garraia-skills`.
- Register the five GAR-498 MVP skills as built-ins.
- Return deterministic dry-run outputs for workflow skills.
- Validate all command-producing skills with the central bash safety gate.
- Document current status in `docs/maxpower/skills.md`.

## Out of Scope

- Provider-backed execution through `AgentRuntime`.
- Full orchestration for GAR-499.
- Replacing the existing markdown skill parser/scanner/installer.

## Validation

- Targeted unit coverage added in `crates/garraia-skills/src/native.rs`.
- Local Rust tests could not be executed in this environment because
  `cargo`, `rustc`, and `rustfmt` were not available on PATH.
