- **Instalacao limpa nao nasce com aviso espurio no `/api/diagnostics`
  (#1471).** `tools.bash` com `agent.sandbox.mode = off` — o default
  documentado do perfil `standard` — passa de `warning` a `not_configured`,
  com o mesmo passo de como ligar o sandbox (fora de unix, idem); sandbox
  configurado e inutilizavel (sem binario, sem backend, ssh, tool elevada ou
  fora da allowlist) continua `warning`. `runtime.channels` sem canal de
  mensageria passa de `warning` a `not_configured`: chat web, CLI e API nao
  entram no registry de canais e o WhatsApp vinculado tem a linha
  `whatsapp.linked`; o passo antigo mandava procurar erro de registro de um
  canal `web` que nunca existiu ali, e o novo diz como declarar um canal.
