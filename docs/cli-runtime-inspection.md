# CLI Runtime Inspection (issue #1712, first slice)

The interactive `garra chat` registers a native **read-only** `cli_inspect` tool. This complements the user-facing `/status` and `/tools` commands; it is **not** the gateway's privileged `garra_diagnostics` tool.

## Tool contract

- `{"scope":"identity"}` (default): binary version from the running binary, currently selected provider/model, and the previous completed turn's model as known by the runtime, with a separate boolean for provider-confirmed model names.
- `{"scope":"tools","offset":0}`: a paginated inventory of registered runtime tools with their reported availability **before** the per-turn policy gate. Tool registration never proves permission to invoke it. Each page contains at most 16 items.
- `{"scope":"all"}`: both datasets (page-limited).
- `gateway` and `mcp_servers`: **unknown / not inspected**. This tool does not claim the gateway is reachable, and it never queries a remotely exposed admin endpoint.

Each response includes `origin=cli_runtime`, `read_only=true` and the current channel. No filesystem reads, shell, HTTP calls, credential reads, service restarts, or raw log output are permitted.

## Known limitations / follow-up

This is an initial implementation, **not** resolution of the full issue. Per-turn permission policy is not exposed as a reliable structured value yet; the tool returns `not_verified` for permission. Gateway and MCP health must only be added through a deliberately authorized interface, with role-based filtering and verified observation time. Future work should share one status snapshot between slash commands and agent inspection, broaden permission coverage, and add provider-backed E2E verification.

The tool reads `AgentRuntime::tool_inventory`, `AgentRuntime::disponibilidade_de`, and `AgentRuntime::last_turn_stats` so available fields are grounded in the running process rather than the source repository or prompts.
