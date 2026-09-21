- **`ToolRegistry::execute_program` (`garraia-tools`) marcada `#[deprecated]`
  (#1226).** A funcao entrou pela #1224 como primeiro passo do Code Mode, mas
  ficou onde o runtime nao alcanca: `garraia-tools` nao e dependencia do
  `AgentRuntime`, a funcao nao consulta o `ToolGate` dos modos e nao tem
  nenhum chamador fora dos proprios testes da crate. Deixa-la publica sem
  aviso convidava alguem a liga-la ao loop por fora do gate — exatamente o
  caminho que a #1226 quer fechar. O atributo (`since = "0.4.3"`, a versao do
  trem GarraIA, nao a da crate) aponta para a substituta: a `tool_program`
  intrinseca do `AgentRuntime`, com gate por passo, que e a #1226 S-B. Nada
  foi removido nesta fatia; os testes seguem exercitando a funcao sob
  `#[allow(deprecated)]`.
