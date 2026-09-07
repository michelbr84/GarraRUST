- **Modo customizado passa a valer na execucao (#986).** O CRUD existe desde o
  GAR-232 — `POST/GET/PATCH /api/modes/custom`, com UI no WebChat — e o runtime
  nunca leu nada dele: dava para criar um modo, tentar seleciona-lo, tomar 400 do
  `POST /api/mode/select` (que so aceitava os nove nativos), e, se conseguisse
  gravar por outro caminho, ver a execucao ignorar tudo. Agora o `base_mode`, o
  `prompt_override`, os `tool_policy_overrides` e os `defaults` formam um perfil
  efetivo que chega ao portao de ferramentas.
- **As duas grafias do override de politica sao aceitas.** O `README.pt-BR.md`
  documenta `{"allow": [...], "deny": [...]}` e a struct chama os campos
  `allowed`/`denied`. Quem seguiu a documentacao publicada nao pode ver o
  override ser ignorado em silencio. Chave ausente preserva a do perfil base;
  presente substitui — nao ha merge de listas, porque quem declara `allow` esta
  dizendo qual e a lista, e nao acrescentando a ela.
- **O `prompt_override` e o `defaults` chegam ao modelo.** Achado de auditoria:
  na primeira versao os dois eram gravados, devolvidos pela API e **nunca lidos
  na execucao** — o perfil os carregava e nada extraia dali. O criterio de aceite
  da issue ("overrides de politica, prompt e defaults sao respeitados") nao
  estava cumprido, e o teste que eu tinha escrito verificava so o struct, entao
  passava com a feature morta. Precedencia: override explicito do chamador >
  valor do modo > config do runtime > default. Diferente dos `ModeLimits`, o
  `max_tokens` do modo **nao** e limitado ao padrao: aquele limita quantas vezes
  o agente roda ferramenta (cada uma podendo rodar `bash`) e o tempo de parede
  junto; este limita o tamanho de uma resposta, e o README documenta `8192` como
  uso pretendido. Quem configurou `max_tokens` no runtime continua vencendo.
- **Nome de modo nativo e recusado na criacao.** `select_mode` tenta o nativo
  primeiro — e tem de tentar, para um customizado chamado `code` nao sequestrar
  o nativo. A consequencia era que um modo criado com nome nativo ficava gravado
  e nunca selecionavel. Aceitar a criacao e negar a selecao depois e o pior dos
  dois.
- **`GET /api/modes` passa a listar os customizados**, com a `tool_policy`
  **efetiva** e nao a do modo base. A mensagem de erro do `select` mandava
  conferir uma lista que nunca continha o modo recem-criado.
- **Um lugar so monta o `ExecContext`.** Os dez pontos que atendem usuario
  chamavam `ExecContext::with_mode(state.chosen_agent_mode_for(..).await)` cada
  um por si; agora chamam `state.exec_context_for(..)`, que resolve o modo e, se
  for customizado, ja traz o perfil. E a mesma razao de o `chosen_agent_mode_for`
  existir: foi a repeticao que deixou o `/mode` gravando numa chave e a execucao
  lendo outra por meses.
