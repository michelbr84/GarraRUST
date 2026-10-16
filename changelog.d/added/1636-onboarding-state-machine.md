- **Maquina de estados do onboarding do desktop (#1636).** Novo modulo
  `onboarding` em `garraia-desktop-core` (sem Tauri, dentro dos gates de CI):
  as telas do Assistente de Primeiro Uso — `BoasVindas → ModoWhatsApp → Qr →
  Personalidade → Pronto` — como estado puro, sem relogio nem I/O, no padrao do
  `state.rs`. A tela do QR mapeia os eventos NDJSON do `garra whatsapp link
  --machine` (fatia 1) via o tipo `EventoNdjson` (serde, tolerante a eventos
  desconhecidos). `Avancar` nao pula o vinculo, `Falhou` permite tentar de novo
  e nenhum estado terminal avanca sozinho. Inclui o tipo `OnboardingPersistido`
  (round-trip para `onboarding.json`). So logica e testes; a janela e a fiacao
  da webview ficam para a fatia 3. Refs #1636
