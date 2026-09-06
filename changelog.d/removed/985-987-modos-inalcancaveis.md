- **Duas das quatro camadas do sistema de modos eram inalcancaveis, e sairam
  (#985, #987).** O `garraia-runtime/src/mode.rs` tinha ~910 linhas com
  `AgentMode`, `ModeProfile`, `ToolPolicy` e `ModeEngine` proprios e **nunca
  foi instanciado**: o unico consumidor do crate e o gateway, que importa so o
  `RuntimeSettings`. Os ~250 usos de `ModeEngine::new()` que o arquivo tinha
  eram testes dele mesmo. O `/mode` e o `/modes` de
  `garraia-channels/src/commands/builtins/` idem: `register_builtins` nao e
  chamado em lugar nenhum, e o registry que atende o usuario nasce vazio e e
  preenchido so pelo `register_commands` do gateway.
- **A #985 descrevia o risco errado, e o certo e menor.** Ela falava em
  "validar um modo que o runtime nao aplica" — isso nao acontecia, porque
  codigo inalcancavel nao executa nada. O custo real era manutencao dupla e
  leitura enganosa: o `ToolPolicy` morto tinha `read_only` e o vivo nao, e a
  #988 chegou a propor "honrar `read_only`" com base na copia morta. A #987
  falava em "comportamento divergente em canais que usam a camada generica" —
  nao existe tal canal.
- **Fica registrado qual e o canonico**, no lugar onde alguem vai procurar:
  `garraia_agents::modes` e o que o `/mode`, o `POST /api/mode/select` e o
  `GET /api/modes` usam; e o roteamento automatico em producao e o
  `garraia_gateway::auto_router`, nao o `AutoRouter` de
  `garraia_agents::agent_mode`, que tambem nao tem chamadores.
- Os outros doze comandos de `builtins/` continuam no mesmo estado de
  inalcancavel. Nao os apaguei: liga-los ou remove-los e decisao de quem os
  escreveu. Mas o docblock do modulo agora diz que nao estao ligados, para nao
  enganar um terceiro leitor — ja enganou dois.
