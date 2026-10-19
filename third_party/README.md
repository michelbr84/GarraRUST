# Forks vendorizados de dependências de terceiros

Este diretório contém cópias de crates publicados no crates.io com uma
alteração mínima, aplicadas via `[patch.crates-io]` no `Cargo.toml` da raiz.
Cada fork existe para eliminar advisories RUSTSEC que a versão publicada
não pode corrigir sem uma mudança de versão major.

## Forks

### `tauri-plugin-updater` 2.14.0 (upstream: crates.io)

- **Mudança:** dependência `zip` do alvo `windows` passou de `"4"` para `"9"`.
- **Mudança (segurança, #1671):** o fork **nunca** desliga a verificação de TLS
  do updater. Os campos `dangerousAcceptInvalidCerts` e
  `dangerousAcceptInvalidHostnames` continuam sendo aceitos na config para não
  quebrar instalações existentes, mas são ignorados (com `log::warn!`).
  Motivo: os 4 sinks `danger_accept_invalid_certs(true)` /
  `danger_accept_invalid_hostnames(true)` do upstream eram vetor de MITM de
  update e apontavam pelo CodeQL (`rust/disabled-certificate-check`) no PR
  #1668. Preferência do projeto: correção em código, não supressão
  (`docs/security/codeql-suppressions.md` §3). Teste de guarda:
  `crates/garraia-gateway/tests/updater_tls_nunca_desligado.rs`.
  **Condição de remoção:** quando o upstream permitir desligar a verificação
  por opt-in registrado e auditável (ou quando o fork for removido junto com o
  do `zip`), reavaliar — a guarda impede a volta silenciosa.
- **Motivo:** `zip` 4.x está afetado por RUSTSEC-2026-0367, -0368 e -0369.
  Versões `>= 9.0.0` corrigem os três. A dependência é usada somente na
  extração de atualizações `.zip` no Windows (`src/updater.rs`, `extract_zip`),
  que o GarraRUST não publica. A API usada (`ZipArchive::new`,
  `ZipArchive::extract`) existe no `zip` 9.
- **Compatibilidade de API verificada:** `ZipArchive::new(reader)` e
  `ZipArchive::extract(&mut self, dir)` existem no zip 9.0.5 com a mesma forma
  usada em `extract_zip` (`src/updater.rs`). Replica executada com zip 9.0.5
  real (caminho feliz extrai; caminho `../` e recusado).
  **Nao compilado no alvo Windows nesta maquina.**
- **Escopo:** Linux e macOS nao compilam essa dependencia (ela e especifica
  do alvo Windows). (ela é específica
  do alvo Windows). Nenhum comportamento de atualização muda.
- **Condição de remoção:** quando o `tauri-plugin-updater` publicado usar
  `zip >= 9`, remover a entrada `[patch.crates-io]` e este diretório.

### `utoipa-swagger-ui` 10.0.1 (upstream: crates.io)

- **Mudança:** dependência de build `zip` passou de `"8.6"` para `"9"`.
- **Motivo:** o `build.rs` usa o `zip` para extrair o `swagger-ui-dist` na
  compilação. A versão 8.x está afetada pelos mesmos advisories. Não existe
  `utoipa-swagger-ui` publicado com `zip` 9 (a 10.0.1 é a mais nova).
- **Mudanca de compatibilidade (patch no fork):** no `zip` 9, `ZipFile::name()`
  retorna `Result<Cow<str>, ZipError>` (no 8.x era `Cow`). O `build.rs` foi
  ajustado em `file.name()?.ends_with('/')` (linha ~136). Esse e o unico
  ponto de codigo alterado no fork. Sem ele, o build script nao compila.
- **API verificada no zip 9.0.5:** `ZipArchive::new`, `by_index`,
  `enclosed_name`, `ZipError::InvalidArchive` e a feature `deflate`.
- **Uso no build:** a gateway usa a feature `vendored` do utoipa-swagger-ui.
  Com ela o build NAO baixa nada da rede (o zip do swagger-ui vem embutido na
  crate `utoipa-swagger-ui-vendored`). O downloader do upstream, que nao
  verifica hash do zip, nao roda. Teste `tests/swagger_ui_vendored.rs` prende
  essa escolha.
- **Condição de remoção:** quando o `utoipa-swagger-ui` publicado usar
  `zip >= 9`, remover a entrada `[patch.crates-io]` e este diretório.

## Manutenção

- Dono: mantenedores do GarraRUST (issue #1662).
- Atualização: ao subir a versão do crate upstream, reaplicar a mesma mudança
  (uma linha no `Cargo.toml` do fork) e rodar `cargo deny check advisories`.
- Não editar a lógica do código-fonte destes forks sem uma issue própria.
- Licenças: ambos os crates upstream são `Apache-2.0 OR MIT` e
  `MIT OR Apache-2.0`; os arquivos de licença originais permanecem no diretório.
