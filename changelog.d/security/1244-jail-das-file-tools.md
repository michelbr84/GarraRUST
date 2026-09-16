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
  A recusa devolve **uma unica frase** ao modelo, sem o caminho, sem a raiz e
  sem distinguir "nao existe" de "existe mas esta fora" — as tres recusas sao
  byte-identicas, para a tool nao virar oraculo de existencia de arquivo. A
  mensagem util da #923 (que diz onde procurou e por que ali) fica preservada
  para o arquivo ausente **dentro** da raiz, onde ela nao vaza nada.
  O construtor das tres tools passou a exigir o jail: `FileReadTool::new(None)`
  nao compila mais. Um jail que se pode esquecer de passar e um jail que se
  esquece de passar — foi exatamente o que aconteceu. E os testes de regressao
  nao chamam o construtor: eles pedem a tool ao runtime que
  `build_agent_runtime` montou, que e o mesmo objeto que o turno do agente usa.
  `garra config check` avisa quando `agent.file_roots` inclui `/` ou o proprio
  `$HOME`, que devolvem `~/.ssh` e `.env` ao alcance do modelo.
  Residual conhecido, registrado e nao reivindicado como resolvido: a checagem
  resolve o caminho e quem abre o arquivo e a tool, num segundo passo. Entre um
  e outro, quem tiver escrita dentro da raiz pode trocar um componente por
  symlink para fora (TOCTOU). Fechar isso exige abrir por descritor
  (`openat2` com `RESOLVE_BENEATH` no Linux) e nao tem equivalente portatil nos
  tres sistemas operacionais que o projeto suporta.
