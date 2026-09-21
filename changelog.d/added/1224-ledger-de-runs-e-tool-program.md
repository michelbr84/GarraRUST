- **Runs de sub-agente deixam rastro auditavel, e um restart nao os perde em
  silencio (#1224).** Os sub-agentes do `AgentCoordinator` morriam com o
  processo e nao deixavam nada para tras — mesma limitacao do `delegate_task`
  do Hermes. Quem derrubasse o gateway no meio de uma delegacao ficava sem
  saber que ela existiu, muito menos que ficou pela metade.
  Agora existe a tabela `agent_runs` e o `SessionStore` sabe abrir, fechar e
  listar run: `start_agent_run` e idempotente e nao reabre run que ja terminou,
  e `mark_interrupted_runs()` converte todo run que ficou `running` em
  `interrupted`, devolvendo a lista para o operador. O `AgentCoordinator` fala
  com isso por um `RunLedger`, cujo default e um `NoopLedger` de custo zero —
  quem nao injeta o adapter nao paga nada.
  O que **nao** mudou, e vale dizer com todas as letras: o run continua sem
  sobreviver ao processo, e nesta fatia `AgentCoordinator::spawn_agent` segue
  sem chamador de producao — a fatia original entregou CLI e gateway sem
  gravar run nenhum. O primeiro escritor real do ledger (scheduler de
  heartbeats gravando cada execucao e a subida do gateway/CLI marcando runs
  interrompidos) entra pela #1227. O que existe aqui e o
  schema + a auditoria funcionando, prontos para o chamador. Retomar um run
  interrompido, e ligar o ledger a um caminho de producao, e a #1227.
  Os snippets gravados sao truncados em 500 caracteres de proposito: o ledger e
  auditoria, nao armazenamento de conversa.

- **`ToolRegistry::execute_program` — prototipo do Code Mode em `garraia-tools`
  (#1224).** Um pipeline de N tools custava N inferencias, porque o modelo
  voltava ao loop entre cada passo. A funcao recebe um programa JSON
  (`steps: [{tool, args, as}]`) e executa os passos em sequencia, sem voltar
  ao modelo entre eles.
  A substituicao de variaveis e deliberadamente tudo-ou-nada: um arg **e** a
  variavel ou nao e, sem interpolacao dentro de string maior — nada de injecao
  de prefixo. Falha em qualquer passo encerra o programa com o indice do passo,
  e o orcamento de `max_steps` (16 por default) impede que um programa vire
  loop.
  O que ela **nao** e, com todas as letras: nao e o runtime executando nada.
  Ela vive em `garraia-tools`, crate da qual o `AgentRuntime` (em
  `garraia-agents`) nao depende; nao consulta o `ToolGate` dos modos; e nao
  tem nenhum chamador fora dos proprios testes da crate. A `tool_program`
  intrinseca do `AgentRuntime`, com gate por passo, e a #1226 (S-B) — e e la
  que o caminho se torna alcancavel. Ate entao a funcao fica marcada
  `#[deprecated]` (#1226 S-E), para que ninguem a ligue ao loop por fora do
  gate.
