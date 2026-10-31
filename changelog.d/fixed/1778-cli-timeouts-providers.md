- **Providers da CLI obedecem `config.timeouts` (#1778).** `garra chat`
  montava os providers com `Client` cru: contra Ollama/llama.cpp o turno nao
  tinha teto de tempo em lugar nenhum (GAP-1) e `timeouts.llm.default_secs`
  era inerte na CLI (GAP-2) — o gateway ja montava o client dele a partir da
  config, a CLI ignorava. Os builders de cliente viram puros e
  compartilhados (`garraia_agents::cliente_llm`, alcancavel pela CLI e pelo
  gateway): connect timeout de 10s e redirects desligados em todo client num
  lugar so, janela global de `timeouts.llm.default_secs` (120s default;
  `0` = sem teto, #1593) e janela por entrada via `llm.<nome>.timeout_secs`,
  que vence a global. O ramo `llamacpp` do gateway, que recebia um client
  vazio, passa a receber o global de verdade. O pull de modelo do Ollama
  segue sem teto, de proposito — a janela de 120s mataria o download no meio.
