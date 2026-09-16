- **`memory.auto_extract` e `memory.max_facts` — o auto-learning de fatos
  finalmente tem knob (#1221).** Com a memoria ligada, o `AgentRuntime`
  disparava `MemoryExtractor::extract_facts` em **todo** turno do usuario: uma
  chamada LLM extra, incondicional, sem nenhuma forma de desligar so ela. Quem
  usa provider pago ou com rate limit pagava a extracao em cada mensagem e a
  unica saida era desligar a memoria semantica inteira — jogar fora a busca
  para economizar a escrita. As duas chaves ja eram prometidas por docs
  antigas; agora existem de verdade.
  `memory.auto_extract` (default `true`) desliga apenas a chamada de extracao,
  preservando a memoria semantica. `memory.max_facts` (default sem teto) limita
  quantos fatos um unico turno pode gravar, mantendo os de maior confidence —
  ordenacao estavel, entao empate resolve pela ordem de chegada. Os dois
  defaults reproduzem exatamente o comportamento historico: quem nao mexer no
  `config.yml` nao percebe diferenca.
  A validacao que vivia inline no loop (confidence minima de 0.80, key e value
  nao vazios) saiu para `select_learned_facts`, uma funcao pura — era a unica
  forma de afirmar o teto e o desempate em teste sem subir um provider.
  `extraction_interval`, a terceira chave que as docs prometiam, ficou
  deliberadamente de fora: e a de menor ganho das tres e exigiria um contador
  de turnos por sessao, estado novo para economizar menos que o simples
  desligar.
