- **Dois perfis `llm:` do mesmo tipo param de disputar um slot de provider (#1540).**
  Os arms `openai` e `ollama` do bootstrap registravam todo perfil com o id do
  **tipo**, entao duas entradas `provider: openai` (um llama-server local e um
  Ollama OpenAI-compat, p. ex.) colidiam em `"openai"`: `get_provider` so
  achava a primeira e a ordem de registro vinha da iteracao do `HashMap` de
  config — o provider efetivo, e o default, eram sorteados a cada boot. Em
  0.4.6 a mesma config alternava o `/api/health.model` entre dois modelos, e no
  boot em que o Ollama ganhava toda conversa com tools quebrava. Agora cada
  perfil e registrado pela **chave** (`with_name`), como os arms de nuvem ja
  faziam, e `agent.default_provider` / `fallback_providers` / `model=<chave>/...`
  passam a enderecar a instancia certa. Perfil cuja chave e o proprio tipo
  (`openrouter`) nao muda; um `agent.default_provider` que nomeia o *tipo* sem
  existir como chave no mapa `llm:` agora cai num WARN explicito em vez de
  resolver para um perfil aleatorio.
