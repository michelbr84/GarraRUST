- **Suites unitarias de `garraia-config`, `garraia-gateway` e `garraia-learning` passam num host Windows (#1762).**
  Eram as 20 falhas que o job `Unit tests (windows-latest)` (#1697) expos na primeira execucao
  fora do Linux. Fixtures portaveis onde a producao e multiplataforma (caminhos absolutos na
  forma nativa, scans de fonte normalizando CRLF, caminho nao verbatim para o `..` chegar ao
  `canonicalize`, raiz canonica como `allowed_roots()` entrega); `cfg(unix)` so onde a feature e
  Unix por natureza (fiacao do sandbox) ou a premissa nao existe no Windows (duas grafias da
  passphrase, que la sao a mesma variavel de ambiente); `config check` passa a ser testado
  acusando o sandbox fora de Unix. Unica mudanca de producao: `relative_skill_path` do
  `garraia-learning` emite pathspec de git com `/` no Windows. O job ganha chave de cache
  propria com fallback de leitura na do `Test`, para nunca gravar um `target/` incompleto sobre
  a chave do check obrigatorio.
