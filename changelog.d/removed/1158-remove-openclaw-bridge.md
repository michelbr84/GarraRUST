- Remove `crates/garraia-agents/src/tools/openclaw_bridge.rs` (codigo morto):
  o `OpenClawToolBridge` nunca foi registrado no `tools/mod.rs` nem
  referenciado em lugar algum desde 2026-04-06. A integracao OpenClaw viva
  segue em `garraia-channels` (feature `openclaw`) e `garra migrate openclaw`.
  Recuperavel do historico se tool-sharing entrar no roadmap.
