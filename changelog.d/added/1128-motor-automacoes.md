- O gateway arma um motor de automacoes declarativas (#1128, epic #1124,
  ADR 0020) dentro do `garraia-hardware`. Com a secao `hardware.automations`
  no config (`dir` com as regras e `risk_ceiling` — default `r1`), o boot
  carrega os arquivos `*.toml`/`*.json` do dir, compila as regras e o motor
  assina o barramento de eventos que os adapters (#1126/#1127) publicam.
- Regra declarativa: gatilho `state_changed` (entidade) ou `cron`; condicoes
  em expressao (`to.state > 32`, avaliadas com fail-closed — erro de
  avaliacao nunca vira `false` silencioso); acoes `device_execute` em
  sequencia; knobs `debounce_secs` (rajada de eventos colapsa em uma
  execucao) e rate limit `max_per_hour` (janela deslizante de 1h).
- O gate de risco rege sem excecao: automacao roda sem canal de confirmacao
  (ninguem vai apertar "aprovar" as 3 da manha), entao o teto de risco do
  config corta o que ela pode pedir — `r0`, `r1` ou `r2`, nunca R3+: acao
  acima do teto fica bloqueada, com a negativa na auditoria. `garra config
  check` recusa `risk_ceiling` fora de `r0`/`r1`/`r2`.
- Toda execucao fica auditada no `automations.db` (mesmo data dir do resto
  do hardware): disparo, resultado (`executada`, `bloqueada_risco`,
  `condicao_falsa`, `condicao_erro`, `rate_limited`, `erro`), o detalhe de
  cada acao e a duracao. Regra desabilitada nao arma; spec quebrada nao sobe
  silenciosa — warn no boot e o motor fica fora ate o arquivo consertar.
- Feature `automations` OFF por default no crate; o gateway (e `garra chat`,
  pela mesma funcao de bootstrap) a liga, e o config decide se o motor arma.
  Sem a secao `hardware.automations`, nada muda — nenhum arquivo de regra e
  lido e o motor nao sobe.