- **ADR 0021 (GarraIA Desktop Control Center) aceito pelo dono (#1181).** A opcao D
  passa de proposta a decisao: crate `garraia-desktop-core` sem Tauri (no CI, com
  testes), casca Tauri fina, abas servidas pela superficie `/api/*` do gateway (as
  mutantes ja cobertas pela guarda anti-CSRF do #1182) e aba Agents como cliente do
  AgentDeck. Os milestones do epico podem comecar, na ordem core -> CLI -> casca ->
  abas. Ficam pendentes, como o ADR previa, o gatilho S1 do ADR 0007 (framework de
  UI, no inicio do M2) e o desenho da dependencia do AgentDeck (M3).
- **Decisoes de produto do threat model fechadas pelo dono (#1182, #1191).** O
  pareamento continua global por instalacao (um codigo do `/pair` vale em qualquer
  canal habilitado; o `channel_id` do `PairingManager` e informativo), o codigo fica
  em 6 digitos (com 20 palpites por ciclo a chance de acerto e 2e-5, e o codigo e
  ditado por voz), os limites de tentativa ficam fixos (5 por usuario / 15 min / 20
  globais) e alcancar o console por nome DNS segue exigindo `gateway.allowed_origins`.
  Tudo registrado nas secoes 5.10 e 5.11 de `docs/security/threat-model.md`.
