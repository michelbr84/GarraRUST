- **`garra whatsapp owner` / `unowner` administram o papel de dono (#1395).** Ate aqui
  virar dono so acontecia junto com o `allow --owner` (ou dentro do `link`), e deixar
  de ser dono so existia editando o `config.yml` a mao — ou passando o `remove`, que
  tira o acesso junto. O `owner <numero>` grava em `owners` pela mesma escrita do
  `allow --owner`, com as mesmas duas portas: `execution.profile = isolated-pod`
  obrigatorio (64 fora dele) e confirmacao explicita, `--yes`/`-y` fora do terminal.
  O `unowner <numero>` tira o papel **sem nunca tirar o acesso**: quando a identidade
  so existia em `owners`, a entrada passa para `allow` na mesma escrita, entao nao ha
  instante no disco em que a pessoa fique fora das duas listas. Rebaixar funciona em
  qualquer perfil, de proposito — e em `standard` que um dono esquecido e um
  privilegio latente —, e a confirmacao vale para o **ultimo** dono, o unico
  rebaixamento que deixa a configuracao sem dono nenhum. Os dois comandos sao
  idempotentes, so imprimem os quatro ultimos digitos de cada identidade e aparecem
  no `whatsapp users` e no `status` na mensagem seguinte, sem reiniciar o gateway.
