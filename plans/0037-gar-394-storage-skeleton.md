# Plan 0037 — GAR-394: `garraia-storage` skeleton (trait ObjectStore + LocalFs)

**Status:** Em execução
**Autor:** Claude Opus 4.7 (sessão autônoma, ClaudeMaxPower + Superpowers)
**Data:** 2026-04-21 (America/New_York)
**Issues:** [GAR-394](https://linear.app/chatgpt25/issue/GAR-394) (parcial — slice 1 de N)
**Branch:** `feat/0037-gar-394-storage-skeleton`
**Unblocks:** GAR-395 (tus resumable uploads), `message_attachments` handler, `task_attachments` handler, `/v1/files/*` endpoints

---

## 1. Goal

Criar o crate `garraia-storage` no workspace com o menor skeleton útil:

1. `trait ObjectStore` async com a API mínima: `put`, `get`, `delete`, `exists`,
   `head` (metadata), e assinaturas *async* para presigned URLs (`presign_put`,
   `presign_get`) retornando erro `Unsupported` para backends que não os
   implementam no slice 1.
2. `LocalFs` — impl completa (exceto presigned URLs) com:
   - prefix de base dir configurável
   - path sanitization (rejeita `..`, path absoluto, bytes nulos, pipes)
   - criação idempotente de diretórios pai
   - `head` retornando tamanho + last-modified + etag=sha256 do conteúdo
3. Integração mínima: `garraia-storage` entra no `[workspace] members`, é
   compilável, clippy-verde, testado.

Este slice **habilita** os próximos consumers (crate `garraia-gateway` ao
implementar `/v1/files/*`), mas **não** os entrega. S3/MinIO ficam para
slice 2+, tus.io resumable para slice 3+.

## 2. Non-goals

- **Não** implementa `S3Compatible` (aws-sdk-s3) — slice 2 (GAR-394 impl S3).
- **Não** implementa `MinioBackend` dedicado — slice futuro; a API S3 já
  cobre MinIO via endpoint override.
- **Não** implementa presigned URLs reais — `LocalFs` retorna
  `StorageError::Unsupported` para `presign_*`, backends futuros
  implementam.
- **Não** toca em `garraia-workspace/files` (migration 003 já mergeada);
  nenhum dado vai/vem do DB neste slice.
- **Não** integra com nenhum handler HTTP em `garraia-gateway`; zero
  blast radius no gateway neste PR.
- **Não** implementa integrity HMAC (ADR 0004 §5 HMAC anti-tampering) —
  slice 2+ junto com S3. Etag SHA-256 é computado no `head`/`put` de
  `LocalFs` como baseline.
- **Não** implementa versionamento nativo — slice futuro; o skeleton só
  grava `object_key` + bytes.

## 3. Scope

**Novos arquivos:**

- `crates/garraia-storage/Cargo.toml`
- `crates/garraia-storage/src/lib.rs` — re-exports
- `crates/garraia-storage/src/error.rs` — `StorageError` + `Result`
- `crates/garraia-storage/src/object_store.rs` — `trait ObjectStore`,
  `PutOptions`, `GetResult`, `ObjectMetadata`
- `crates/garraia-storage/src/local_fs.rs` — `LocalFs` impl
- `crates/garraia-storage/src/path_sanitize.rs` — função compartilhada de
  validação de `object_key`
- `plans/0037-gar-394-storage-skeleton.md` (este arquivo)

**Arquivos modificados:**

- `Cargo.toml` (workspace root) — adicionar `crates/garraia-storage` ao
  `members` + entrada em `[workspace.dependencies]`
  (`garraia-storage = { path = "crates/garraia-storage" }`)
- `plans/README.md` — entrada 0037
- `CLAUDE.md` — muda `garraia-storage` da lista de *planejados* para *ativos*

**Escopo mínimo do trait (slice 1):**

```rust
#[async_trait::async_trait]
pub trait ObjectStore: Send + Sync + 'static {
    async fn put(&self, key: &str, bytes: Bytes, opts: PutOptions) -> Result<ObjectMetadata>;
    async fn get(&self, key: &str) -> Result<GetResult>;
    async fn head(&self, key: &str) -> Result<ObjectMetadata>;
    async fn delete(&self, key: &str) -> Result<()>;
    async fn exists(&self, key: &str) -> Result<bool>;
    async fn presign_put(&self, key: &str, ttl: Duration) -> Result<Url>;
    async fn presign_get(&self, key: &str, ttl: Duration) -> Result<Url>;
}
```

`async_trait` é preferido a AFIT nativo neste slice porque
`dyn ObjectStore` será comum na fase 3.5 (múltiplos backends em runtime).

**Testes:**

- Unit tests em `path_sanitize`: `..`, paths absolutos (Unix/Windows),
  NUL bytes, nomes reservados, caminhos válidos.
- Unit tests em `local_fs`: roundtrip put/get, delete idempotente,
  exists, head retorna etag SHA-256 consistente, overwrite preserva etag
  novo, error paths (chave vazia, chave inválida).
- Smoke test: usar `tempdir` para isolar cada teste; não polui `/tmp`.

## 4. Acceptance criteria

1. `cargo check -p garraia-storage` verde.
2. `cargo clippy -p garraia-storage --all-targets -- -D warnings` verde.
3. `cargo fmt --check -p garraia-storage` verde.
4. `cargo test -p garraia-storage` verde com ≥ 12 unit tests.
5. `cargo build --workspace --exclude garraia-desktop` verde (crate entra
   no workspace sem quebrar nada).
6. `garraia-storage` aparece em `CLAUDE.md` como crate **ativo**.
7. Path sanitization rejeita: `..`, absolute paths, NUL bytes, reserved
   names (`CON`, `PRN` no Windows), chaves vazias.
8. `LocalFs::put` + `LocalFs::get` roundtrip exato (bytes idênticos).
9. `LocalFs::head` retorna `Some(ObjectMetadata)` com etag=sha256 quando
   existe, `StorageError::NotFound` quando não.
10. `LocalFs::delete` em chave inexistente é idempotente (não retorna erro).
11. `LocalFs::presign_*` retorna `StorageError::Unsupported` com mensagem
    clara.
12. `@code-reviewer` APPROVE.
13. `@security-auditor` APPROVE (path traversal é SEC-H foco).
14. CI 9/9 green.

## 5. Design rationale

### 5.1 Por que `async_trait` em vez de AFIT

Fase 3.5 prevê múltiplos backends co-existindo em runtime (`LocalFs` para
dev, `S3Compatible` para prod). `dyn ObjectStore` precisa funcionar com
AFIT + `dyn` tem caveats (no Rust stable via `native-tls-box-return`). O
`async_trait` macro elimina a dor e é aceito pelo resto do projeto
(presente em 4 crates).

### 5.2 `put` retorna `ObjectMetadata` com `etag`

Caller (gateway `/v1/files POST`) grava o etag retornado em
`file_versions.checksum_sha256` + `file_versions.integrity_hmac`
(migration 003 já exige regex `^[0-9a-f]{64}$`). `LocalFs` computa
SHA-256 no put; S3 devolve o etag nativo (que *é* SHA-256 quando upload é
single-part; para multi-part o contrato muda e S3 retorna um etag
sintético — esse edge fica para slice 2).

### 5.3 `path_sanitize` compartilhado

Todos os backends precisam da mesma política para chaves (`object_key`
da ADR 0004). Centralizar em um módulo evita drift entre impls. A função
retorna `Result<&str, SanitizeError>` — ok vira passagem zero-cost, erro
carrega a regra violada.

### 5.4 `presign_*` async + `Unsupported` para LocalFs

O trait **deve** expor presigned URLs porque o GAR-394 inteiro promete isso
(ADR 0004 §Presigned URLs ≤ 15 min). Mas `LocalFs` não tem como emitir
URL assinada por HTTP — não existe servidor. Portanto o skeleton:

- Define a assinatura async (`Duration` TTL + retorno `Url`).
- `LocalFs` implementa retornando `StorageError::Unsupported`.
- Gateway detecta `Unsupported` e responde 501 Not Implemented ao
  endpoint `/v1/files/{id}/presign/*` em dev.

Slice 2 (S3) implementa de verdade; dev workflow usa `?disposition=inline`
direto no handler sem presign.

### 5.5 `Bytes` em vez de `Vec<u8>`

`bytes::Bytes` é a moeda corrente do ecossistema Rust async (Axum, Hyper,
Tower todos usam). Evita clones ao fluir da rede para o disco. `LocalFs`
ainda precisa derreter a `Bytes` em `&[u8]` para escrever, mas zero-copy
no path de leitura.

## 6. Testing strategy

- **Unit tests** `#[tokio::test]` para cada método do trait em `LocalFs`
  isolados com `tempfile::tempdir`.
- **Path sanitize tests** — table-driven: inputs válidos e inválidos.
- **Smoke test** integration ad-hoc: `cargo test -p garraia-storage` no
  CI (adicionado por default ao matrix de test).
- **Roundtrip etag**: `put` → `head` → comparar etag retornado vs etag
  novo `head`. Qualquer divergência = falha de consistência.

## 7. Security review triggers

- **SEC-H path traversal**: a `path_sanitize` TEM QUE rejeitar `..` em
  todas as posições, paths absolutos (tanto `/foo` quanto `C:\foo`),
  NUL bytes (envenenamento de C-APIs), caracteres reservados do Windows
  (`CON`, `PRN`, `AUX`, `NUL`, `COM1-9`, `LPT1-9`).
- **SEC-H race entre put+delete**: o skeleton não garante atomicidade;
  documentar explicitamente que `LocalFs` não é safe para writers
  concorrentes na mesma chave — slice futuro pode usar `AtomicPath` ou
  `tempfile::persist`.
- **SEC-M memory exhaustion**: `put` recebe `Bytes` inteiro em memória.
  Slice 2 precisa streaming via `AsyncRead`. Por ora documentar no plan.
- **SEC-M log redaction**: nenhuma linha de log pode conter o conteúdo de
  `bytes`. Só a chave e o tamanho.
- **SEC-L etag timing**: SHA-256 é O(n); documentar que `put` é O(n) em
  latência (aceitável, já que caller sabe o tamanho).

## 8. Rollback plan

Reversível via `git revert <merge-commit>`. Nenhum schema mudou; nenhum
handler existente mudou. O crate `garraia-storage` aparece ou some do
workspace — sem efeito lateral.

## 9. Risk assessment

| Risco | Severidade | Mitigação |
|---|---|---|
| Path traversal na chave | HIGH | `path_sanitize` + testes específicos. Falha = rejeição com erro. |
| Race em writes concorrentes | MEDIUM | Documentado; slice 2 fix. |
| Bytes inteiro em RAM para files grandes | MEDIUM | Documentado; slice 2 faz streaming. |
| `async_trait` adiciona overhead vs AFIT | LOW | Overhead é um Box<dyn Future> por call — negligible na ordem de magnitude de disk I/O. |

## 10. Open questions

Nenhuma — o escopo é cirúrgico. S3 vem em slice separado onde haverá
decisões (endpoint override, region, path-style vs virtual host-style,
SSE-S3 vs SSE-KMS).

## 11. Future work

- Slice 2 (GAR-394 impl S3): `S3Compatible` via `aws-sdk-s3` + presigned
  URLs reais + SSE-S3 + content-type allowlist + HMAC integrity opcional.
- Slice 3 (GAR-395): tus.io resumable uploads em cima do trait.
- Slice 4: streaming `AsyncRead`/`AsyncWrite` no trait (breaking change do
  signature `put`/`get` — requer migration no consumer slice por slice).

## 12. Definition of done

- [x] Plan mergeado (este arquivo).
- [ ] Crate `garraia-storage` criado e compilando.
- [ ] Unit tests verdes.
- [ ] Workspace compila inteiro (`cargo build --workspace --exclude garraia-desktop`).
- [ ] Clippy + fmt verdes.
- [ ] Code review aprovado.
- [ ] Security audit aprovado ≥ 8.5/10.
- [ ] CI 9/9 green.
- [ ] PR mergeado em `main`.
- [ ] Linear GAR-394 **comentado** (continua aberta — slice 1 de N).
- [ ] `CLAUDE.md` atualizado (garraia-storage de planejado → ativo).
- [ ] `plans/README.md` atualizado.
- [ ] `.garra-estado.md` atualizado ao fim da sessão.
