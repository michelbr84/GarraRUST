- README, README.pt-BR e ROADMAP §4.4 diziam que Google Chat, Microsoft
  Teams, Matrix, LINE, IRC e Signal estavam "implementados no crate,
  wiring no gateway pendente", e o badge anunciava "5 channels". Os seis
  estao ligados no build default desde antes disso: `garraia-gateway` liga
  as features de canal incondicionalmente e o `server.rs` instancia os seis
  no boot, sem `cfg` — Google Chat, Teams e LINE por rota de webhook,
  Matrix, IRC e Signal por conexao no boot com retry em backoff. Badge
  passa a 11, a tabela de status distingue 🔹 (ligado, cobertura unitaria no
  crate) de ✅ (ligado e coberto por teste de integracao do gateway), e a
  §4.4 do ROADMAP fica fechada com a pendencia real nomeada: falta teste de
  integracao do gateway para os seis. (#1567)
