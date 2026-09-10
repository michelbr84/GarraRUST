- **#1089** o CI passa a compilar e rodar `garraia-agents` com
  `--features mcp`. A feature e OFF por default (`default = []`) e
  `tests/mcp_lifecycle.rs` comeca com `#![cfg(feature = "mcp")]`, entao os tres
  testes de ciclo de vida do MCP (reconexao, deteccao de filho morto e
  disconnect limitado) nunca executavam em nenhum job. O novo step
  `Run clippy + tests (mcp)` segue o mesmo padrao dos steps `signal`, `line` e
  `mcp-http`, que ja fechavam buracos identicos.
