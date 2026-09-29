- **`.mcp.json` da raiz volta a subir o servidor MCP (#1536).** A entrada
  apontava para `./target/release/garra`, um caminho relativo a um binario que
  so existe depois de `cargo build --release` e so resolve se o cliente MCP
  abrir exatamente na raiz do repo — em worktree limpo o servidor nunca subia.
  Agora usa `garra` do `PATH`, como o `mcp.json.example` sempre usou. No mesmo
  arquivo faltava `GARRAIA_MCP_ENABLE_TOOLS=1`, sem o qual `tools/list`
  anuncia apenas `garra_ask` e `garra_agent` nunca chega ao host.
