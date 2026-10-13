- **Redacao extensivel da ponte MCP HTTP (#1626, item 4).** Nova chave opt-in
  `gateway.mcp_http.redact_patterns`: lista de regex do operador (ate 32, 512
  caracteres cada, sintaxe do crate `regex`, tempo linear) que se somam aos
  padroes embutidos e viram `[REDACTED]` no que a ponte publica ou devolve.
  Padrao vazio, que casa o texto vazio, longo demais ou invalido e ignorado e
  reportado como erro pelo `config check`, sem derrubar os demais. Default vazio:
  comportamento inalterado.
