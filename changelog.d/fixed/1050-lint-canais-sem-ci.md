- canais: `google_chat` deixa de escrever um `Default` que o clippy pede
  derivado, e `matrix` colapsa dois `if` aninhados. Os tres erros existiam
  desde sempre e nao apareciam porque nenhum job do CI compilava essas
  features — o step novo do `ci.yml` passa a compilar e testar as cinco que
  faltavam (`google_chat`, `teams`, `matrix`, `irc`, `voice`). (#1050)
