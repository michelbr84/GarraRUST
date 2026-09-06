- **O chat ganha layout de conversa e cabecalho compacto (#934, #935).**
  `voce >` virou `❯` e `garra > ` virou um rotulo `Garra` em linha propria —
  o rotulo na mesma linha da resposta so identificava o primeiro paragrafo.
  A abertura deixou de gastar doze linhas com o mascote mais `Diretorio:` e
  `Projeto: Arquivos: a, b, c...`: agora sao tres linhas com versao, modelo,
  modo, caminho encurtado, ramo do git e tipo de projeto. O mascote nao
  sumiu, virou `garra about`; a inspecao detalhada de diretorio virou
  `/context`; e a listagem de arquivos, que ninguem lia na tela, continua
  indo para o prompt do sistema, onde serve para alguma coisa. O ramo do git
  sai do `.git/HEAD` sem subprocesso, seguindo o ponteiro `gitdir:` de
  worktree e submodulo. Novo modulo `conversation.rs`, puro no molde do
  `spinner` (nao escreve no terminal nem le o relogio), o que torna
  afirmavel o que ninguem exercita a mao: o caminho ASCII, o sem cor e o
  terminal estreito.
