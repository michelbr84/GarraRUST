- canais: o bridge OpenClaw passa a ser construido de fato quando ha uma
  secao `[channels.<nome>]` com `channel_type = "openclaw"` e `enabled = true`
  dos dois lados. Ate agora `state.openclaw_client` era `None` constante e as
  quatro rotas `/api/openclaw/*` caiam todas no ramo "nao configurado", com o
  cliente completo — loop de reconexao, conversao nos dois sentidos, status —
  sem ninguem para construi-lo. `garra config check` ganha um aviso para a
  armadilha dos dois `enabled` independentes e para `ws_url` que nao e
  WebSocket. (#1050)
