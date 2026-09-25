- **O workspace padrao e o diretorio de cada sessao nascem fechados em `0700`
  no proprio `mkdir` (#1463).** Antes o diretorio era criado com a umask do
  processo (tipicamente `0755`) e so depois fechado por `set_permissions`, com
  uma janela em que qualquer usuario local o lia; e o `create_dir_all` seguia
  um symlink plantado no proprio caminho. Agora a criacao e de um componente
  so, ja com o modo certo, e um link no caminho faz o `mkdir` falhar em vez de
  ser seguido. A sequencia symlink → nao-diretorio → mkdir passa a existir uma
  unica vez, em `SessionWorkspace::garantir_raiz`, chamada pelo boot do
  gateway (para `<data_dir>/workspace`) e pelo turno (para o subdiretorio da
  sessao), em vez de duas copias que podiam divergir (#1460). O boot cria o
  `data_dir` antes, porque numa instalacao limpa ele ainda nao existe nesse
  ponto. A guarda de fonte que exige um unico ponto de montagem de
  `ToolContext` em `runtime.rs` passa a casar o construtor em qualquer forma
  (numa linha so, com caminho de modulo, com `..base`), e nao so a linha exata
  `ToolContext {` (#1464).
