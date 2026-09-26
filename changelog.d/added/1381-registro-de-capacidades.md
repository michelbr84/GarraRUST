- **Registro de capacidades do runtime (#1381, #1387, #1416; ADR 0025 §5).**
  Uma funcao so (`capacidades_registro::registro`) reune o inventario do
  runtime, o portao do turno (nome e classe), a disponibilidade de cada
  ferramenta, o estado dos servidores MCP e a exposicao do `bash` numa
  visao por capacidade com estado — `visible`, `denied`, `unavailable`,
  `unhealthy`, `not_configured` — e motivo legivel por maquina e por humano
  (mais remediacao). `garra_status` passa a devolver `capabilities` (e a
  nota do prompt manda o modelo responder a partir dela: `denied` e
  "existe e nao esta liberada", nunca "nao existe"; `unavailable` repete a
  remediacao); o `/api/diagnostics` ganha `tools.capabilities` (contagens
  por estado e o que esta indisponivel/fora do ar/nao configurado);
  `GET /admin/api/capabilities` expoe o mesmo registro ao console.
  `device_read`/`device_execute` ficam indisponiveis (`no_devices`) sem
  dispositivo registrado; `device_list` continua, porque e ela que explica.
