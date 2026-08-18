/**
 * garraia-installer — Cloudflare Worker que serve o `install.sh` em
 * `https://get.garraia.cloud` (issue #827).
 *
 * Por que existe: `raw.githubusercontent.com` aplica rate limit por IP e o
 * IP de saída de pods cloud (RunPod etc.) é compartilhado — o one-liner do
 * site morria com HTTP 429 antes de qualquer código do repo executar. Este
 * Worker busca o script a partir de IPs da Cloudflare e o mantém no cache
 * de edge, tirando o rate limit por IP do usuário final do caminho.
 *
 * Contrato (issue #827 §Proposta):
 *   - `GET /` e `GET /install.sh` respondem o conteúdo de `install.sh` do
 *     branch `main`, com `Content-Type: text/x-shellscript; charset=utf-8`.
 *   - Cache no edge via `caches.default` com TTL curto (5 min) — mudanças
 *     no `install.sh` propagam em <= CACHE_TTL_SECONDS.
 *   - Fallback: se o raw responder erro, tenta o mirror jsDelivr (mesmo
 *     canal alternativo documentado pelo PR #826) antes de desistir.
 *   - Nunca cacheia falha: upstream indisponível => 502 `no-store` com os
 *     canais alternativos no corpo, para o operador seguir manualmente.
 */

const UPSTREAMS = [
  "https://raw.githubusercontent.com/michelbr84/GarraRUST/main/install.sh",
  "https://cdn.jsdelivr.net/gh/michelbr84/GarraRUST@main/install.sh",
];

const CACHE_TTL_SECONDS = 300; // 5 min — aceite da issue: propagação <= TTL

export default {
  async fetch(request, _env, ctx) {
    if (request.method !== "GET" && request.method !== "HEAD") {
      return new Response("method not allowed\n", {
        status: 405,
        headers: { allow: "GET, HEAD" },
      });
    }

    const url = new URL(request.url);
    if (url.pathname !== "/" && url.pathname !== "/install.sh") {
      return new Response("not found — use / (ou /install.sh)\n", {
        status: 404,
        headers: { "content-type": "text/plain; charset=utf-8" },
      });
    }

    // `/` e `/install.sh` compartilham uma única entrada de cache.
    // NOTA: `caches.default` é no-op em domínios *.workers.dev — o cache de
    // edge só funciona atrás de zona própria (get.garraia.cloud). Ver README.
    const cacheKey = new Request(new URL("/install.sh", url.origin), {
      method: "GET",
    });
    const cache = caches.default;

    let response = await cache.match(cacheKey);
    if (!response) {
      response = await fetchInstallerFromUpstreams();
      if (response.ok) {
        ctx.waitUntil(cache.put(cacheKey, response.clone()));
      }
    }

    if (request.method === "HEAD") {
      return new Response(null, {
        status: response.status,
        headers: response.headers,
      });
    }
    return response;
  },
};

async function fetchInstallerFromUpstreams() {
  let lastError = "nenhum upstream tentado";

  for (const upstream of UPSTREAMS) {
    const host = new URL(upstream).host;
    try {
      const res = await fetch(upstream, {
        headers: { "user-agent": "garraia-installer-worker (+issue #827)" },
      });
      if (res.ok) {
        const body = await res.text();
        return new Response(body, {
          status: 200,
          headers: {
            "content-type": "text/x-shellscript; charset=utf-8",
            "cache-control": `public, max-age=${CACHE_TTL_SECONDS}`,
            "x-content-type-options": "nosniff",
            // Diagnóstico: de onde veio a cópia cacheada (nunca é segredo).
            "x-garraia-upstream": host,
          },
        });
      }
      lastError = `${host} respondeu ${res.status}`;
    } catch (err) {
      lastError = `${host} falhou: ${err}`;
    }
  }

  return new Response(
    "instalador GarraIA temporariamente indisponível (" +
      lastError +
      ").\nCanais alternativos:\n" +
      "  curl -fsSL https://github.com/michelbr84/GarraRUST/releases/latest/download/install.sh | sh\n" +
      "  curl -fsSL https://cdn.jsdelivr.net/gh/michelbr84/GarraRUST@main/install.sh | sh\n",
    {
      status: 502,
      headers: {
        "content-type": "text/plain; charset=utf-8",
        "cache-control": "no-store",
      },
    },
  );
}
