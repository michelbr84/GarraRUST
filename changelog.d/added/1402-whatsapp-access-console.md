- **Web Console — pagina "WhatsApp Access" (#1402, #1403, #1404, #1405,
  #1406, #1407, #1408, #1411, #1413; ADR 0025).** Terceiro consumidor do
  caminho unico: le `GET /admin/api/whatsapp/access` (o mesmo documento da
  CLI) e muda por `POST /admin/api/whatsapp/access`. Resumo (canal, perfil
  de execucao, piso do dono, hot reload, contagens, avisos), admissao
  `restricted` / `Anyone (open)`, default do desconhecido, grupos (liga/
  desliga, default, politica por JID), formulario "Add phone / identity"
  com nivel e write, matriz por principal (piso, nivel, write, o que pode
  de fato) com seletor `chat|read|full`, toggle de write, Block/Unblock,
  Make owner/Demote e Remove, botao "Reset to safe defaults" e a trilha de
  audit. **Toda mutacao passa por um preview** (`dry_run`: mudancas + o que
  cada principal ganha e perde) antes de confirmar; elevacoes perigosas
  (`open`, owner, reset) trazem aviso. A pagina so conhece `...1234`: age
  sobre uma linha mandando `identity_last4`, que o gateway resolve entre
  as identidades declaradas (ambiguo = 409); a identidade inteira nunca
  desce ao navegador. `data-testid` estaveis + spec Playwright
  `tests/playwright/whatsapp-access.spec.ts`. Motor: `Mutacao::Papel`
  (owner/unowner, preservando o acesso ao rebaixar o dono legado) e
  `Mutacao::Remover` (tira de `allow`, `owners` e `access.users`), tambem
  expostos na API (`owner`, `unowner`, `remove`).
