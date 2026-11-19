- **`garra_diagnostics` diz se o processo roda o build instalado (#1914).**
  A entrada nova `versao.executavel`, logo depois de `versao.instalada`,
  traz o SHA-256 do executavel CARREGADO (no Linux/Android, `/proc/self/exe`,
  que abre a imagem em uso mesmo depois de o arquivo ter sido trocado) e o do
  arquivo INSTALADO hoje no caminho do executavel (sem o sufixo ` (deleted)`
  do procfs). Dois hashes lidos e iguais dao `ok`; diferentes dao `degradado`
  com "reinicie o gateway" — o caso de um `garra update` sem restart, que a
  versao do crate sozinha nao distingue (rc5 e rc6 saem os dois como
  `0.5.2`). Leitura negada sai `nao_verificado` com motivo fechado
  (`acesso_negado`, `nao_encontrado`, `falha_de_leitura`,
  `caminho_desconhecido`), estado `erro` e `iguais: null`:
  falta de leitura nunca vira igualdade. Plataforma sem handle para a imagem
  carregada (macOS, Windows) sai `nao_suportado`, com so o hash do instalado.
  Nenhum caminho entra no relatorio, e a tool continua so do operador e
  fail-closed. Build/revisao embutida, servico/PID e canal de release ficam
  para as proximas fatias da #1914.
