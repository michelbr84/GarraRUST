# 4. Object storage (S3-compatible, MinIO default)

- **Status:** Accepted
- **Deciders:** @michelbr84 + Claude (sessão autônoma 2026-04-21; review: `@code-reviewer` + `@security-auditor`)
- **Date:** 2026-04-21
- **Tags:** fase-3, ws-storage, files, versioning, security, gar-374
- **Supersedes:** none
- **Superseded by:** none
- **Links:**
  - Issue: [GAR-374](https://linear.app/chatgpt25/issue/GAR-374)
  - Plan: [`plans/0030-adr-batch-unblock.md`](../../plans/0030-adr-batch-unblock.md)
  - Unblocks: GAR-394 (crate `garraia-storage`), GAR-395 (tus server), GAR-387 (migration 003 files)
  - Roadmap: [ROADMAP §3.5 Files & Storage](../../ROADMAP.md)
  - Research base: [`deep-research-report.md` §Files](../../deep-research-report.md)

---

## Context and Problem Statement

A Fase 3.5 do `ROADMAP.md` entrega **arquivos compartilhados** escopo `group_id` com versionamento, soft-delete, links de compartilhamento e auditoria. Hoje não temos object storage — arquivos só existem como `TEXT` blob em mobile auth/chat. Para suportar:

- Anexos em chat (até 100 MB),
- Pastas de grupo com versionamento,
- Upload retomável (tus protocol, GAR-395),
- Deploy self-host E cloud (LGPD art. 46/GDPR art. 32 requer data residency control),

Precisamos de uma **abstração de object storage** + uma escolha de default self-host + um set de implementações shipped em v1. A decisão impacta: crate `garraia-storage` (GAR-394), migration 003 (GAR-387 — columns `object_key`, `etag`, `checksum_sha256`), handler de upload/download no gateway, UI mobile/desktop, e runbook de backup.

Esta decisão é **security-heavy**: presigned URLs, encryption at rest, data residency, audit trail. Merece `@security-auditor` review (trigger do plan 0030 §10).

---

## Decision Drivers

1. **★★★★★ Data residency / self-host** — LGPD art. 46 + "local-first" do North Star exige que usuário possa rodar storage 100% na própria máquina. Cloud-only é não-starter.
2. **★★★★★ S3 API compat** — ecossistema de storage moderno padroniza S3. Qualquer abstração que não fale S3 desperdiça 15 anos de tooling (mc, s3cmd, backup scripts).
3. **★★★★★ Versionamento obrigatório** — ROADMAP 3.5 explícito: cada arquivo tem `file_versions` table + `object_key` versioned. Soft-delete depende disso. Compliance (LGPD art. 18 direito de retificação) também.
4. **★★★★ Presigned URLs** — browser/mobile upload direto ao storage sem proxy de bytes pelo gateway. Obrigatório para uploads > 100 MB sem estourar memória do gateway.
5. **★★★★ Encryption at rest** — SSE-S3 (server-side) em cloud; filesystem-level (LUKS/dm-crypt) em local. Documentar responsabilidade compartilhada.
6. **★★★ Multi-part upload** — arquivos > 5 MB via chunks. S3 padrão.
7. **★★★ Rust SDK maturity** — `aws-sdk-s3` é maduro (ex-rusoto, oficial AWS). `s3` crate (third-party `rust-s3`) é alternative leve.
8. **★★★ Sane defaults em dev** — `cargo test` tem que poder subir storage local sem Docker. LocalFs resolve isso.
9. **★★ Cost-efficient cloud options** — Cloudflare R2 (zero egress), Backblaze B2 (low cost/TB), são S3-compat e mais baratos que AWS nativo.
10. **★★ Lifecycle rules** — expire unfinished uploads, transition old versions a cold storage. Feature futura, não v1.

---

## Considered Options

### A) **Trait `ObjectStore` + 3 impls: `LocalFs` / `S3Compatible` / `MinIO`** *(recommended)*

**O que é:** crate `garraia-storage` define trait `ObjectStore` com métodos `put`, `get`, `delete`, `presign_put`, `presign_get`, `list`, `copy`, `set_versioning`. Três implementações shipped em v1:

- **`LocalFs`** — arquivos em filesystem, para dev, CLI standalone, Raspberry Pi. Versionamento via diretório hash-prefixed (`objects/ab/cdef.../v1`, `.../v2`).
- **`S3Compatible`** — via `aws-sdk-s3` apontando para AWS S3, Cloudflare R2, Backblaze B2, Wasabi, GCS (com compat layer), qualquer S3 endpoint. Feature flag `storage-s3`.
- **`MinIO`** — na prática é `S3Compatible` com endpoint configurado (`http://minio:9000`); ship como preset documentado no docker-compose + config helper.

**Pros:**
- ✅ Trait `ObjectStore` permite swap zero-code entre dev → prod → self-host.
- ✅ S3 API cobre 15 anos de tooling.
- ✅ LocalFs destrava `cargo test` + `garraia-cli` offline sem Docker.
- ✅ MinIO é S3-compat + open source + self-host friendly.
- ✅ Versionamento ⇒ obrigatório por contrato do trait.
- ✅ Presigned URLs nativo em S3-compat; emulado em LocalFs via token HMAC + short-lived.
- ✅ Feature flags Cargo (`storage-local`, `storage-s3`, `storage-minio`) permitem slim binaries.

**Cons:**
- ⚠️ 3 code paths em tested state (mitigação: integration tests com testcontainers + LocalFs em tempdir).
- ⚠️ LocalFs presigned URL é "HMAC signed token" simulando S3 — requer endpoint custom em gateway (`GET /v1/objects/signed/{token}`).

**Fit score:** 9.5/10.

### B) **S3-compat only (sem LocalFs)**

**Pros:**
- ✅ Single code path.
- ✅ Produção direta.

**Cons:**
- ⚠️ `cargo test` precisa subir MinIO ou testcontainers toda vez — lento + dep Docker em dev.
- ⚠️ Usuário single-machine rodando GarraIA local (Raspberry Pi, desktop Mac) precisa instalar MinIO — contrário ao "local-first, zero-friction" do North Star.
- ⚠️ `garraia-cli` perde capacidade offline.

**Fit score:** 6/10.

### C) **LocalFs only + manual S3 via garraia-cli upload script**

**Pros:**
- ✅ Simplicidade máxima em dev.

**Cons:**
- ⚠️ Não escala para multi-client cloud deploy (presigned URL inexistente para browser direto).
- ⚠️ Perda de backup/lifecycle automático que S3 entrega.

**Fit score:** 3/10. Descartado.

### D) **Per-provider native APIs (Azure Blob, GCS, AWS S3, etc.) sem trait unificado**

**Pros:**
- ✅ Cada provider tunado.

**Cons:**
- ⚠️ Multiplica code paths + complicates testing.
- ⚠️ Trocar cloud = rewrite.

**Fit score:** 2/10. Descartado.

### E) **Object storage via banco (TOAST em Postgres / bytea column)**

**Pros:**
- ✅ Um só sistema.

**Cons:**
- ⚠️ pg_dump de DB com 100 GB de anexos trava backup.
- ⚠️ Postgres não tem versionamento nativo de bytea — reinventar.
- ⚠️ Sem presigned URL.
- ⚠️ Performance terrível em arquivos > 10 MB.

**Fit score:** 1/10. Descartado.

---

## Decision Outcome

**Escolha: Opção A — trait `ObjectStore` com 3 impls shipped em v1.**

### Contrato do trait `ObjectStore` (v1)

```rust
use async_trait::async_trait;
use bytes::Bytes;
use std::time::Duration;

#[async_trait]
pub trait ObjectStore: Send + Sync {
    /// Upload full object. Returns etag + version_id (when versioning enabled).
    async fn put(&self, key: &ObjectKey, body: Bytes, metadata: PutMetadata) -> Result<PutOutcome>;

    /// Download full object.
    async fn get(&self, key: &ObjectKey, version_id: Option<&str>) -> Result<GetOutcome>;

    /// Delete object or specific version. Returns marker when versioning on.
    async fn delete(&self, key: &ObjectKey, version_id: Option<&str>) -> Result<DeleteOutcome>;

    /// Presigned PUT URL for direct browser/mobile upload. TTL must be ≤ 15 min.
    async fn presign_put(&self, key: &ObjectKey, ttl: Duration, metadata: PutMetadata) -> Result<PresignedUrl>;

    /// Presigned GET URL. TTL must be ≤ 15 min.
    async fn presign_get(&self, key: &ObjectKey, version_id: Option<&str>, ttl: Duration) -> Result<PresignedUrl>;

    /// List objects under prefix (paginated).
    async fn list(&self, prefix: &str, cursor: Option<&str>, limit: usize) -> Result<ListPage>;

    /// Copy (server-side when available).
    async fn copy(&self, src: &ObjectKey, dst: &ObjectKey) -> Result<CopyOutcome>;
}
```

### Key schema

```
{group_id}/{folder_path}/{file_uuid}/v{N}
```

Exemplo: `a1b2c3.../docs/readme/f4e5d6.../v3`. `folder_path` é opcional; root files têm apenas `{group_id}/{file_uuid}/vN`.

**Sanitização obrigatória de `folder_path`** (construída em `ObjectKey::new()`, fail-closed):
- Max 512 chars.
- Componentes vazios (`//`) rejeitados.
- Componentes `.` e `..` rejeitados (anti-traversal).
- Charset restrito: `[a-zA-Z0-9_\-./]` — rejeita Unicode lookalikes, `%`-encoded sequences, barras invertidas.
- Separador canônico: `/` (não `\`, não `⁄`, etc.).
- Validação executada ANTES de qualquer chamada para impl (LocalFs vulnerável a path traversal real em filesystems POSIX; S3 trata opaque mas consistência evita surpresa).

Rationale:
- **Prefix por `group_id`** permite S3 bucket policies por grupo + analytics.
- **UUID por file** evita collision e abstrai renames (rename é só metadata em `files` table, não movimento físico).
- **Versioned suffix `vN`** funciona em todos os 3 impls mesmo quando S3 versioning estiver off (LocalFs).

### Security policy (security-auditor review triggers)

1. **Presigned URL TTL MÁXIMO 15 minutos.** `Duration::from_secs(15 * 60)` é o cap runtime; valor maior é rejeitado com `Error::TtlTooLong`.
2. **Encryption at rest**:
   - S3/MinIO: SSE-S3 (AES-256 server-side) obrigatório. SSE-KMS opcional via config. Bucket policy bloqueia uploads sem `x-amz-server-side-encryption`.
   - LocalFs: responsabilidade do operador (dm-crypt/LUKS/FileVault). Documentado em `docs/storage.md`.
3. **Content-Type validation (allow-list)**: `put` aceita somente MIME types em **allow-list** explícita: `image/*` (`png, jpeg, webp, gif, svg+xml`), `application/pdf`, `video/mp4`, `audio/mpeg | ogg | wav`, `application/zip`, `application/json`, `text/*` (`plain, csv, markdown`), `application/vnd.openxmlformats-officedocument.*`. Qualquer outro tipo requer flag `--unsafe-mime` explícita no request (gateway loga WARN + audit event `file.unsafe_mime_accepted`). Deny-list seria reativa; allow-list fail-closed é consistente com `CLAUDE.md` regra 6 (nunca expor superfície ampla sem explicit opt-in). Lista é configurável via `storage.allowed_mime_types` no `garraia-config`.
4. **Checksum integrity + HMAC anti-tampering**: upload computa `sha256` do conteúdo E **HMAC-SHA256** sobre `{object_key}:{version_id}:{sha256_hex}` assinado pela chave do servidor (reuso `GARRAIA_REFRESH_HMAC_SECRET` ou chave dedicada `GARRAIA_STORAGE_HMAC_SECRET`). Ambos armazenados em `file_versions.checksum_sha256` e `file_versions.integrity_hmac`. `get` verifica ambos antes de retornar conteúdo — se o operador trocar o blob no bucket, o HMAC recomputado com a chave do servidor NÃO bate. Crítico especialmente para LocalFs (sem server-side integrity nativo).
5. **Presigned URL scoping**: sempre escopado a `{group_id}/{key}` — token assinado inclui hash de escopo; substituição de path no URL falha verificação. `presign_get` **SEMPRE** inclui `Content-Disposition: attachment; filename="..."` no response signing policy (mitiga render inline de MIMEs ambíguos e reduz surface de XSS via SVG/HTML). Response headers sugeridos no endpoint que entrega a URL: `Referrer-Policy: no-referrer` (evita vazar token em navegação subsequente). S3 access log filter deve excluir query strings (documentado em `docs/storage.md` deploy guide).
6. **Audit trail**: toda operação emite `audit_events` via `garraia-auth::audit_workspace` (plan 0021). Eventos obrigatórios v1: `file.uploaded`, `file.deleted`, `file.version_pruned`, **`file.presign_get_issued`** (com `{caller_id, group_id, file_id, ttl_secs, ip}` — rastreabilidade de acesso a dados pessoais, LGPD art. 46 / GDPR art. 32), **`file.access_denied`** (tentativa cross-tenant, útil para detecção de enumeração). `file.shared` (link externo) + `file.permission_changed` ficam para v2 quando share_type estender.
7. **Cross-tenant isolation**: handler `/v1/files/{id}` valida `file.group_id = caller.group_id` ANTES de presign. Falha = 404 (não 403, para não vazar existência do `file_id`). Alinhado com padrão de Stripe/GitHub e NIST AC-6.
8. **Short-lived credentials em cloud**: AWS creds via IAM Role (não long-lived access keys quando possível). Documentado em `docs/storage.md`.
9. **LocalFs encryption — gate explícito**: `garraia-storage` em backend `local` loga `WARN "LocalFs backend without disk-level encryption attestation; LGPD art. 46 compliance is operator responsibility"` no startup **salvo** quando `GARRAIA_STORAGE_ENCRYPTED_DISK=true` declarar que o operador confirmou dm-crypt/LUKS/FileVault. Não é enforcement (não podemos detectar encrypted disk de forma portável), mas é uma tripwire documentada.
10. **Presigned URL TTL range [30s, 900s]**: cap MÁXIMO 15 min, mas também cap MÍNIMO 30s para evitar falhas silenciosas de token expirado antes do upload começar. Valores fora do range retornam `Error::TtlOutOfRange`.
11. **SSE-KMS rotation policy**: quando SSE-KMS estiver habilitado, recomendação documentada é rotation de 90 dias (alinhado com NIST SP 800-57). Documentado em `docs/storage.md`.

### Versionamento — regras

- **Toda escrita cria nova versão** (v1, v2, ...). Tabela `file_versions` registra `version`, `object_key`, `etag`, `checksum_sha256`, `integrity_hmac`.
- **Delete (soft)** = marca `files.deleted_at` + **mantém** todas as versões. Admin pode restaurar.
- **Purge automática (retention-driven)** = job periódico após retention policy (default 30 dias). Lista versões, deleta objetos, então deleta row. **v1 é protegida** dessa purge automática para preservar "origin" histórico.
- **Max versions (pruning)**: config `storage.max_versions_per_file` (default `50`). Excedendo, versão mais antiga NÃO-v1 é purgeada.
- **Data erasure explícita (LGPD art. 18 / GDPR art. 17)** = solicitação via `DELETE /v1/me` ou admin endpoint com flag `--include-origin=true`. Remove **inclusive v1** + todos os `file_versions` associados. Job de purge tem modo `--include-origin` que atende compliance. A proteção de v1 vale APENAS para purge automática por retention, NUNCA para right-to-erasure. Documentado em `docs/compliance/data-erasure.md` (a criar em GAR-400 / GAR-399).

### Lifecycle / retention

- LGPD art. 16: dados devem ser apagados ao fim do tratamento. Retention policy por grupo (`groups.settings_jsonb.retention_days`).
- v1 **não implementa** transition rules (hot → cold tier). Fica para Fase 6.
- v1 **implementa**: expire de uploads tus incompletos após 24h.

### Cloud provider recommendations (docs, não enforce)

| Provider | Fit | Custo/TB | Egress | Residency |
|---|---|---|---|---|
| **MinIO (self-host)** | ✅ default self-host | hardware cost | 0 | total user control |
| **Cloudflare R2** | ✅ recommended cloud | $15/mo | **FREE** | global, EU/US/Asia regions |
| **Backblaze B2** | ✅ alt cloud | $6/mo | $0.01/GB | US/EU |
| **AWS S3** | ✅ enterprise | $23/mo | $0.09/GB | 30+ regions |
| **GCS via S3 compat** | ⚠️ works but APIs quirks | $20/mo | $0.12/GB | 40+ regions |
| **Azure Blob** | ❌ não S3 compat | — | — | — |

Primary recommendation self-host: **MinIO**. Primary recommendation cloud: **Cloudflare R2** (zero egress matters para mobile clients pulling anexos).

---

## Consequences

### Positive

- Crate `garraia-storage` ganha scope claro (GAR-394).
- Migration 003 (GAR-387) pode ser escrita com `object_key` coluna tipada.
- Upload tus (GAR-395) tem backend-agnostic path.
- Presigned URLs destravam upload direto de mobile/browser.
- Versionamento + audit trail atende LGPD/GDPR compliance (GAR-399, GAR-400).
- Deploy flexibility: dev (LocalFs) → staging (MinIO) → prod (R2/S3) sem code change.

### Negative

- 3 code paths (mitigação: integration tests por backend).
- LocalFs presigned URL requer handler custom no gateway (`GET /v1/objects/signed/{token}`).
- Content-Type deny-list precisa ser revisada periodicamente.

### Neutral

- Config via `garraia-config` (quando GAR-379 fechar completamente) exposes `storage.backend = "local" | "s3" | "minio"`.
- Testcontainers fornece MinIO para integration tests em CI (bloqueia via GAR-401 ainda em backlog).

---

## Supersession path

Superseded se:
- S3 API deixar de ser padrão de facto (unlikely em 5 anos).
- Novo requisito de compliance exigir storage com key escrow nativo (SSE-C) — nesse caso, extend trait com `put_with_key`.
- Performance de LocalFs for bloqueante em single-machine workloads > 10k files → consider embedded object store (IceDB, SeaweedFS).

---

## Links de referência

- S3 API spec: <https://docs.aws.amazon.com/AmazonS3/latest/API/Welcome.html>
- MinIO docs: <https://min.io/docs/minio/linux/index.html>
- Cloudflare R2: <https://developers.cloudflare.com/r2/>
- `aws-sdk-s3` crate: <https://docs.rs/aws-sdk-s3>
- tus 1.0 protocol: <https://tus.io/protocols/resumable-upload>
- LGPD art. 46 (segurança): <https://www.planalto.gov.br/ccivil_03/_ato2015-2018/2018/lei/l13709.htm>
- GDPR art. 32 (security of processing): <https://gdpr-info.eu/art-32-gdpr/>
- ADR 0003 (Postgres accepted): [`0003-database-for-workspace.md`](0003-database-for-workspace.md)
