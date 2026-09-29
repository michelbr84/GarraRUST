- **Perfis `llamacpp` e `anthropic` voltam a resolver por chave, e o fallback por
  tipo volta a funcionar (#1554).** O #1540 deu id por perfil aos arms `openai` e
  `ollama` do bootstrap e deixou `anthropic` e `llamacpp` para tras. O `llamacpp`
  tinha um agravante proprio: o id que ele registra (`llama-cpp`, com hifen)
  **nao** e igual ao `provider:` que o config usa para escolher o arm
  (`llamacpp`), entao um perfil unico ja bastava para quebrar — nao resolvia nem
  pela chave nem pelo fallback por tipo, e `agent.default_provider` apontando
  para ele era silenciosamente ignorado (so um WARN no log) enquanto o gateway
  seguia atendendo por outro provider, com a config aparentemente correta. Com
  dois perfis do mesmo tipo, os dois arms repetiam a colisao de slot do #1540.
  Agora `AnthropicProvider` e `LlamaCppProvider` tem `with_name` e os dois arms
  registram pela chave do perfil, e `resolve_registered_provider_id` conhece a
  traducao tipo -> id canonico a partir de uma constante publica, em vez de um
  literal repetido.
- **`agent.default_provider` que nomeia o *tipo* volta a resolver quando ha um
  unico perfil daquele tipo (#1554).** Isto e uma mudanca de comportamento em
  relacao ao #1540, que transformou esse caso num WARN: ao registrar todo perfil
  pela chave, o #1540 quebrou sem perceber quem tinha `default_provider: ollama`
  com `llm.local.provider: ollama`, porque o registro passou a ser `local`. A
  correcao do #1554 teria repetido o estrago com `anthropic` e `llama-cpp`.
  Agora o nome pedido e procurado em tres lugares: id registrado, chave de
  perfil e — novo — tipo cujo perfil foi registrado pela chave. O terceiro passo
  exige **exatamente um** candidato: com dois perfis do mesmo tipo o nome e
  genuinamente ambiguo e a recusa com WARN continua, porque escolher um seria
  reintroduzir o sorteio por boot que o #1540 existiu para matar.
