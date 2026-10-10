- Expõe `garra_send_message`/`garra_ask` a orquestradores externos com identidade
  própria: `mcp_http.orchestrators[]` (`nome` + `key_env`, valor nunca em config),
  política por orquestrador (subseto de tools e destinos em interseção com a
  allowlist global), auditoria atribuível por chamada e `mcp_http/redacao.rs`
  (redação de padrões de segredo no payload e no erro). Lista vazia (default)
  mantém o comportamento de antes. Refs #1613