# garraia-installer — Worker do `get.garraia.cloud`

Cloudflare Worker que serve o `install.sh` do branch `main` com cache no
edge, eliminando o HTTP 429 por IP do `raw.githubusercontent.com` no
bootstrap (issue #827; contexto completo no PR #826 §"O que este PR NÃO
resolve").

```bash
curl -fsSL https://get.garraia.cloud | sh
```

## Como funciona

- `GET /` e `GET /install.sh` respondem o conteúdo de
  `https://raw.githubusercontent.com/michelbr84/GarraRUST/main/install.sh`
  com `Content-Type: text/x-shellscript; charset=utf-8`.
- **Cache de edge** (`caches.default`) com TTL de **5 minutos** — o Worker
  busca o raw raramente e a partir de IPs da Cloudflare; o rate limit por
  IP do usuário final sai do caminho. Mudanças no `install.sh` propagam em
  até 5 min (aceite da issue).
- **Fallback**: se o raw falhar, tenta o mirror jsDelivr antes de responder
  502 (com os canais alternativos no corpo). Falhas nunca são cacheadas.

## Deploy (runbook)

Pré-requisitos: conta Cloudflare com o DNS de `garraia.cloud` ativo na
Cloudflare (nameservers apontados) e `wrangler` >= 3 autenticado
(`wrangler login`).

1. Deploy inicial (fica em `garraia-installer.<subdomínio>.workers.dev`):

   ```bash
   cd deploy/installer-worker
   wrangler deploy
   ```

2. Smoke test no workers.dev (sem cache de edge — esperado; `caches.default`
   é no-op fora de zona própria):

   ```bash
   curl -fsS https://garraia-installer.<subdomínio>.workers.dev/ | head -5
   # Deve imprimir o cabeçalho do install.sh
   ```

3. Criar o registro DNS: na zona `garraia.cloud`, um registro
   `get` do tipo `AAAA 100::` (ou `CNAME` placeholder) **com proxy laranja
   ligado** — a rota do Worker intercepta antes do origin.

4. Descomentar o bloco `routes` no `wrangler.toml` e re-deployar:

   ```bash
   wrangler deploy
   ```

5. Aceite (critérios da issue #827):

   ```bash
   # Em pod Ubuntu novo:
   curl -fsSL https://get.garraia.cloud | sh
   # Header de diagnóstico (raw ou jsdelivr + cache HIT/MISS):
   curl -fsSI https://get.garraia.cloud | grep -i "x-garraia-upstream\|cf-cache-status\|content-type"
   ```

6. Pós-aceite: atualizar site + `README.md` + `docs/installation.md`
   anunciando `get.garraia.cloud` como canal primário, mantendo raw /
   release-CDN / jsDelivr como alternativos. (Deliberadamente **não** feito
   junto com este código — anunciar a URL antes do DNS + deploy estarem
   vivos quebraria a instrução para todo mundo.)

## Rollback

`wrangler delete` (ou desativar a rota na dashboard). O one-liner clássico
via raw/release-CDN/jsDelivr continua funcionando de forma independente.
