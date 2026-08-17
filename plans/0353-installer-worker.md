# Plan 0353 — Worker `get.garraia.cloud` para o instalador (issue #827)

> **Status:** código entregue em 2026-08-17 (America/New_York); deploy +
> DNS ficam com o operador (pré-requisitos externos da issue).
> **Origem:** issue #827, follow-up do PR #826 ("O que este PR NÃO resolve").

## 1. Problema

O one-liner `curl -fsSL https://raw.githubusercontent.com/.../install.sh | sh`
morre com HTTP 429 em pods cloud novos: o rate limit do raw é por IP e o IP
de saída é compartilhado. Acontece antes de qualquer código do repo rodar —
não é eliminável pelo repositório; o PR #826 apenas mitigou (retries, release
asset, mirror jsDelivr).

## 2. Entrega

`deploy/installer-worker/` (opção 1 da issue — código vive neste repo):

- `worker.js` — ES module servindo `GET|HEAD /` e `/install.sh`:
  - upstream primário raw `main`, fallback jsDelivr;
  - `caches.default` com TTL 300 s (aceite: propagação <= TTL);
  - `Content-Type: text/x-shellscript; charset=utf-8` + `nosniff` +
    header de diagnóstico `x-garraia-upstream`;
  - falha de upstream => 502 `no-store` com canais alternativos no corpo
    (nunca cacheia falha);
  - 404 para outros paths, 405 para outros métodos.
- `wrangler.toml` — rota `get.garraia.cloud/*` comentada até o DNS existir.
- `README.md` — runbook de deploy em 6 passos + rollback.

## 3. Deliberadamente fora

- **Anunciar a URL** em site/README/docs/installation.md — só depois do
  deploy + DNS vivos (anunciar antes quebraria a instrução para todos).
  Passo 6 do runbook.
- Testes automatizados do Worker no CI — exigiria toolchain node/miniflare
  nova no pipeline por ~100 linhas de JS sem dependências; validação é o
  smoke test do runbook. Reavaliar se o Worker crescer.

## 4. Verificação

- Código sem dependências externas; revisão manual + smoke test pós-deploy
  (runbook §5): `curl -fsSL https://get.garraia.cloud | sh` em pod novo,
  headers `x-garraia-upstream`/`cf-cache-status`.
- A issue #827 permanece aberta até o aceite (exige DNS + deploy, fora do
  alcance do repo).
