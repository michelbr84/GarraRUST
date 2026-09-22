- **A chave de um provider so vai para o endpoint da propria entrada `llm:` (#1370).**
  `garraia ask -p openai` (e o `garra_ask`/`garra_agent` do MCP com
  `provider=openai`) lia `llm.openai.api_key` e descartava `llm.openai.base_url`:
  a chave de um endpoint OpenAI-compativel proprio era enviada para
  `https://api.openai.com`. O smoke de instalacao limpa da v0.4.4 pegou isso, e a
  auditoria dos outros pontos de construcao da CLI achou a mesma classe de
  defeito em mais quatro lugares: `-p anthropic` e `-p openrouter` tambem
  ignoravam a `base_url`; a autodeteccao (sem `agent.default_provider`) fazia o
  mesmo com os tres; o caminho do `agent.default_provider` lia a `base_url` da
  entrada padrao mas a chave de `llm.<tipo>` (com `default_provider: lmstudio` e
  um `llm.openai` ao lado, a chave do `llm.openai` ia para o LM Studio) e largava
  a `base_url` de um default `anthropic`; e o `--url` avulso mandava
  `OPENAI_API_KEY` ou `GARRAIA_EMBEDDING_API_KEY` para o endereco digitado.
  Agora endpoint e credencial saem sempre da mesma entrada
  (`crates/garraia-cli/src/provider_binding.rs`): dentro dela o `api_key` vence a
  variavel de ambiente do tipo; sem entrada, o endpoint e o padrao do tipo e a
  chave so vem do ambiente, nunca de outra entrada; o `--url` usa `LLM_API_KEY`
  ou a chave da entrada com a mesma `base_url`. **A variavel de ambiente do tipo
  (`OPENAI_API_KEY`, `ANTHROPIC_API_KEY`, `OPENROUTER_API_KEY`, inclusive a de um
  `.env` do diretorio corrente) so vai para o host padrao do tipo:** ela preenche
  uma entrada sem `api_key` apenas quando a entrada nao tem `base_url` ou tem o
  proprio host padrao (o `https://openrouter.ai/api/v1` que o `garraia init`
  grava continua funcionando com a chave na env). **Mudanca de comportamento:**
  uma entrada com `base_url` propria e sem `api_key` deixa de receber a variavel
  do ambiente, inclusive pelo `agent.default_provider` (onde ela ja era enviada
  para a `base_url` antes desta versao); o OpenAI-compativel manda o marcador
  `not-needed`, e `anthropic`/`openrouter` recusam com erro. Quem dependia disso
  poe o `api_key` na propria entrada. `-p <alias>` (uma entrada `llm:` com outro
  nome, ja aceita pela policy do MCP) passa a funcionar em vez de falhar com
  "Provider desconhecido", e o `garra_agent` resolve o provider pelo id com que
  ele foi registrado, entao um alias de `anthropic`/`ollama`/`llamacpp` (e o
  proprio `llamacpp`) nao falha mais com "provider '<nome>' not found". Na
  autodeteccao, um `llm.openrouter` que declara `provider: openai` com chave
  propria volta a ser escolhido, vinculado inteiro (a chave dele vai para a
  `base_url` dele); sem chave propria, o candidato cai para a
  `OPENROUTER_API_KEY` do ambiente no host padrao, em vez de sumir.
