- **`web_search` funciona sem chave, via SearXNG self-hosted (#1034).** A tool
  era acoplada a API do Brave e so entrava no runtime com `BRAVE_API_KEY`;
  sem a chave o agente nao tinha busca nenhuma. O backend virou plugavel
  (`SearchBackend::{Brave, Searxng}`), com a nova secao `agent.web_search`
  (`backend: brave|searxng`, `searxng_url`, ou `GARRAIA_SEARXNG_URL`). Sem a
  secao a regra e a de sempre (Brave com chave); sem chave e com URL, SearXNG;
  `backend` explicito ganha e, se faltar o que ele precisa, a tool fica de
  fora e `garra config check` aponta. O SearXNG (`/search?format=json`)
  devolve `title`/`url`/`content` mapeados para o mesmo schema do Brave; a URL
  passa pelo guard de SSRF com loopback e LAN liberados e link-local/metadata
  de nuvem bloqueados, com cliente pinado como o `web_fetch`.
