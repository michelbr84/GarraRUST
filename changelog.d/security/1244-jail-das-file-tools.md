- **As file tools do agente passam a ter jail de diretorio — `~` e caminho
  absoluto nao resolvem mais em qualquer lugar do disco (correcao da #1244).**
  `file_read`, `file_write` e `list_dir` recebiam o caminho cru do modelo,
  expandiam `~` e aceitavam caminho absoluto sem confinamento nenhum. O
  parametro `allowed_directories` existia e tinha teste — e os dois pontos de
  registro em producao (`bootstrap/mod.rs` do gateway e `chat.rs` da CLI)
  passavam `None`. Era a quinta instancia do mesmo defeito neste repositorio:
  funcao de seguranca bem testada cujo ponto de chamada em producao nenhum
  teste exercitava.
  A consequencia era direta: um prompt chegando por Telegram, Discord ou
  WhatsApp mandava o modelo ler `~/.ssh/id_rsa`, `/etc/shadow` ou o proprio
  `config.yml` do gateway — que carrega chave de LLM em claro quando o
  operador nao usa o cofre. E `list_dir` era o reconhecimento: com ela o
  modelo achava o alvo antes de pedir a leitura.
  Agora existe `garraia_agents::FileJail`. As raizes efetivas de uma chamada
  sao a uniao de `agent.file_roots` (config, vazio por padrao, mais a env
  `GARRAIA_FILE_ROOTS`) com o `working_dir` da sessao. Conjunto vazio significa
  **negar tudo**, nao "tudo liberado": sem raiz conhecida nao ha como afirmar
  que um caminho e seguro. No gateway isso faz a raiz default ser o diretorio
  da sessao e nada mais — e o `working_dir` de uma sessao ja passa por
  `project_root::confine` antes de ser gravado. Na CLI o CWD do processo entra
  como raiz, porque quem roda `garra chat` e o dono da maquina no diretorio que
  escolheu.
  O confinamento canonicaliza **antes** de comparar, e compara componente a
  componente. Isso cobre os quatro vetores: `..`, symlink que aponta para fora
  da raiz, `~` que expande para fora dela e caminho absoluto. Para escrita, em
  que o alvo ainda nao existe e `canonicalize` falharia, o jail sobe ate o
  ancestral existente mais proximo e canonicaliza ele — e o que barra
  `raiz/link-para-fora/novo.txt`, que uma checagem so do `parent` textual
  deixaria passar.
  Um quinto vetor entrou depois da auditoria de seguranca, e e o contraintuitivo:
  `canonicalize` falhar nao quer dizer "nao existe", quer dizer "nao resolve".
  Um symlink **pendurado** — cujo alvo nao existe — falha no `canonicalize` e
  existe para o `lstat`, e o `open(O_CREAT)` de uma escrita segue o link e cria
  o arquivo no alvo, fora da raiz. Bastava um repositorio clonado trazer
  `raiz/evil -> ../../../home/u/.ssh/authorized_keys` versionado no git. Agora
  cada componente que nao canonicaliza passa por `symlink_metadata`: existir
  para o `lstat` sem resolver e recusa. O caminho e normalizado por
  `components()` antes desse `lstat`, porque com barra final (`raiz/evil/`) o
  `lstat` segue o link por POSIX e o pendurado voltaria a parecer inexistente.
  Um pendurado apontando para dentro da raiz tambem passou a ser recusado —
  fail-closed assumido, para nao reimplementar resolucao de symlink a mao.
  A recusa devolve **uma unica frase** ao modelo, sem o caminho, sem a raiz e
  sem distinguir "nao existe" de "existe mas esta fora" — as tres recusas sao
  byte-identicas, para a tool nao virar oraculo de existencia de arquivo. A
  mensagem util da #923 (que diz onde procurou e por que ali) fica preservada
  para o arquivo ausente **dentro** da raiz, onde ela nao vaza nada.
  O construtor das tres tools passou a exigir o jail: `FileReadTool::new(None)`
  nao compila mais. Um jail que se pode esquecer de passar e um jail que se
  esquece de passar — foi exatamente o que aconteceu. E os testes de regressao
  do gateway nao chamam o construtor: eles pedem a tool ao runtime que
  `build_agent_runtime` montou, que e o mesmo objeto que o turno do agente usa.
  No caminho MCP (`garra_agent`) o `working_dir` passou a ser confinado antes
  de ser aceito. Ele vira raiz das file tools dentro do `FileJail`, e ali quem
  o escreve e o MODELO, pelo argumento da tool: sem a checagem,
  `{"working_dir": "/"}` devolvia o disco inteiro as file tools.
  `handle_agent_call` agora responde `invalid_params` para um `working_dir`
  fora das raizes do operador — ele pode estreitar o jail ou ficar dentro,
  nunca alargar. A doc do schema ainda dizia "validated for existence only —
  not against allowed_dirs", frase escrita quando o `working_dir` nao era raiz
  e que depois instruia o modelo a usar exatamente o buraco; foi corrigida no
  schema, no campo da struct e — a copia de maior alavancagem — no system
  prompt que o modelo le a cada turno, onde ela sobrevivera a primeira
  correcao. Um modelo que leia "o campo e validado apenas quanto a existencia"
  trata a recusa do jail como erro de caminho e roteia pelo `bash`, que de fato
  passa por fora; e o mesmo prompt manda "nunca contornar silenciosamente um
  bloqueio de seguranca". O jail tambem passou a ser construido **uma vez por
  chamada**, em `handle_agent_call`, e desce por parametro ate `build_tools`:
  a instancia que valida o `working_dir` e a mesma que vai para as file tools,
  entao nao ha duas reguas para manter em concordancia.
  `garra config check` avisa quando `agent.file_roots` inclui `/` ou o proprio
  `$HOME`, que devolvem `~/.ssh` e `.env` ao alcance do modelo — e avisa
  tambem sobre a env `GARRAIA_FILE_ROOTS`, que soma raizes as da config e antes
  nao passava por validacao nenhuma (`GARRAIA_FILE_ROOTS=/` desligava o jail em
  silencio). A comparacao acontece depois do `canonicalize`, senao
  `$HOME/../$USER` passa batido. No boot, o gateway nomeia as raizes em vez de
  so conta-las e emite um `warn!` por raiz que, ja resolvida, seja `/` ou o
  `$HOME`: um jail apertado cria pressao operacional exatamente na direcao da
  unica configuracao que o desliga, e essa saida nao pode ser a silenciosa.
  Residual conhecido, registrado e nao reivindicado como resolvido: a checagem
  resolve o caminho e quem abre o arquivo e a tool, num segundo passo. Entre um
  e outro, quem tiver escrita dentro da raiz pode trocar um componente por
  symlink para fora (TOCTOU). Fechar isso exige abrir por descritor
  (`openat2` com `RESOLVE_BENEATH` no Linux) e nao tem equivalente portatil nos
  tres sistemas operacionais que o projeto suporta.
  Segundo residual, e este merece ser dito sem rodeio: **contra symlink a
  defesa e o `canonicalize`; contra hardlink nao existe defesa com esta API**.
  Um hardlink dentro da raiz apontando para o inode de um arquivo de fora nao e
  um ponteiro que se resolve, e um segundo nome do mesmo inode — o
  `canonicalize` nao tem o que seguir, o `starts_with` aprova, e a escrita cai
  no inode de fora. No `file_write` o backup `.bak` ainda copia o conteudo de
  fora para dentro da raiz, e o escape de escrita vira tambem escape de
  leitura. O impacto pratico e menor que o do symlink pendurado: o git nao
  versiona hardlink, entao o vetor "repositorio clonado" nao serve, e o ataque
  exige quem ja tenha escrita dentro da raiz — a mesma pre-condicao do TOCTOU.
  Fica declarado na §5.72 do threat model em vez de omitido.
