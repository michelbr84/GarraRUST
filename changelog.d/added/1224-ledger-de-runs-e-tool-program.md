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
  sobreviver ao processo, e nesta fatia nem `mark_interrupted_runs()` roda na
  subida nem `AgentCoordinator::spawn_agent` tem chamador de producao — CLI e
  gateway ainda nao gravam run nenhum, entao `agent_runs` fica vazia ate a
  #1227 decidir onde plugar o escritor real. O que existe aqui e o
  schema + a auditoria funcionando, prontos para o chamador. Retomar um run
  interrompido, e ligar o ledger a um caminho de producao, e a #1227.
  Os snippets gravados sao truncados em 500 caracteres de proposito: o ledger e
  auditoria, nao armazenamento de conversa.

- **`ToolRegistry::execute_program` — primeiro passo do Code Mode (#1224).** Um
  pipeline de N tools custava N inferencias, porque o modelo voltava ao loop
  entre cada passo. Agora ele pode escrever um programa JSON
  (`steps: [{tool, args, as}]`) que o runtime executa inteiro num turno so.
  A substituicao de variaveis e deliberadamente tudo-ou-nada: um arg **e** a
  variavel ou nao e, sem interpolacao dentro de string maior — nada de injecao
  de prefixo. Falha em qualquer passo encerra o programa com o indice do passo,
  e o orcamento de `max_steps` (16 por default) impede que um programa vire
  loop.
  Nesta fatia a funcao ainda nao tem chamador: o wrapper no runtime e a
  exposicao no schema de tools do CLI e do gateway sao a #1226 — que precisa
  fazer cada passo passar pelo mesmo `ToolGate` do loop normal antes de tornar
  o caminho alcancavel.
