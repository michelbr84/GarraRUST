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
- **`GET /api/modes` passa a listar os customizados**, com a `tool_policy`
  **efetiva** e nao a do modo base. A mensagem de erro do `select` mandava
  conferir uma lista que nunca continha o modo recem-criado.
- **Um lugar so monta o `ExecContext`.** Os dez pontos que atendem usuario
  chamavam `ExecContext::with_mode(state.chosen_agent_mode_for(..).await)` cada
  um por si; agora chamam `state.exec_context_for(..)`, que resolve o modo e, se
  for customizado, ja traz o perfil. E a mesma razao de o `chosen_agent_mode_for`
  existir: foi a repeticao que deixou o `/mode` gravando numa chave e a execucao
  lendo outra por meses.
