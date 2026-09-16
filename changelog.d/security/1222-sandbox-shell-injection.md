- **O sandbox por tool passa a conter o comando de verdade — metacaractere de
  shell nao escapa mais para o host (correcao da #1222).** A primeira versao do
  `SandboxPolicy::wrap_command` montava a linha do `docker run` citando o
  comando com `{:?}`, o `Debug` do Rust. `escape_debug` parece quoting mas nao
  e: ele escapa `"`, `\` e caracteres de controle, e nao toca em `$` nem em
  crase. Como o `BashTool` entrega a linha inteira ao shell do host
  (`sh -c <linha>`), um comando contendo `$(...)` ou crase era expandido pelo
  **host**, antes de o `docker run` sequer existir. O container recebia apenas
  o resultado da expansao.
  O efeito e o oposto do proposito da funcionalidade: `--network none`,
  `--security-opt no-new-privileges` e o mount do workdir seguiam todos
  presentes na linha de comando e todos irrelevantes, porque o comando do
  atacante nunca chegava a entrar no container. E a entrada e alcancavel: o
  comando vem de tool call do LLM, que o proprio projeto ja trata como
  influenciavel por injecao indireta de prompt (#1213). Fail-closed quando o
  backend falta, fail-open no conteudo do comando — a combinacao pior, porque
  o operador liga `mode: all` e passa a confiar numa contencao que nao existe.
  O mesmo valia para o `cwd`, interpolado sem aspas em `-v {cwd}:{cwd}`: um
  diretorio com `;` virava comando extra no host, e um com espaco simplesmente
  quebrava o `docker run`.
  Agora `command`, `cwd`, `image` e o host do `ssh` passam por `sh_quote`,
  quoting POSIX com aspas simples (`'\''` para a aspa interna) — dentro de
  aspas simples o shell nao expande nada. O ramo `ssh` leva **duas** camadas,
  porque o `ssh` nao entrega argv ao host remoto: ele remonta uma string que o
  shell remoto reparseia, entao uma camada morre em cada lado.
  A lacuna real era de teste: os onze testes existentes afirmavam que as flags
  de hardening estavam presentes na string, e nenhum afirmava que um
  metacaractere ficava **inerte**. Os novos testes executam a string por um
  shell de verdade e conferem que o comando chega como uma unica palavra
  literal — inclusive um que falha de proposito se alguem "simplificar" o
  quoting duplo do `ssh`.
  De quebra, `fail_closed_backend_ausente` deixou de depender de haver cliente
  `ssh` instalado: ele fazia `.unwrap()` no retorno e quebrava em qualquer
  maquina sem ssh; agora asserta explicitamente os dois desfechos possiveis,
  em vez de pular em silencio.
