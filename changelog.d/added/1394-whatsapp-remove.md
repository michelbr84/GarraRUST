- **`garra whatsapp remove <numero>` revoga o acesso pela CLI (#1394).** Ate aqui
  autorizar tinha comando e revogar so existia editando o `config.yml`. O `remove`
  e o espelho do `allow`: tira a identidade de `allow` e de `owners` pela mesma
  chave que o portao do gateway compara (o celular com e sem o nono digito e o
  mesmo), preserva as outras chaves da secao e **nao** desliga o canal. Remover um
  DONO exige confirmacao explicita — `--yes`/`-y` fora do terminal, pergunta com
  default NAO dentro dele —, e quem nao estava na lista sai 0, porque revogar e
  idempotente.
