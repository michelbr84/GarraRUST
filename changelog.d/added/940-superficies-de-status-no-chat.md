- **`/status` e `/tools` novos, e `/context` legivel (#940).** O `/status` diz
  provider, modelo, ferramentas, sessao e o que o ultimo turno de fato usou —
  sempre o valor **vivo**: o `/model` troca o modelo no meio da sessao, entao
  ler a config diria o que era verdade no boot. O `/tools` responde "quais
  ferramentas o agente tem", que ate aqui nao dava para perguntar: `/tools` era
  apelido de `/tool`, que mostra o que elas **produziram**.
- **As superficies pararam de escrever cor incondicional.** `/help`,
  `/context`, `/tool`, `/history`, `/models` e a despedida usavam
  `println!("{DIM}...{RESET}")`, entao `garra chat | cat` carregava escape.
  Medido no binario: **12 sequencias de escape numa sessao de seis comandos
  redirecionada, agora zero.** E a divida que a ADR 0017 registra, paga onde
  ela mais aparecia.
- **Um painel e uma lista, com a mesma moldura.** Vale a regra do cartao de
  erro: quem sabe **o que** mostrar e o `chat.rs`, quem sabe **como desenhar**
  e a interface. Sendo funcao pura de `(titulo, linhas, estilo, largura)` para
  `String`, "respeita `NO_COLOR`", "cabe no terminal estreito" e "alinha a
  continuacao" viram assercao sobre um valor de retorno, sem terminal e sem
  processo.
- **O `/help` nao pode mais divergir do que existe** — e ele ja divergia: nao
  citava `/models`, e prometia `/provider <nome>` como se trocasse de provider
  (ele responde "reinicie"). Os comandos viraram uma tabela, e um teste confere
  os dois sentidos contra o proprio fonte: comando anunciado que nao existe, e
  comando que existe sem ser anunciado, param no CI.
- **Sem contagem de arquivos no `/context`**, ao contrario do exemplo da issue.
  A propria issue pede para nao varrer o diretorio so para desenhar status, e
  num repositorio grande a varredura custa mais que todo o resto do comando. O
  `/context` tambem deixou de despejar quinze nomes de arquivo no campo
  "Projeto": aquela listagem existe para o prompt do sistema, onde e util ao
  modelo, e nao para quem digitou o comando querendo uma linha.
- **Valor sem espaco maior que a largura estoura, e nao e truncado.** Um
  caminho e um nome de ramo nao tem onde quebrar, e cortar com reticencia
  destruiria justamente o que se copia do painel — quem quebra e o terminal.
