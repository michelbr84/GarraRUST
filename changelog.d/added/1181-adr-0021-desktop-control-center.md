- **ADR 0021 propoe o GarraIA Desktop Control Center (#1181).** Uma aplicacao
  grafica central (`garraia desktop`) que reune agentes, providers, integracoes,
  logs e os toggles do passaro e da Chat Bar, sem remover nada do desktop atual.
  A decisao central nao e de UI: `garraia-desktop` so tem build em PR (via
  `desktop.yml`, que nao e check obrigatorio) e nenhum lint ou teste, entao a
  logica vai para uma crate nova sem Tauri (`garraia-desktop-core`), que cai nos
  checks obrigatorios. As abas reaproveitam a superficie `/api/*` do gateway,
  com as mutantes condicionadas ao conserto da exposicao cross-origin (#1182), e
  a aba de agentes e cliente do AgentDeck em vez de reimplementar os adapters.
  Status proposed: a aceitacao e do dono.
