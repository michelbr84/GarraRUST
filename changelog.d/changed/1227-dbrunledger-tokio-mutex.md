- **`DbRunLedger` passa a aceitar o `Arc<tokio::sync::Mutex<SessionStore>>` que o
  gateway ja guarda (#1227).** O adapter do ledger de runs exigia
  `std::sync::Mutex`, enquanto o `AppState` do gateway guarda o `SessionStore`
  atras do mutex do tokio - tipos incompativeis, entao o wiring era impossivel
  sem um segundo store so para o ledger. O trait `RunLedger` vira `async` (via
  `async_trait`, a mesma excecao documentada de `garraia_storage::ObjectStore`:
  e consumido como `dyn`), `NoopLedger` e `DbRunLedger` acompanham, e
  `SubAgentConfig` ganha `session_id` (builder `with_session_id`), que o
  `spawn_agent` repassa ao `on_start` - antes o run nascia sempre com
  `session_id = NULL`. Continua sem chamador de producao: o adapter agora e
  utilizavel, mas nenhum caminho do gateway/CLI constroi um `AgentCoordinator`,
  e o wiring na subida segue sendo a #1227.
