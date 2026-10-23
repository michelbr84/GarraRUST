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
  numero" aplica `whatsapp access open --yes` — o Garra responde qualquer um, e
  por isso a casca so aceita esse modo com a confirmacao explicita do usuario
  (`confirmoAberto`), nunca por um invoke silencioso. "Numero dedicado" aplica
  `whatsapp access restricted` **e** `whatsapp allow <numero pessoal>`: o Garra
  fica num chip dedicado e o usuario fala dele pelo numero pessoal, que precisa
  estar autorizado (senao o portao ficaria com 0 autorizados e descartaria o
  proprio dono em silencio); o numero e validado/normalizado no core
  (`normalizar_numero`, E.164). Nenhum default de seguranca e afrouxado em
  silencio. A persona (nome saneado, estilo, idioma pt-BR por padrao) vira
  `agent.system_prompt` pelo novo `ConfigLoader::set_agent_system_prompt`. O
  `concluido` so e gravado — e o gateway so reinicia — apos todos os passos
  saírem com exit 0.
- **Endurecimento da webview do desktop (#1636, fatia 3).** CSP restritiva no
  `tauri.conf.json` (`default-src 'self'; script-src 'self'; style-src 'self'
  'unsafe-inline'; ...`): todas as telas do desktop passaram a carregar o JS por
  arquivo externo (`onboarding.js`, `settings.js`, `chat-bar.js`, `boot.js`), sem
  script inline. A janela do wizard ganhou uma capability propria e minima
  (`capabilities/onboarding.json`, so IPC + eventos), fora da `default` ampla.
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
