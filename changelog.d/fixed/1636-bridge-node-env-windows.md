- **Ponte do WhatsApp agora sobe o Node no Windows (#1636).** O `env_clear()`
  da contencao do processo filho tirava `SystemRoot` e os demais essenciais de
  sistema, e o Node abortava logo no boot com `Assertion failed:
  ncrypto::CSPRNG(nullptr, 0)` (codigo 134) — a inicializacao do RNG de cripto
  nao acha a cripto do Windows sem eles. O bridge passa a herdar, so no Windows,
  os mesmos essenciais que o transporte stdio dos servidores MCP ja herdava
  (`SystemRoot`, `SystemDrive`, `windir`, `COMSPEC`, `PATHEXT`, `USERPROFILE`,
  `APPDATA`, ...), nenhum deles segredo nem vetor de execucao de codigo. Afetava
  pareamento, atendimento e `npm ci`, nao so o novo `self-test`.
