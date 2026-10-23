- **Assistente de Primeiro Uso no app desktop (#1636, fatia 3).** Na primeira
  execucao, o Garra Desktop abre um wizard de cinco telas em pt-BR — Boas-vindas
  -> Modo -> QR -> Personalidade -> Pronto — para um usuario nao-dev instalar o
  `.msi`, abrir o app, escanear o QR e conversar com o Garra pelo WhatsApp **sem
  abrir terminal nenhuma vez**. O QR e rasterizado num canvas a partir da matriz
  que o sidecar `garraia whatsapp link --machine` (fatia 1) emite em NDJSON — o
  payload cru (credencial) nunca chega a webview. A escolha "proprio" vs
  "dedicado" persiste em `onboarding.json` (em `app_config_dir()`) e e
  configuravel depois em Ajustes -> WhatsApp (trocar modo e refazer pareamento).
- **Mapeamento de modo e persona para a config (#1636, fatia 3).** "Meu proprio
  numero" aplica `whatsapp access open --yes` (o Garra responde qualquer um que
  mandar mensagem, avisado em tela) e "Numero dedicado" aplica `whatsapp access
  restricted` (so o proprio usuario; o default fail-closed, reaplicado ao trocar
  de volta) — nenhum default de seguranca e afrouxado em silencio. A persona
  (nome, estilo, idioma pt-BR por padrao) vira `agent.system_prompt` pelo novo
  `ConfigLoader::set_agent_system_prompt`. Ao concluir, o gateway reinicia para o
  canal recem-vinculado entrar no ar.
- **Logica do onboarding testada no CI (#1636, fatia 3).** A casca Tauri
  (`garraia-desktop`, fora dos gates de CI) ficou fina: a montagem do comando do
  sidecar, o mapeamento modo->config, o primeiro-uso e o texto da persona vivem
  em `garraia-desktop-core::onboarding` (puro, com testes). A UI tem um teste
  Playwright (`crates/garraia-desktop/tests/`) que abre `onboarding.html` sem
  Tauri e sem gateway, com um `window.__TAURI__` falso, e dirige o fluxo com
  eventos NDJSON gravados — inclusive um QR real da fatia 1 — afirmando a
  navegacao, o canvas desenhado, a persistencia do modo/persona e o caminho de
  erro. O `desktop.yml` ganhou o job `onboarding-ui` e um tripwire de grep no
  `assert-ui-assets`. O roteiro manual do pareamento real (com celular) esta em
  `docs/desktop.md`.
