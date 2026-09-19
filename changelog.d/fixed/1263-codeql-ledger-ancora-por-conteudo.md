- **O ledger CodeQL passou a ancorar por conteudo, nao por numero de linha
  (#1263).** `check-ledger-anchors.py` procura o `sink_snippet` no arquivo e
  DERIVA a linha, em vez de le-la do JSON e comparar. Antes, qualquer commit
  que acrescentasse ou removesse linhas ACIMA de um sink derrubava o gate sem
  encostar no statement suprimido — a entrada #113 andou duas vezes no mesmo
  PR (#1252), `:769` para `:826` para `:867`, sem o snippet mudar um byte, e
  cada rodada custou uma fila de ~110 jobs. Pior: o caminho mais rapido para
  ficar verde era editar o `sink_snippet` ate casar com a linha, o que e
  fraudar o registro de auditoria. Agora o gate fica vermelho em um caso so: o
  statement mudou, sumiu, ou ficou ambiguo. `line` virou campo derivado e o
  proprio checker o reescreve no `.json` e na coluna `File:line` do `.md`,
  dizendo `linha derivada X (antes Y)`; no CI ele roda com `--no-rewrite` e so
  reporta. Snippet com 2+ ocorrencias no arquivo agora e erro explicito de
  configuracao, resolvido por uma ancora auxiliar de conteudo
  (`"disambiguator": {"kind": "function", "value": "<fn envolvente>"}`) e nunca
  por ordinal, que reaponta em silencio quando aparece um statement identico
  acima — 13 das 30 entradas precisaram disso. De brinde, as citacoes
  `arquivo:linha` dentro das justificativas passaram a ser conferidas: era por
  falta disso que um drift sobrevivia escondido num documento de auditoria de
  seguranca (a justificativa do #113 citava `:630` e `:643` quando o codigo
  estava em `:824` e `:837`).
