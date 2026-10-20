- **Catálogo versionado de funcionalidades embutido no binário (#1670).**
  O Garra conhece e explica as funcionalidades da versão que está executando,
  inclusive após instalação limpa (sem clone do repositório): a fonte única é o
  manifesto `crates/garraia-gateway/assets/catalogo-funcionalidades.json`
  (chaves estáveis em inglês, conteúdo pt-BR), embutido via `include_str!` —
  o build falha se o arquivo-fonte não existir e nada é lido do disco em
  runtime. Cada recurso traz descrição curta, pré-requisitos, limitações e
  estado de disponibilidade (`disponivel` / `configuravel` / `opcional`), com
  marcador de feature/config (`gate`) para o que depende de compilação ou
  interruptor (sandbox, TLS, mcp-http) — recurso que exige configuração nunca
  aparece como `disponivel`. Consulta sob demanda pela nova tool
  `garra_capabilities` (filtros `name`/`category`, limite de itens; sem filtro
  devolve só contagens — consumo de contexto baixo), integrada ao
  `garra_status`, que ganha a seção `catalog` com a versão do catálogo e o
  enlace da tool. Em turno restrito a tool devolve só o subconjunto que a
  permissão permite (itens `operator_only` retidos e contados).
