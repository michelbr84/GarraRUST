- **A recusa de uma ferramenta nomeia a camada que barrou (#1915).** No
  registro de capacidades (`garra_status`, `GET /admin/api/capabilities`) toda
  ferramenta negada saia com o mesmo `reason_code: policy` e a frase "modo da
  sessao ou teto de quem fala" — o agente nao sabia qual dos dois valia nem o
  que mudaria o estado, e `bash`, `file_write` e uma ferramenta MCP conectada
  apareciam com o mesmo motivo. Uma linha `denied` agora traz `denied_by`
  (`mode` | `principal_ceiling` | `mode_and_principal_ceiling`) e
  `denied_because` (`no_tools` | `denied_by_name` | `denied_capability` |
  `unclassified` | `not_in_allowlist`), com motivo e remediacao proprios:
  modo e do usuario (`/mode`), teto e do operador e trocar de modo nao
  resolve, e MCP sem anotacao e "classificacao ausente", nao defeito de
  conexao. O texto nao cita nome de modo, politica nem servidor, e nao sugere
  contorno. `reason_code` continua `policy` e os campos novos sao aditivos;
  sem portao conhecido a linha fica generica, como antes. A explicacao e o
  enforcement leem a mesma funcao (`recusa_da_politica`), e um teste varre
  modo x teto x ferramenta para provar que nao divergem. Nenhum modo, teto,
  raiz ou jail foi alterado.
