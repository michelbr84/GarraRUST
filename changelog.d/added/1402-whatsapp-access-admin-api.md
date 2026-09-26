- **API admin da Access Policy v2 (#1402, #1412, #1413, #1414; ADR 0025):**
  `GET /admin/api/whatsapp/access` devolve a politica efetiva — o MESMO
  documento de `garraia whatsapp access --json`, produzido por
  `whatsapp_linked_politica::visao` — mais `hot_reload` (se o gateway rele
  a config a quente); `POST /admin/api/whatsapp/access` aplica uma mutacao
  (`action`: open | restricted | default | level | write | block | unblock |
  groups | group-default | group | reset, com `identity`/`jid`/`level`/
  `write`/`enabled` e `dry_run`) pelo mesmo `mutacao::aplicar` da CLI —
  validacao antes de gravar (400 com a mensagem acionavel), escrita atomica
  `0600`, impacto por principal (o que ganha e perde) e audit com `origem:
  admin_api` e o username do admin; `GET /admin/api/whatsapp/access/audit`
  le a trilha. Leitura com `Channels/Read` (viewer le), mutacao com
  `Channels/Update` (viewer recebe 403); cookie e CSRF dos layers do
  `/admin`. A API nunca revela identidade (so `...1234`). O check
  `whatsapp.access` do `/api/diagnostics` passa a avisar `access.admission:
  open` (com o passo para fechar) e secao `access` com valor invalido.
