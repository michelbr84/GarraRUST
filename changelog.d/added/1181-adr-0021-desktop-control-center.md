- **ADR 0021 propoe o GarraIA Desktop Control Center (#1181).** Uma aplicacao
  grafica central (`garraia desktop`) que reune agentes, providers, integracoes,
  logs e os toggles do passaro e da Chat Bar, sem remover nada do desktop atual.
  A decisao central nao e de UI: `garraia-desktop` esta excluido de todos os
  gates de CI (`ci.yml:307,550,566,729`), entao a logica vai para uma crate nova
  sem Tauri (`garraia-desktop-core`), o conteudo das abas reaproveita a
  superficie `/api/*` que o gateway ja serve, e a aba de agentes e cliente do
  AgentDeck em vez de reimplementar os adapters. Status proposed: a aceitacao e
  do dono.
