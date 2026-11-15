# CLI Runtime Inspection (issue #1712, first slice)

The interactive `garra chat` registers a native **read-only** `cli_inspect` tool. This complements the user-facing `/status` and `/tools` commands; it is **not** the gateway's privileged `garra_diagnostics` tool.

## Tool contract

- `{"scope":"identity"}` (default): binary version from the running binary, currently selected provider/model, and the previous completed turn's model as known by the runtime, with a separate boolean for provider-confirmed model names.
- `{"scope":"tools","offset":0}`: a paginated snapshot of registered runtime tools permitted by the current turn gate. Blocked tools are omitted; each entry reports operational availability separately from permission. The synthetic `tool_program` entry is included only when the runtime gate permits it. Each page contains at most 16 items. Without a dispatch snapshot, permission is `unknown` and the tool list is empty (fail-closed).
- `{"scope":"all"}`: both datasets (page-limited).
- `gateway` and `mcp_servers`: **unknown / not inspected**. This tool does not claim the gateway is reachable, and it never queries a remotely exposed admin endpoint.

Each response includes `origin=cli_runtime`, `read_only=true` and the current channel. No filesystem reads, shell, HTTP calls, credential reads, service restarts, or raw log output are permitted.

## Known limitations / follow-up

This remains an initial implementation, **not** resolution of the full issue. The `tools` scope reports only names admitted by the current turn gate, and it marks the snapshot `unknown` and returns no tool entries if invoked outside the guarded runtime dispatch. Blocked tool names are not disclosed. Gateway and MCP health remain unknown until a deliberately authorized, current source exists. Follow-up still includes provider-backed E2E verification and broader agreement with the user-facing status surfaces.

The tool reads the runtime's guarded current-turn tool snapshot, `AgentRuntime::tool_inventory`, `AgentRuntime::disponibilidade_de`, and `AgentRuntime::last_turn_stats` so permission, availability, and identity fields are grounded in the running process rather than the source repository or prompts.
