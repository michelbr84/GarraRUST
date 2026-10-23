- **Node privado por caminho explicito para a ponte do WhatsApp (#1636).**
  O modo vinculado agora resolve o `node` por caminho explicito, nesta ordem:
  `GARRAIA_NODE_BIN` (absoluto; invalido e erro, nunca fallback silencioso para
  a PATH), depois o Node privado empacotado ao lado do executavel em execucao
  (layouts fixos por plataforma, relativos ao `current_exe()`), e so entao a
  PATH. O resolver nunca muda o `PATH` do processo nem substitui instalacoes
  existentes: o Node escolhido e invocado pelo caminho absoluto. O gateway e a
  CLI usam o mesmo resolver e registram a origem (`env`/`privado`/`path`).
- **`npm` so e exigido quando ha dependencias a instalar (#1636).** Com a
  arvore pronta (`node_modules` ja no lugar), o pareamento e o atendimento
  sobem sem `npm` nenhum; `npm` so e cobrado quando o plano e instalar, e sua
  ausencia vira um erro claro em vez de uma falha de `spawn` crua.
- **Semeadura da arvore pre-construida da ponte (#1636).** Quando ha um
  diretorio de ponte pre-construida ao lado do Node privado e ele bate com o
  `package-lock.json` embutido no binario, o `node_modules` e copiado
  (atomicamente, sem shell) para o diretorio da ponte em vez de rodar
  `npm ci`; arvore que nao bate e ignorada com aviso, caindo no comportamento
  de sempre. Prepara a fatia de empacotamento (nenhuma primeira execucao
  precisa baixar dependencia).
- **`garra whatsapp self-test [--json]` (#1636).** Resolve o Node e roda
  `bridge.mjs --self-test`, que importa todas as dependencias de execucao
  (Baileys, pino, sharp) sem abrir socket nem parear, e reporta a origem do
  Node, o caminho, a versao e a versao de cada dependencia. Smoke dos
  instaladores em ambiente sem Node global.
