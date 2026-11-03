- **Sondas da escolha de provider e subcomandos `garra mcp` obedecem
  `config.timeouts`; o default descartado deixa de cair em silencio (#1793).**
  `garra chat`, `garra --model <tag>` e `garra ask` sem `-p` perguntam a um
  Ollama/llama.cpp local se ele esta de pe antes de escolhe-lo — o
  `agent.default_provider` quando e local, e o Ollama no fim da autodeteccao —
  e essas duas sondas rodavam no cliente de inferencia: `timeouts.llm`
  (120s) ou `llm.<nome>.timeout_secs` (no Termux, dezenas de minutos). Um
  daemon que aceita a conexao e nao responde segurava a abertura do REPL pela
  janela inteira. As sondas passam a usar `timeouts.health.default_secs` (5s
  por padrao, piso de 1s, a mesma regra da sonda do STT no gateway). Quando o
  default configurado nao serve (sonda sem resposta, entrada ausente em
  `llm:`, tipo que a CLI nao escolhe, sem credencial), a escolha continua
  caindo na autodeteccao, agora com um `warn` que diz a chave e o motivo — o
  caso de um `default_provider` apontando para uma entrada `openai-codex`, que
  a CLI ignorava sem aviso, aparece por ai. E `garra mcp
  inspect|resources|prompts` usavam 30s fixos para falar com o servidor
  (`garra mcp list` mostrava esse 30 como se fosse o timeout dele): passam a
  usar o `timeout` da entrada ou `timeouts.mcp.default_secs` (60s), a mesma
  regra do boot do gateway, agora num lugar so
  (`McpServerConfig::timeout_efetivo`).
