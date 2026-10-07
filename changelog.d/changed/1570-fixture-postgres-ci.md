- **Fixture de Postgres no CI destrava os 10 testes de integração do gateway (#1570).**
  Job novo `Gateway Integration (Postgres)` no `ci.yml`: service container de
  `postgres:16.8-alpine` + as env vars que o bootstrap do `GatewayServer`
  exige (`GARRAIA_JWT_SECRET`, `GARRAIA_REFRESH_HMAC_SECRET`,
  `GARRAIA_LOGIN_DATABASE_URL`, `GARRAIA_SIGNUP_DATABASE_URL` — segredos
  fictícios de CI, mesmo padrão do job e2e) + `--include-ignored`. Os 10
  `#[ignore]` de `auth_test.rs`, `gateway_integration.rs` e `voice_e2e_test.rs`
  (causa-raiz única: `server.run()` saía em silêncio sem Postgres desde o
  fix/ci-triage-2026-04-15) agora rodam de verdade no CI; a mensagem de cada
  `#[ignore]` passa a apontar o job e o comando local. O `#[ignore]` de
  throughput do `markdown.rs` permanece — é sonda, não dívida. Também neste
  PR: baseline `.quality/baseline.json` re-medido com
  `freeze-baseline.py --adopt-current-file-metrics --reason '#1570'` (campos
  de tamanho passam a descrever o repositório atual; `coverage` 72,0% e
  `audit` 0 critical/0 high preservados), workflow renomeado de "Quality
  Ratchet" para "Quality Report" para o nome não prometer bloqueio que o modo
  report-only (plan 0064) não faz, e healthcheck no serviço `ollama` do
  compose (`ollama list`) — o `garraia` e os demais serviços já tinham.
