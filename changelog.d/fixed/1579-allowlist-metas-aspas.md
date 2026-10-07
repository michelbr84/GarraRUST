- **`bash`: aspas agora protegem a prosa do argumento na allowlist (#1579).**
  A varredura `META_SHELL` olhava a string crua e tratava parênteses, `;`,
  `|`, `$` etc. **dentro de aspas** como "comando composto" — um `forja-ask`
  com "(transcreva o conteúdo completo)" na mensagem era negado mesmo com o
  prefixo `forja-ask *` liberado pelo operador. Agora a checagem segue a
  tokenização do shell: aspas simples literalizam tudo; aspas duplas mantêm
  ativos só crase, escape e `$` que abre expansão (o `$` de `"R$ 50"` é
  literal); aspas não fechadas continuam negando. Composto real (`;`, `&&`,
  pipe, `$(...)`, redireção, newline) segue nunca casando.
