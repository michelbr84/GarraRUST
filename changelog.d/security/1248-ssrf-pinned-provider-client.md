- **`POST /api/providers` passa a conectar o provider com cliente HTTP pinado
  aos IPs validados pelo SSRF guard, com redirects desligados (fecha #1248).**
  Ate agora a rota validava o `base_url` do caller com `vet_url` +
  `IpScope::AllowPrivate` — certo — mas descartava o `VettedUrl` e deixava o
  provider construir o proprio `reqwest::Client`, com a politica de redirect
  default (ate 10 saltos) e sem `resolve_to_addrs`. Isso abria duas janelas que
  o guard existe para fechar: (1) **redirect laundering** — um host publico que
  passa na validacao responde `302` para `169.254.169.254` (metadados de nuvem)
  ou qualquer faixa interna, e o `vet_url` ja tinha rodado sem olhar o destino
  do redirect; (2) **DNS rebinding** — o host era resolvido uma vez na validacao
  e de novo na conexao, e entre as duas o registro podia mudar. O alcance nao e
  teorico: `/api/*` e auth-free, `POST /api/providers/test` devolve a latencia
  (canal de exfiltracao por tempo) e `PATCH /api/providers/default` passa a
  rotear todo o trafego de chat — e a chave do provider — pelo destino.
  Agora o `add_provider` guarda o `VettedUrl`, constroi o cliente via
  `ssrf::pinned_client` (os mesmos `resolve_to_addrs` + `redirect::none()` que
  `web_fetch`, `plugins_handler`, `health` e mais 5 pontos ja usam) e o injeta
  pelo `with_client()` que todo provider ja tinha. Como defesa em profundidade,
  os construtores default de `OpenAiProvider`, `AnthropicProvider` e
  `OllamaProvider` tambem passam a usar `redirect::Policy::none()`, para que
  nem um provider vindo do arquivo de config no boot possa ser redirecionado
  para fora do host. O caminho local (Ollama em `127.0.0.1` sob `AllowPrivate`)
  segue funcionando. Teste de regressao com wiremock prova que um `302` para um
  segundo mock nao e seguido; sem a correcao o cliente segue o redirect e
  devolve a resposta do alvo como se fosse do provider legitimo.
