- **`garraia whatsapp access`, `level`, `write`, `block`, `unblock`: a Access
  Policy v2 pela CLI (#1396, #1397, #1398, #1399, #1400, #1401, #1413,
  #1414; ADR 0025).** `access` imprime a politica efetiva inteira (admissao,
  default do desconhecido, grupos, e cada principal com piso, nivel e o que
  pode de fato — calculado pelo MESMO `ToolGate` do turno, nunca por um
  parser paralelo), com `--json` e, so localmente, `--reveal`. `access
  open|restricted` troca a admissao (`open` avisa e pede confirmacao ou
  `--yes`); `access default chat|read [--write]` define o que um desconhecido
  recebe em `open` (`full` e recusado); `level <numero> chat|read|full` e
  `write <numero> on|off` gravam nivel e escrita por identidade (`write`
  mexe so em escrita de arquivo, nativa e MCP); `block`/`unblock`; `access
  groups on|off|default <nivel>` e `access group <jid> <nivel>`; `access
  reset` volta ao seguro preservando donos e bloqueios (confirmacao ou
  `--yes`, idempotente). Toda mutacao aceita `--dry-run`: mostra o que
  mudaria e o impacto por principal (o que ganha e perde: escrita, shell,
  dispositivo, mensagem, MCP) sem gravar nem auditar. Toda mutacao aplicada
  vai para `<data_dir>/audit/whatsapp-access.jsonl` (`0600`, rotacao por
  tamanho, `audit_max_bytes` na secao): quando, quem, por onde, acao, alvo
  `...1234` e o resumo antes/depois — nunca identidade inteira, chave ou
  mensagem; `access audit [--json] [--limit N]` le a trilha. Um unico caminho
  de mutacao no gateway (`whatsapp_linked_politica::mutacao`), que a API
  admin e o Web Console reutilizam. Exit codes: 0 · 1 cancelado · 64 uso ·
  65 dado invalido · 70 config · 73 gravou mas o audit falhou.
