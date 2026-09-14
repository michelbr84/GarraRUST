//! Guarda anti-CSRF **genérica** do gateway (#1182).
//!
//! O #1093 fechou `/api/learning/*` — as rotas que rodam `git` no repositório
//! do dono. O resto da superfície mutante ficou aberta: `PATCH /api/settings`
//! escreve a config, `POST /api/mode/select` troca o modo do agente,
//! `POST /api/mcp/marketplace/install` instala servidor MCP,
//! `POST /v1/chat/completions` gasta a chave de LLM do dono, `DELETE
//! /api/memory` apaga memória. Todas elas são `/api/*` auth-free por desenho
//! ("quem alcança a porta é o dono") — só que **o navegador do dono alcança a
//! porta rodando código de terceiros**: basta o dono visitar uma página, e ela
//! dispara o `POST` contra `127.0.0.1:3888` de dentro do navegador dele.
//! Pior, o CORS default era `allow_origin(Any)`, então até a *resposta* voltava
//! legível para a página do atacante.
//!
//! Este módulo é a versão genérica daquele guarda, mais as primitivas de
//! gramática de authority que o `learning_auth` já tinha e que agora são
//! compartilhadas pelos dois (e por `/ws` e `/ws/parrot`).
//!
//! ## O que [`cross_origin_guard`] faz
//!
//! Monta um [`Pedido`] a partir dos headers e da URI (o `Host` vem do header
//! ou, em HTTP/2, da pseudo-header `:authority` que o hyper guarda na URI —
//! sem esse fallback o console do próprio gateway servido com TLS nativo
//! seria recusado) e decide por método:
//!
//! - **`OPTIONS`** passa: o preflight é respondido pelo `CorsLayer`, que fica
//!   **por fora** deste middleware.
//! - **`POST`/`PUT`/`PATCH`/`DELETE`** ([`decide_escrita`]), fora da skip-list
//!   [`SEM_GUARDA`]:
//!   1. `Origin` **exatamente** igual a uma entrada de `gateway.allowed_origins`
//!      → passa (`origem_declarada`). É a escotilha do perfil reverse proxy /
//!      nome DNS declarado, onde o esquema do `Origin` (https, terminado no
//!      proxy) pode não ser o do transporte que este processo enxerga (http).
//!   2. [`cross_origin`] → recusa. `Origin` de outro esquema, de outra
//!      authority que o `Host`, `Origin: null`, fora da gramática de
//!      authority, ou `Sec-Fetch-Site: cross-site` sem `Origin`.
//!   3. Âncora anti-DNS-rebinding ([`ancora_ok`]), só quando há `Origin` →
//!      recusa se o `Host` for um nome DNS que não é `localhost` nem está em
//!      `gateway.allowed_origins`. Sem isso, o domínio do atacante
//!      re-resolvido para `127.0.0.1` manda `Origin` e `Host` iguais entre si
//!      e passa o passo 2.
//! - **`GET`/`HEAD`** passam — com uma exceção: o **handshake de WebSocket**
//!   (`Upgrade: websocket`) é julgado por [`decide_leitura`]. Leitura HTTP
//!   pura fica com o CORS, que sem `allowed_origins` não anuncia origem
//!   nenhuma e a página do atacante não lê a resposta; recusar o `GET` aqui
//!   não acrescentaria nada e derrubaria navegação (`Sec-Fetch-Site:
//!   cross-site` sem `Origin` é o clique num link para o console).
//!
//! O corpo do `403` é um `&'static str` fixo: nada do que veio no pedido
//! (`Origin`, `Host`, token) é ecoado. O log leva caminho e método, nunca o
//! valor dos headers.
//!
//! ## Handshake de WebSocket
//!
//! WebSocket não passa por CORS: `new WebSocket("ws://127.0.0.1:3888/ws")` de
//! qualquer página sobe sem preflight, e depois do handshake a página **lê**
//! cada frame. [`ws_upgrade_permitido`] aplica [`decide_leitura`] ao
//! handshake — no middleware (qualquer rota com `Upgrade: websocket`, para
//! uma rota nova nascer coberta) e de novo nos handlers de `/ws` (webchat) e
//! `/ws/parrot` (Garra Desktop), como defesa em profundidade. O cliente do
//! desktop é uma webview Tauri v2, cuja origem é `tauri://localhost`
//! (WebKitGTK/WKWebView) ou `http://tauri.localhost` (WebView2, Windows) —
//! ver [`ORIGENS_TAURI`]. Uma página web **não consegue** apresentar `Origin`
//! com esquema que não seja `http`/`https` (ou `null`), e `*.localhost`
//! resolve para loopback por definição (RFC 6761), então aceitar essas
//! origens não reabre o vetor do navegador.
//!
//! ## Residual conhecido: leitura sob DNS rebinding
//!
//! Depois do rebinding a página do atacante é **same-origin** com o gateway,
//! e o navegador não manda `Origin` em `GET` same-origin — então nenhuma
//! regra baseada em `Origin` fecha `GET /api/sessions` ou `/api/logs` nesse
//! cenário. Fechá-lo exigiria recusar todo `Host` que é nome DNS não
//! declarado, inclusive sem `Origin`, o que mataria `curl http://nas.local:3888
//! /health`, o app mobile por hostname e os healthchecks do Docker Compose.
//! O residual fica registrado em `docs/security/threat-model.md` §5.10; a
//! mitigação é `gateway.api_key` (o gate de verdade) ou servir o console só
//! por IP/`localhost`.
//!
//! ## Recorte deliberado: nada de `ConnectInfo` aqui
//!
//! O `learning_mutations_guard` tem um passo a mais — sem `gateway.api_key`
//! configurada, peer não-loopback leva `503 auth not configured`. **Este
//! guarda genérico NÃO replica isso**, de propósito: a superfície ampla
//! inclui o caminho que o **app mobile na LAN** usa hoje contra um Garra sem
//! `api_key` (cenário suportado, ver `gateway_auth.rs` e o `GarraConnection`
//! do app). Herdar o `503` do learning aqui quebraria esse app. O que este
//! módulo fecha é o que o **navegador** pode ser forçado a fazer; quem já
//! executa código na máquina (ou já alcança a porta e fabrica headers com
//! `curl`) continua sendo assunto de `gateway.api_key`, o gate de verdade.
//!
//! ## Âncora: por que não é `host_de_loopback`
//!
//! O learning ancora no `Host` ser **nome de loopback**, porque aquele passo
//! só roda no caso "peer loopback, sem credencial". Aqui a superfície é o
//! Web Console inteiro, que é legitimamente acessado por IP de LAN
//! (`http://192.168.1.10:3888`, o app mobile e o navegador do tablet). Um
//! `Host` desses não é nome de loopback e morreria. A âncora certa é outra:
//! **DNS rebinding exige um nome DNS** — um IP literal não é rebindável, e
//! `localhost` não existe no DNS público. Daí [`ancora_ok`]: IP literal,
//! `localhost`, ou nome que o dono listou em `gateway.allowed_origins`.
//! Consequência documentada: alcançar o console por **qualquer** nome DNS
//! (mDNS `.local`, Tailscale, nome de serviço Docker, ingress, reverse proxy)
//! exige esse nome em `gateway.allowed_origins`.

use std::net::{IpAddr, Ipv6Addr};
use std::sync::Arc;

use axum::extract::{Request, State};
use axum::http::{HeaderMap, Method, StatusCode, Uri, header};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use garraia_config::GatewayConfig;
use tracing::warn;

/// Corpo do 403. Constante: nada do que veio no pedido é ecoado.
pub const CORPO_CSRF: &str = "gateway: cross-origin request refused (see gateway.allowed_origins)";

/// Corpo do 403 dos handshakes WebSocket (`/ws`, `/ws/parrot`).
pub const CORPO_WS: &str = "ws: cross-origin upgrade refused (see gateway.allowed_origins)";

/// Caminhos **mutantes** que não passam por [`decide_escrita`]. Leitura com
/// `Origin` continua sob [`decide_leitura`] em todo caminho.
///
/// Não é "exceção para facilitar": cada um já é guardado por algo mais
/// estrito, e dupla aplicação só trocaria o corpo do 403 (quebrando o
/// contrato de resposta que os testes daquelas rotas já travam).
///
/// - `/api/learning/` — guarda própria do #1093 (`learning_mutations_guard`),
///   que além do anti-CSRF tem o fail-closed de peer/credencial.
/// - `/api/plugins/` — `require_admin_auth` + `require_csrf` no
///   `plugins_handler`.
/// - `/webhooks/` — server-to-server assinado (WhatsApp/Google Chat/Teams/
///   LINE). Não há navegador no caminho e o remetente nunca manda `Origin`;
///   a autenticidade vem da assinatura do provedor.
///
/// `/admin/` **não** está aqui de propósito: `POST /admin/api/setup` (cria o
/// primeiro admin numa instalação nova), `/admin/api/login` e
/// `/admin/api/recovery/*` são montados **fora** do `require_csrf` do
/// sub-router admin (`admin/routes.rs`, `public_routes`/`recovery_routes`), e
/// a única coisa entre eles e uma página visitada pelo dono é esta guarda.
/// O console admin é same-origin, então nada legítimo muda.
const SEM_GUARDA: &[&str] = &["/api/learning/", "/api/plugins/", "/webhooks/"];

/// Origens da webview do Garra Desktop (Tauri v2), aceitas no handshake de
/// WebSocket e em leituras.
///
/// Fonte: `tauri-2.11.5/src/manager/mod.rs::tauri_protocol_url` — em Windows
/// e Android o app é servido em `http://tauri.localhost` (ou `https://` com
/// `useHttpsScheme`), nas demais plataformas em `tauri://localhost`; e o
/// protocolo de IPC do próprio Tauri (`src/ipc/protocol.rs`) parseia o header
/// `Origin` desses pedidos como URL, com `tauri://localhost` nos testes dele —
/// é o mesmo header que a webview manda no handshake. Não foi medido em
/// runtime nesta entrega (este ambiente não tem GTK/webkit nem `DISPLAY`);
/// se o desktop não conectar, o log diz `ws: cross-origin upgrade refused` e
/// esta lista é o lugar a olhar.
///
/// Comparação exata, case-insensitive, barra final tolerada. Nunca curinga.
pub const ORIGENS_TAURI: &[&str] = &[
    "tauri://localhost",
    "http://tauri.localhost",
    "https://tauri.localhost",
];

/// Esquema efetivo do transporte que este processo serve: `https` só quando o
/// binário tem a feature `tls` **e** cert e chave estão configurados — o mesmo
/// critério de `server.rs`, que sem a feature avisa e cai para HTTP puro.
///
/// Olhar só para a config (como `session_auth::session_cookie_secure` faz)
/// recusaria o console inteiro de um binário de release com `tls_cert_path`
/// preenchido: o servidor fala `http`, o navegador manda `Origin: http://…`,
/// e a comparação de esquema contra `https` falharia em todo pedido.
pub fn esquema_efetivo(gateway: &GatewayConfig) -> &'static str {
    if cfg!(feature = "tls") && gateway.tls_cert_path.is_some() && gateway.tls_key_path.is_some() {
        "https"
    } else {
        "http"
    }
}

/// `gateway.allowed_origins` limpa para uso na guarda e no CORS: entradas
/// vazias e o curinga `*` caem fora, com aviso. `*` em particular faria o
/// `CorsLayer::allow_origin(list)` do tower-http entrar em pânico no boot
/// ("Wildcard origin (`*`) cannot be passed to `AllowOrigin::list`") — e é
/// exatamente o reflexo de quem quer o allow-all antigo de volta.
pub fn origens_validas(gateway: &GatewayConfig) -> Vec<String> {
    gateway
        .allowed_origins
        .iter()
        .filter_map(|origem| {
            let limpa = origem.trim().trim_end_matches('/');
            if limpa.is_empty() {
                return None;
            }
            if limpa == "*" {
                warn!(
                    "gateway.allowed_origins: `*` nao e suportado — liste origens explicitas \
                     (scheme://host[:port]); entrada ignorada"
                );
                return None;
            }
            Some(limpa.to_string())
        })
        .collect()
}

/// O header `Origin` como a guarda o enxerga.
#[derive(Clone, Copy, Debug)]
enum Origem<'a> {
    /// Header ausente: cliente não-navegador, navegação, `<img>`, same-origin
    /// `GET`.
    Ausente,
    /// Header presente mas fora de ASCII: fail-closed.
    Ilegivel,
    Valor(&'a str),
}

/// Os três campos do pedido que a guarda lê, já resolvidos.
///
/// `host` vem do header `Host` ou, na falta dele, da authority da URI — em
/// HTTP/2 o navegador manda `:authority` e nenhum header `Host`, e o hyper
/// guarda a pseudo-header na URI do request.
#[derive(Clone, Copy, Debug)]
pub struct Pedido<'a> {
    origem: Origem<'a>,
    host: Option<&'a str>,
    sec_fetch_site: Option<&'a str>,
}

impl<'a> Pedido<'a> {
    pub fn de(headers: &'a HeaderMap, uri: &'a Uri) -> Self {
        let origem = match headers.get(header::ORIGIN) {
            None => Origem::Ausente,
            Some(valor) => match valor.to_str() {
                Ok(s) => Origem::Valor(s),
                Err(_) => Origem::Ilegivel,
            },
        };
        let host = headers
            .get(header::HOST)
            .and_then(|h| h.to_str().ok())
            .or_else(|| uri.authority().map(|a| a.as_str()));
        let sec_fetch_site = headers.get("sec-fetch-site").and_then(|v| v.to_str().ok());
        Self {
            origem,
            host,
            sec_fetch_site,
        }
    }

    /// `true` quando o header `Origin` veio no pedido (legível ou não).
    pub fn tem_origin(&self) -> bool {
        !matches!(self.origem, Origem::Ausente)
    }
}

/// `true` quando o pedido chega de uma origem de navegador que não é a do
/// próprio gateway.
///
/// Um pedido legítimo do console web (servido pelo próprio gateway) traz
/// `Origin` com o mesmo esquema do transporte e a mesma authority do `Host`
/// — porta default (`:80` em http, `:443` em https) normalizada dos dois
/// lados. Um pedido do app (Dio) ou do `curl` não traz `Origin` nenhum. O
/// que **não** é legítimo: `Origin` de outro esquema (`https` contra um
/// gateway http, ou um esquema exótico), `Origin` de outra authority,
/// `Origin: null` (sandbox/redirect), ou `Sec-Fetch-Site: cross-site` sem
/// `Origin`. Origem malformada ou `Host` ausente são tratados como
/// cross-origin — fail-closed, e nunca se ecoa o valor rejeitado.
pub fn cross_origin(pedido: &Pedido, scheme: &str) -> bool {
    match pedido.origem {
        Origem::Ilegivel => true, // não-ASCII no Origin: fail-closed
        Origem::Valor(origem) => {
            if origem.eq_ignore_ascii_case("null") {
                return true;
            }
            let Some((esquema, resto)) = origem.split_once("://") else {
                return true; // sem esquema, o valor não é uma origem válida
            };
            // O esquema do Origin tem de ser o do transporte: `https` contra
            // um gateway http não é a origem do console que passa aqui.
            if !esquema.eq_ignore_ascii_case(scheme) {
                return true;
            }
            // Gramática do header `Origin` (RFC 6454): `scheme "://" host
            // [":" port]` — sem path, query, fragmento ou userinfo. O parse
            // estrito de `parse_authority` rejeita qualquer sobra; nenhum
            // navegador manda, quem manda é cliente de mentira: fail-closed.
            let Some((host_origem, porta_origem)) = parse_authority(resto) else {
                return true;
            };
            match pedido.host {
                // Sem Host (nem `:authority`) não há como confirmar mesma
                // origem — fail-closed.
                None => true,
                Some(host) => {
                    // Host fora da gramática de authority também não
                    // compara: string alguma se casa por cima de lixo.
                    let Some((host_header, porta_header)) = parse_authority(host) else {
                        return true;
                    };
                    !mesma_authority(host_origem, porta_origem, host_header, porta_header, scheme)
                }
            }
        }
        Origem::Ausente => pedido
            .sec_fetch_site
            .is_some_and(|v| v.eq_ignore_ascii_case("cross-site")),
    }
}

/// Faz o parse **estrito** de uma authority `host[:porta]` — a única forma
/// que `Origin` (RFC 6454) e `Host` (RFC 3986 §3.2) têm. Retorna o host sem
/// porta e a porta explícita, e rejeita **qualquer sobra**:
///
/// - IPv6 só entre colchetes, com fechamento exato: `[::1]`, `[::1]:3888`;
///   `[::1]lixo` e `[::1]:3888:80` não parseiam;
/// - fora de colchetes o `:` só separa porta — `host:porta:porta` morre na
///   porta que não é dígito;
/// - porta toda dígito, não vazia, no intervalo `1..=65535`;
/// - nome de host só com `[A-Za-z0-9.\-_~]`: `@` (userinfo), `/`, `?` e `#`
///   (path, query, fragmento) não têm vez numa authority.
///
/// Fail-closed: `None` para tudo que não é exatamente uma authority.
pub(crate) fn parse_authority(authority: &str) -> Option<(&str, Option<u16>)> {
    if let Some(resto) = authority.strip_prefix('[') {
        // IP-literal: `[…]` ou `[…]:porta`, e nada depois.
        let (dentro, depois) = resto.split_once(']')?;
        dentro.parse::<Ipv6Addr>().ok()?;
        let porta = if depois.is_empty() {
            None
        } else {
            Some(valida_porta(depois.strip_prefix(':')?)?)
        };
        return Some((dentro, porta));
    }
    // Fora de colchetes um `:` separa a porta — e é o único permitido:
    // o segundo `:` de `host:porta:porta` morre na porta não-dígito.
    let (host, porta) = match authority.split_once(':') {
        None => (authority, None),
        Some((antes, depois)) => (antes, Some(valida_porta(depois)?)),
    };
    if host.is_empty()
        || !host
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b'_' | b'~'))
    {
        return None;
    }
    Some((host, porta))
}

/// Porta de authority válida: toda dígito, não vazia, no intervalo
/// `1..=65535` (`u16` sem a porta zero).
fn valida_porta(p: &str) -> Option<u16> {
    if p.is_empty() || !p.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let porta: u16 = p.parse().ok()?;
    (porta > 0).then_some(porta)
}

/// Compara as duas authorities já parseadas, normalizando a porta default
/// dos dois lados: `:80` em http e `:443` em https não mudam a origem.
fn mesma_authority(
    host_origem: &str,
    porta_origem: Option<u16>,
    host_header: &str,
    porta_header: Option<u16>,
    scheme: &str,
) -> bool {
    let porta_default: u16 = if scheme.eq_ignore_ascii_case("https") {
        443
    } else {
        80
    };
    host_origem.eq_ignore_ascii_case(host_header)
        && porta_origem.unwrap_or(porta_default) == porta_header.unwrap_or(porta_default)
}

/// `true` quando a authority do `Host` é um **nome de loopback**:
/// `127.0.0.1` (ou qualquer IP de loopback), `localhost` ou `[::1]`.
///
/// É a âncora anti-DNS-rebinding do `learning_mutations_guard`: nomes de
/// loopback não existem no DNS público, então um `Host` que não é loopback
/// não pode ser o endereço pelo qual um gateway ligado em loopback de fato
/// foi alcançado — é o alias de um domínio do atacante que o navegador da
/// vítima resolveu para `127.0.0.1`. O `Host` passa pelo mesmo parse estrito
/// de [`parse_authority`]: `[::1]lixo` não é nome de loopback, é lixo —
/// fail-closed em tudo que não parseia.
///
/// A superfície ampla usa [`ancora_ok`], não esta função: ver o doc do
/// módulo.
pub fn host_de_loopback(pedido: &Pedido) -> bool {
    let Some(host) = pedido.host else {
        return false;
    };
    let Some((sem_porta, _)) = parse_authority(host) else {
        return false;
    };
    sem_porta.eq_ignore_ascii_case("localhost")
        || sem_porta
            .parse::<IpAddr>()
            .map(|ip| ip.is_loopback())
            .unwrap_or(false)
}

/// Âncora anti-DNS-rebinding da superfície ampla: `true` quando o `Host` do
/// pedido **não pode** ser um alias re-resolvido de um domínio do atacante.
///
/// Consultada só quando há `Origin` (cliente não-navegador não é vítima de
/// rebinding). Três formas de passar:
///
/// 1. **IP literal** (v4, ou v6 entre colchetes). Rebinding é um truque de
///    DNS: quem digita o IP não passa por resolvedor nenhum, e o navegador
///    da vítima não tem como ser levado a mandar `Host: 192.168.1.10` para
///    um servidor que não seja aquele. Cobre o Web Console acessado por IP
///    de LAN, que `host_de_loopback` mataria.
/// 2. **`localhost`** e `*.localhost`, que resolvem para loopback por
///    definição (RFC 6761) e DNS público nenhum aponta para fora da máquina.
/// 3. **Nome listado em `gateway.allowed_origins`** — o dono declarou o
///    domínio do reverse proxy / da LAN dele; é o mesmo ato de confiança do
///    CORS. A comparação é só do **host**, sem esquema nem porta: rebinding
///    troca o IP por trás de um nome, e a porta não participa do truque.
///    Barra final na entrada é tolerada, como em `origem_declarada`.
///
/// `Host` ausente ou fora da gramática → `false` (fail-closed).
pub fn ancora_ok(pedido: &Pedido, allowed_origins: &[String]) -> bool {
    let Some(host_header) = pedido.host else {
        return false;
    };
    let Some((host, _porta)) = parse_authority(host_header) else {
        return false;
    };
    // `parse_authority` devolve o IPv6 já sem colchetes, então o `parse`
    // cobre as duas famílias.
    if host.parse::<IpAddr>().is_ok() || nome_de_localhost(host) {
        return true;
    }
    allowed_origins.iter().any(|origem| {
        let authority = origem
            .split_once("://")
            .map(|(_, resto)| resto)
            .unwrap_or(origem.as_str())
            .trim_end_matches('/');
        parse_authority(authority)
            .is_some_and(|(permitido, _)| permitido.eq_ignore_ascii_case(host))
    })
}

/// `localhost` ou qualquer `*.localhost` (RFC 6761 §6.3: resolvem para
/// loopback sem consultar DNS). Cobre `tauri.localhost`, a origem da webview
/// do desktop em Windows.
fn nome_de_localhost(host: &str) -> bool {
    host.eq_ignore_ascii_case("localhost")
        || host
            .rsplit_once('.')
            .is_some_and(|(_, sufixo)| sufixo.eq_ignore_ascii_case("localhost"))
}

/// `true` quando o header `Origin` é **exatamente** uma das origens que o
/// dono declarou em `gateway.allowed_origins`.
///
/// É a escotilha do perfil "reverse proxy com domínio próprio": o proxy
/// termina TLS e fala `http` com o gateway por baixo, então o navegador manda
/// `Origin: https://meu.dominio` contra um transporte que [`cross_origin`]
/// conhece como `http` — a comparação de esquema recusa, e nem a âncora
/// chega a ser consultada. Sem esta função, aquele perfil não teria escotilha
/// nenhuma (nem `gateway.api_key` resolveria: este guarda roda igual com a
/// chave configurada, de propósito — ver `learning_auth_layering.rs`).
///
/// A confiança aqui é a mesma que a lista já carrega para o CORS: o dono
/// escreveu a origem no arquivo de config dele. A comparação é da string
/// inteira (`scheme://host[:port]`), case-insensitive, com uma barra final
/// opcional tolerada dos dois lados — não é match por prefixo, e uma entrada
/// vazia nunca casa.
fn origem_declarada(pedido: &Pedido, allowed_origins: &[String]) -> bool {
    let Origem::Valor(origem) = pedido.origem else {
        return false;
    };
    let origem = origem.trim_end_matches('/');
    if origem.is_empty() {
        return false;
    }
    allowed_origins
        .iter()
        .any(|declarada| declarada.trim_end_matches('/').eq_ignore_ascii_case(origem))
}

/// `true` quando o `Origin` tem esquema `http` ou `https` — o único tipo de
/// origem que uma **página web** consegue apresentar. `tauri://localhost`,
/// `chrome-extension://…` e afins são código que o dono instalou, não
/// página visitada.
fn esquema_web(origem: &str) -> bool {
    origem.split_once("://").is_some_and(|(esquema, _)| {
        esquema.eq_ignore_ascii_case("http") || esquema.eq_ignore_ascii_case("https")
    })
}

/// `true` quando o `Origin` é uma das [`ORIGENS_TAURI`] (exato, barra final
/// tolerada).
fn origem_tauri(origem: &str) -> bool {
    let origem = origem.trim_end_matches('/');
    ORIGENS_TAURI
        .iter()
        .any(|tauri| tauri.eq_ignore_ascii_case(origem))
}

/// Resultado da guarda.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Decisao {
    Passa,
    Recusa,
}

/// Decisão para pedido **mutante** (`POST`/`PUT`/`PATCH`/`DELETE`): os três
/// passos do doc do módulo. Sem `Origin` passa (cliente não-navegador), salvo
/// `Sec-Fetch-Site: cross-site`.
pub fn decide_escrita(pedido: &Pedido, scheme: &str, allowed_origins: &[String]) -> Decisao {
    // A origem que o dono declarou passa direto — é a escotilha do reverse
    // proxy, onde o esquema do `Origin` (https, terminado no proxy) não é o
    // do transporte que este processo enxerga (http).
    if origem_declarada(pedido, allowed_origins) {
        return Decisao::Passa;
    }
    if cross_origin(pedido, scheme) {
        return Decisao::Recusa;
    }
    // Âncora anti-DNS-rebinding: `Origin` e `Host` casarem entre si não
    // basta, porque o alias do atacante casa consigo mesmo.
    if pedido.tem_origin() && !ancora_ok(pedido, allowed_origins) {
        return Decisao::Recusa;
    }
    Decisao::Passa
}

/// Decisão para **leitura** (`GET`/`HEAD`) e para o **handshake de
/// WebSocket**: só há o que decidir quando veio `Origin` de esquema web.
///
/// - Sem `Origin`: navegação, `<img>`/`<script>`, `GET` same-origin do
///   console, app mobile, CLI, `curl` — passa. `Sec-Fetch-Site: cross-site`
///   sem `Origin` é exatamente o clique num link para o console a partir de
///   outro site, e **não** pode ser recusado aqui.
/// - `Origin` fora de ASCII ou `null` — fail-closed.
/// - Esquema que não é `http`/`https` (`tauri://localhost`, extensão de
///   navegador): não é página web, passa.
/// - Uma das [`ORIGENS_TAURI`]: a webview do Garra Desktop, passa.
/// - Senão, as mesmas três regras de [`decide_escrita`].
pub fn decide_leitura(pedido: &Pedido, scheme: &str, allowed_origins: &[String]) -> Decisao {
    match pedido.origem {
        Origem::Ausente => Decisao::Passa,
        Origem::Ilegivel => Decisao::Recusa,
        Origem::Valor(origem) => {
            if origem.eq_ignore_ascii_case("null") {
                return Decisao::Recusa;
            }
            if !esquema_web(origem) || origem_tauri(origem) {
                return Decisao::Passa;
            }
            decide_escrita(pedido, scheme, allowed_origins)
        }
    }
}

/// `true` quando o pedido é um handshake de WebSocket (`Upgrade: websocket`,
/// RFC 6455 §4.1 — case-insensitive, e o header pode listar mais de um
/// protocolo).
fn handshake_websocket(headers: &HeaderMap) -> bool {
    headers
        .get_all(header::UPGRADE)
        .iter()
        .filter_map(|v| v.to_str().ok())
        .flat_map(|v| v.split(','))
        .any(|p| p.trim().eq_ignore_ascii_case("websocket"))
}

/// `true` quando o handshake de WebSocket pode subir. É [`decide_leitura`]:
/// sem `Origin` o cliente não é navegador (o app, o `garra` CLI e o `curl`
/// não mandam) e quem alcança a porta sem navegador é assunto de
/// `gateway.api_key`; com `Origin`, valem as regras de leitura — inclusive a
/// escotilha das [`ORIGENS_TAURI`] para o `/ws/parrot` do desktop.
pub fn ws_upgrade_permitido(pedido: &Pedido, scheme: &str, allowed_origins: &[String]) -> bool {
    decide_leitura(pedido, scheme, allowed_origins) == Decisao::Passa
}

/// Estado da guarda genérica. Montado no `build_router` via [`OriginGuardState::new`].
#[derive(Clone)]
pub struct OriginGuardState {
    /// Esquema efetivo do transporte (`"http"` | `"https"`) —
    /// [`esquema_efetivo`].
    pub scheme: &'static str,
    /// `gateway.allowed_origins` já limpa ([`origens_validas`]).
    pub allowed_origins: Arc<[String]>,
}

impl OriginGuardState {
    pub fn new(gateway: &GatewayConfig) -> Self {
        Self {
            scheme: esquema_efetivo(gateway),
            allowed_origins: origens_validas(gateway).into(),
        }
    }
}

/// O middleware. Montado **por dentro** do gate de `gateway.api_key` (ver o
/// comentário da montagem em `router.rs`): quando os dois se aplicam, o 401
/// do gate vem primeiro.
pub async fn cross_origin_guard(
    State(estado): State<OriginGuardState>,
    req: Request,
    next: Next,
) -> Response {
    let method = req.method().clone();
    // O preflight é respondido pelo `CorsLayer`, que é layer mais externo,
    // antes de chegar aqui; um `OPTIONS` que chegou até aqui não é preflight.
    if method == Method::OPTIONS {
        return next.run(req).await;
    }

    let decisao = {
        let pedido = Pedido::de(req.headers(), req.uri());
        if matches!(method, Method::GET | Method::HEAD) {
            // Leitura HTTP fica com o CORS; só o handshake de WebSocket, que
            // o CORS não cobre, é julgado aqui.
            if handshake_websocket(req.headers()) {
                decide_leitura(&pedido, estado.scheme, &estado.allowed_origins)
            } else {
                Decisao::Passa
            }
        } else if SEM_GUARDA
            .iter()
            .any(|prefixo| req.uri().path().starts_with(prefixo))
        {
            Decisao::Passa
        } else {
            decide_escrita(&pedido, estado.scheme, &estado.allowed_origins)
        }
    };

    if decisao == Decisao::Recusa {
        // Só caminho e método vão para o log; o valor de `Origin`/`Host` não
        // sai, e o corpo é constante.
        warn!(
            path = %req.uri().path(),
            method = %method,
            "gateway: cross-origin request refused"
        );
        return (StatusCode::FORBIDDEN, CORPO_CSRF).into_response();
    }

    next.run(req).await
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use axum::Router;
    use axum::body::Body;
    use axum::http::HeaderValue;
    use axum::http::Request as HttpRequest;
    use axum::middleware::from_fn_with_state;
    use axum::routing::{any, get};
    use http_body_util::BodyExt;
    use tower::ServiceExt;

    async fn handler() -> &'static str {
        "ok"
    }

    /// As rotas mutantes reais que este guarda cobre — uma amostra
    /// representativa de cada família da superfície `/api/*` + `/v1/*` +
    /// `/chat` + `/a2a/*` + o bootstrap do admin. `any(...)` porque o que se
    /// testa aqui é o middleware, não o roteamento por método.
    const MUTANTES: &[(&str, &str)] = &[
        ("PATCH", "/api/settings"),
        ("POST", "/api/mode/select"),
        ("POST", "/api/mcp/marketplace/install"),
        ("POST", "/api/skills"),
        ("PUT", "/api/skills/x"),
        ("DELETE", "/api/skins/x"),
        ("POST", "/api/sessions"),
        ("DELETE", "/api/sessions/x"),
        ("DELETE", "/api/memory"),
        ("POST", "/v1/chat/completions"),
        ("POST", "/v1/messages"),
        ("POST", "/chat"),
        ("POST", "/a2a/tasks"),
        ("POST", "/admin/api/setup"),
        ("POST", "/admin/api/login"),
        ("POST", "/admin/api/recovery/start"),
    ];

    /// Caminhos da skip-list, com o corpo que a rota de verdade responderia.
    const PULADOS: &[&str] = &[
        "/api/learning/skills/x/rollback",
        "/api/plugins/install",
        "/webhooks/whatsapp",
    ];

    /// Leituras: não são deste guarda, salvo o handshake de WebSocket.
    const LEITURAS: &[&str] = &["/api/health", "/api/sessions", "/api/memory/recent"];
    /// Rotas de WebSocket (o handshake é um `GET` com `Upgrade: websocket`).
    const WEBSOCKETS: &[&str] = &["/ws", "/ws/parrot"];
    const HANDSHAKE: &[(&str, &str)] = &[("upgrade", "websocket"), ("connection", "Upgrade")];

    fn estado(allowed: &[&str]) -> OriginGuardState {
        OriginGuardState {
            scheme: "http",
            allowed_origins: allowed.iter().map(|s| (*s).to_string()).collect(),
        }
    }

    fn router(estado: OriginGuardState) -> Router {
        let mut router = Router::new();
        for path in LEITURAS.iter().chain(WEBSOCKETS) {
            router = router.route(path, get(handler));
        }
        for (_, path) in MUTANTES {
            router = router.route(path, any(handler));
        }
        for path in PULADOS {
            router = router.route(path, any(handler));
        }
        router.layer(from_fn_with_state(estado, cross_origin_guard))
    }

    fn request(method: &str, uri: &str, headers: &[(&str, &str)]) -> HttpRequest<Body> {
        let mut builder = HttpRequest::builder().uri(uri).method(method);
        for (nome, valor) in headers {
            builder = builder.header(*nome, *valor);
        }
        builder.body(Body::empty()).expect("request sintetico")
    }

    async fn status_corpo(router: Router, req: HttpRequest<Body>) -> (StatusCode, String) {
        let resp = router.oneshot(req).await.expect("oneshot");
        let status = resp.status();
        let body = resp
            .into_body()
            .collect()
            .await
            .expect("collect body")
            .to_bytes();
        (status, String::from_utf8_lossy(&body).into_owned())
    }

    fn headers_com(pares: &[(&str, &str)]) -> HeaderMap {
        let mut headers = HeaderMap::new();
        for (nome, valor) in pares {
            headers.insert(
                axum::http::HeaderName::from_bytes(nome.as_bytes()).expect("nome de header"),
                HeaderValue::from_str(valor).expect("valor de header"),
            );
        }
        headers
    }

    /// `Pedido` só de headers (URI sem authority, como em HTTP/1.1
    /// origin-form).
    fn pedido<'a>(headers: &'a HeaderMap, uri: &'a Uri) -> Pedido<'a> {
        Pedido::de(headers, uri)
    }

    const URI_RELATIVA: &str = "/x";

    fn ws_ok(pares: &[(&str, &str)], allowed: &[&str]) -> bool {
        let headers = headers_com(pares);
        let uri: Uri = URI_RELATIVA.parse().expect("uri");
        let allowed: Vec<String> = allowed.iter().map(|s| (*s).to_string()).collect();
        ws_upgrade_permitido(&pedido(&headers, &uri), "http", &allowed)
    }

    fn ancora(pares: &[(&str, &str)], allowed: &[&str]) -> bool {
        let headers = headers_com(pares);
        let uri: Uri = URI_RELATIVA.parse().expect("uri");
        let allowed: Vec<String> = allowed.iter().map(|s| (*s).to_string()).collect();
        ancora_ok(&pedido(&headers, &uri), &allowed)
    }

    fn declarada(pares: &[(&str, &str)], allowed: &[&str]) -> bool {
        let headers = headers_com(pares);
        let uri: Uri = URI_RELATIVA.parse().expect("uri");
        let allowed: Vec<String> = allowed.iter().map(|s| (*s).to_string()).collect();
        origem_declarada(&pedido(&headers, &uri), &allowed)
    }

    fn loopback(host: &str) -> bool {
        let headers = headers_com(&[("host", host)]);
        let uri: Uri = URI_RELATIVA.parse().expect("uri");
        host_de_loopback(&pedido(&headers, &uri))
    }

    // ── 1. a tabela das rotas mutantes reais ──────────────────────────────

    #[tokio::test]
    async fn origin_estranho_da_403_em_toda_rota_mutante() {
        let router = router(estado(&[]));
        for (metodo, path) in MUTANTES {
            let req = request(
                metodo,
                path,
                &[
                    ("host", "127.0.0.1:3888"),
                    ("origin", "http://evil.example"),
                ],
            );
            let (status, corpo) = status_corpo(router.clone(), req).await;
            assert_eq!(status, StatusCode::FORBIDDEN, "{metodo} {path}");
            assert_eq!(corpo, CORPO_CSRF, "{metodo} {path}");
        }
    }

    #[tokio::test]
    async fn sem_origin_passa_em_toda_rota_mutante() {
        // O app (Dio), o `curl` e o `garra` CLI não mandam `Origin`.
        let router = router(estado(&[]));
        for (metodo, path) in MUTANTES {
            let req = request(metodo, path, &[("host", "127.0.0.1:3888")]);
            let (status, corpo) = status_corpo(router.clone(), req).await;
            assert_eq!(status, StatusCode::OK, "{metodo} {path}");
            assert_eq!(corpo, "ok");
        }
    }

    #[tokio::test]
    async fn origin_igual_ao_host_passa_em_toda_rota_mutante() {
        // O Web Console é servido pelo próprio gateway: same-origin.
        let router = router(estado(&[]));
        for (metodo, path) in MUTANTES {
            let req = request(
                metodo,
                path,
                &[
                    ("host", "127.0.0.1:3888"),
                    ("origin", "http://127.0.0.1:3888"),
                ],
            );
            let (status, corpo) = status_corpo(router.clone(), req).await;
            assert_eq!(status, StatusCode::OK, "{metodo} {path}");
            assert_eq!(corpo, "ok");
        }
    }

    #[tokio::test]
    async fn sec_fetch_site_cross_site_sem_origin_da_403_na_escrita() {
        let router = router(estado(&[]));
        for (metodo, path) in MUTANTES {
            let req = request(
                metodo,
                path,
                &[("host", "127.0.0.1:3888"), ("sec-fetch-site", "cross-site")],
            );
            let (status, corpo) = status_corpo(router.clone(), req).await;
            assert_eq!(status, StatusCode::FORBIDDEN, "{metodo} {path}");
            assert_eq!(corpo, CORPO_CSRF);
        }
    }

    #[tokio::test]
    async fn origem_nao_web_nao_escreve() {
        // Na escrita HTTP a escotilha das origens de app NAO vale: só o
        // handshake WebSocket e a leitura a têm. `tauri://localhost` não
        // casa o esquema do transporte e morre no passo 2.
        let req = request(
            "PATCH",
            "/api/settings",
            &[("host", "localhost:3888"), ("origin", "tauri://localhost")],
        );
        let (status, _) = status_corpo(router(estado(&[])), req).await;
        assert_eq!(status, StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn options_passa_sempre() {
        // O preflight é do `CorsLayer`; um OPTIONS que chega aqui não é
        // barrado pela guarda.
        let req = request(
            "OPTIONS",
            "/api/settings",
            &[
                ("host", "127.0.0.1:3888"),
                ("origin", "http://evil.example"),
            ],
        );
        let (status, _) = status_corpo(router(estado(&[])), req).await;
        assert_eq!(status, StatusCode::OK);
    }

    // ── 1b. leitura e handshake de WebSocket ──────────────────────────────

    #[tokio::test]
    async fn leitura_http_nao_e_deste_guarda() {
        // `GET` puro passa mesmo com `Origin` estranho ou `Sec-Fetch-Site:
        // cross-site` sem `Origin` (navegação: o dono clica num link para o
        // console a partir de outro site). Quem cuida da resposta de um GET
        // cross-origin é o CORS, que sem `allowed_origins` não a entrega.
        let router = router(estado(&[]));
        for path in LEITURAS {
            for headers in [
                vec![
                    ("host", "127.0.0.1:3888"),
                    ("origin", "http://evil.example"),
                ],
                vec![("host", "127.0.0.1:3888"), ("sec-fetch-site", "cross-site")],
                vec![
                    ("host", "evil.example:3888"),
                    ("origin", "http://evil.example:3888"),
                ],
            ] {
                let req = request("GET", path, &headers);
                let (status, corpo) = status_corpo(router.clone(), req).await;
                assert_eq!(status, StatusCode::OK, "GET {path} com {headers:?}");
                assert_eq!(corpo, "ok");
            }
        }
    }

    #[tokio::test]
    async fn handshake_websocket_com_origin_estranho_da_403() {
        // O handshake é um GET, mas WebSocket não passa por CORS e a página
        // lê cada frame depois: aqui o `Origin` decide — em qualquer rota
        // com `Upgrade: websocket`, para uma rota nova nascer coberta.
        let router = router(estado(&[]));
        for path in WEBSOCKETS {
            for (host, origin) in [
                ("127.0.0.1:3888", "http://evil.example"),
                ("evil.example:3888", "http://evil.example:3888"), // rebinding
                ("127.0.0.1:3888", "null"),
            ] {
                let mut headers = vec![("host", host), ("origin", origin)];
                headers.extend_from_slice(HANDSHAKE);
                let req = request("GET", path, &headers);
                let (status, corpo) = status_corpo(router.clone(), req).await;
                assert_eq!(status, StatusCode::FORBIDDEN, "GET {path} com {origin}");
                assert_eq!(corpo, CORPO_CSRF);
            }
        }
    }

    #[tokio::test]
    async fn handshake_websocket_sem_origin_ou_legitimo_passa() {
        let router = router(estado(&[]));
        for (host, origin) in [
            ("127.0.0.1:3888", None),
            ("127.0.0.1:3888", Some("http://127.0.0.1:3888")),
            ("192.168.1.10:3888", Some("http://192.168.1.10:3888")),
            ("localhost:3888", Some("tauri://localhost")),
            ("localhost:3888", Some("http://tauri.localhost")),
            (
                "localhost:3888",
                Some("chrome-extension://abcdefghijklmnop"),
            ),
        ] {
            for path in WEBSOCKETS {
                let mut headers = vec![("host", host)];
                if let Some(origin) = origin {
                    headers.push(("origin", origin));
                }
                headers.extend_from_slice(HANDSHAKE);
                let req = request("GET", path, &headers);
                let (status, _) = status_corpo(router.clone(), req).await;
                assert_eq!(status, StatusCode::OK, "GET {path} com Origin {origin:?}");
            }
        }
    }

    #[test]
    fn handshake_websocket_e_case_insensitive_e_aceita_lista() {
        assert!(handshake_websocket(&headers_com(&[(
            "upgrade",
            "websocket"
        )])));
        assert!(handshake_websocket(&headers_com(&[(
            "upgrade",
            "WebSocket"
        )])));
        assert!(handshake_websocket(&headers_com(&[(
            "upgrade",
            "h2c, websocket"
        )])));
        assert!(!handshake_websocket(&headers_com(&[("upgrade", "h2c")])));
        assert!(!handshake_websocket(&HeaderMap::new()));
    }

    // ── 2. a âncora anti-rebinding ────────────────────────────────────────

    #[tokio::test]
    async fn ancora_por_host() {
        // Cada caso é `Origin == Host` (casam entre si, passam o passo 2) —
        // só a âncora separa o console legítimo do alias do atacante.
        for (host, allowed, esperado) in [
            // IP de LAN: o Web Console no tablet. `host_de_loopback` mataria.
            ("192.168.1.10:3888", &[][..], StatusCode::OK),
            ("127.0.0.1:3888", &[][..], StatusCode::OK),
            ("[::1]:3888", &[][..], StatusCode::OK),
            ("localhost:3888", &[][..], StatusCode::OK),
            ("LOCALHOST:3888", &[][..], StatusCode::OK),
            // `*.localhost` resolve para loopback por definição (RFC 6761).
            ("tauri.localhost:3888", &[][..], StatusCode::OK),
            // Nome DNS não listado: rebinding.
            ("evil.example:3888", &[][..], StatusCode::FORBIDDEN),
            (
                "localhost.evil.example:3888",
                &[][..],
                StatusCode::FORBIDDEN,
            ),
            // O mesmo nome, declarado pelo dono: passa.
            (
                "meu.dominio:3888",
                &["http://meu.dominio:3888"][..],
                StatusCode::OK,
            ),
            // A entrada listada sem porta continua cobrindo: a âncora
            // compara só o host.
            (
                "meu.dominio:3888",
                &["http://meu.dominio"][..],
                StatusCode::OK,
            ),
            // Barra final na entrada não a invalida (o mesmo que
            // `origem_declarada` tolera).
            (
                "meu.dominio:3888",
                &["http://meu.dominio/"][..],
                StatusCode::OK,
            ),
            // Nome listado ≠ nome do pedido.
            (
                "evil.example:3888",
                &["http://meu.dominio:3888"][..],
                StatusCode::FORBIDDEN,
            ),
        ] {
            let origin = format!("http://{host}");
            let req = request(
                "PATCH",
                "/api/settings",
                &[("host", host), ("origin", &origin)],
            );
            let (status, _) = status_corpo(router(estado(allowed)), req).await;
            assert_eq!(status, esperado, "Host {host} com allowed {allowed:?}");
        }
    }

    #[test]
    fn ancora_fail_closed_sem_host_ou_com_host_invalido() {
        for host in ["[::1]lixo", "127.0.0.1:0"] {
            assert!(
                !ancora(&[("host", host)], &[]),
                "Host fora da gramática aceito: {host}"
            );
        }
        assert!(!ancora(&[], &[]), "sem Host tem de falhar");
    }

    // ── 2b. HTTP/2: sem header Host, a authority vem da URI ────────────────

    #[tokio::test]
    async fn http2_sem_host_usa_a_authority_da_uri() {
        // Em h2 o navegador manda `:authority`, e o hyper a guarda na URI do
        // request — não há header `Host`. O console do próprio gateway tem
        // de continuar passando, e o cross-origin tem de continuar caindo.
        let router = router(estado(&[]));
        let req = request(
            "PATCH",
            "http://127.0.0.1:3888/api/settings",
            &[("origin", "http://127.0.0.1:3888")],
        );
        let (status, _) = status_corpo(router.clone(), req).await;
        assert_eq!(status, StatusCode::OK, "same-origin em h2 recusado");

        let req = request(
            "PATCH",
            "http://127.0.0.1:3888/api/settings",
            &[("origin", "http://evil.example")],
        );
        let (status, corpo) = status_corpo(router.clone(), req).await;
        assert_eq!(status, StatusCode::FORBIDDEN);
        assert_eq!(corpo, CORPO_CSRF);

        // Sem Host E sem authority: fail-closed, como antes.
        let req = request(
            "PATCH",
            "/api/settings",
            &[("origin", "http://127.0.0.1:3888")],
        );
        let (status, _) = status_corpo(router, req).await;
        assert_eq!(status, StatusCode::FORBIDDEN);
    }

    // ── 2c. a escotilha do reverse proxy ──────────────────────────────────

    #[tokio::test]
    async fn origem_declarada_passa_mesmo_com_esquema_diferente() {
        // O perfil "reverse proxy com dominio proprio": o proxy termina TLS,
        // o navegador manda `Origin: https://…`, e o gateway por baixo fala
        // `http`. Sem a escotilha, nao haveria config que salvasse — nem
        // `gateway.api_key`, que nao dispensa a origem certa.
        let req = request(
            "PATCH",
            "/api/settings",
            &[("host", "meu.dominio"), ("origin", "https://meu.dominio")],
        );
        let (status, _) = status_corpo(router(estado(&["https://meu.dominio"])), req).await;
        assert_eq!(status, StatusCode::OK, "origem declarada tem de passar");

        // Barra final tolerada dos dois lados.
        let req = request(
            "PATCH",
            "/api/settings",
            &[("host", "meu.dominio"), ("origin", "https://meu.dominio")],
        );
        let (status, _) = status_corpo(router(estado(&["https://meu.dominio/"])), req).await;
        assert_eq!(status, StatusCode::OK);
    }

    #[tokio::test]
    async fn a_escotilha_nao_e_match_por_prefixo() {
        // O vetor obvio contra uma comparacao frouxa: o atacante registra um
        // dominio que TEM o declarado como prefixo.
        for origin in [
            "https://meu.dominio.evil.example",
            "https://evil.example/https://meu.dominio",
            "https://meu.dominio:8443",
        ] {
            let req = request(
                "PATCH",
                "/api/settings",
                &[("host", "evil.example"), ("origin", origin)],
            );
            let (status, corpo) = status_corpo(router(estado(&["https://meu.dominio"])), req).await;
            assert_eq!(status, StatusCode::FORBIDDEN, "escotilha frouxa: {origin}");
            assert_eq!(corpo, CORPO_CSRF);
        }
    }

    #[test]
    fn lista_vazia_nao_declara_nada() {
        // Guarda contra o `any()` de lista vazia virar `true` num refactor, e
        // contra uma entrada vazia casar com `Origin` vazio.
        assert!(!declarada(&[("origin", "https://meu.dominio")], &[]));
        assert!(!declarada(&[("host", "127.0.0.1:3888")], &[""]));
    }

    // ── 3. a skip-list ────────────────────────────────────────────────────

    #[tokio::test]
    async fn skip_list_nao_e_dupla_guardada() {
        // Cross-origin nos caminhos da skip-list: o guarda genérico não
        // responde — quem responde é a guarda própria daquele caminho (aqui,
        // o handler de mentira). O corpo prova que o 403 não veio daqui.
        let router = router(estado(&[]));
        for path in PULADOS {
            let req = request(
                "POST",
                path,
                &[
                    ("host", "127.0.0.1:3888"),
                    ("origin", "http://evil.example"),
                ],
            );
            let (status, corpo) = status_corpo(router.clone(), req).await;
            assert_eq!(status, StatusCode::OK, "{path} foi duplo-guardado");
            assert_eq!(corpo, "ok");
        }
    }

    #[test]
    fn a_skip_list_nao_cresce_sem_alguem_notar() {
        // Anti-drift. A guarda é **opt-out**: rota mutante nova nasce coberta
        // sem ninguém lembrar de registrá-la — a única forma de nascer
        // desprotegida é entrar nesta lista. Então é a lista que precisa de
        // trava, e não a cobertura. Mexeu aqui, justifique no PR: cada
        // entrada tem de ter guarda própria mais estrita (senão o buraco do
        // #1182 volta por ela). `/admin/` saiu na revisão do #1182: o
        // bootstrap do admin (`/admin/api/setup`) não tinha guarda nenhuma.
        assert_eq!(
            SEM_GUARDA,
            ["/api/learning/", "/api/plugins/", "/webhooks/"],
            "a skip-list mudou — cada entrada precisa de guarda propria"
        );
    }

    // ── 4. o handshake de WebSocket ───────────────────────────────────────

    #[test]
    fn ws_upgrade_permitido_decide_por_origin() {
        // Sem Origin: cliente não-navegador (app, CLI, curl).
        assert!(ws_ok(&[("host", "127.0.0.1:3888")], &[]));
        // Origin de outra origem: o `new WebSocket` da página do atacante.
        assert!(!ws_ok(
            &[
                ("host", "127.0.0.1:3888"),
                ("origin", "http://evil.example")
            ],
            &[]
        ));
        // `Origin: null` (iframe sandbox, redirect): fail-closed.
        assert!(!ws_ok(
            &[("host", "127.0.0.1:3888"), ("origin", "null")],
            &[]
        ));
        // Same-origin: o webchat servido pelo próprio gateway.
        assert!(ws_ok(
            &[
                ("host", "127.0.0.1:3888"),
                ("origin", "http://127.0.0.1:3888")
            ],
            &[]
        ));
        // Rebinding: `Origin == Host`, nome DNS não listado.
        assert!(!ws_ok(
            &[
                ("host", "evil.example:3888"),
                ("origin", "http://evil.example:3888")
            ],
            &[]
        ));
        // O mesmo nome, declarado pelo dono.
        assert!(ws_ok(
            &[
                ("host", "meu.dominio:3888"),
                ("origin", "http://meu.dominio:3888")
            ],
            &["http://meu.dominio:3888"]
        ));
    }

    #[test]
    fn ws_upgrade_aceita_a_webview_do_desktop() {
        // O `ws.js` do Garra Desktop conecta em `ws://localhost:3888/ws/parrot`
        // de uma webview Tauri. Linux/macOS: `tauri://localhost`; Windows
        // (WebView2): `http://tauri.localhost`. Nenhuma página web consegue
        // apresentar essas origens.
        for origin in ORIGENS_TAURI {
            assert!(
                ws_ok(&[("host", "localhost:3888"), ("origin", origin)], &[]),
                "webview do desktop recusada: {origin}"
            );
            let com_barra = format!("{origin}/");
            assert!(ws_ok(
                &[("host", "localhost:3888"), ("origin", &com_barra)],
                &[]
            ));
        }
        // Esquema de app que não é o do Tauri (extensão de navegador):
        // também não é página web.
        assert!(ws_ok(
            &[
                ("host", "localhost:3888"),
                ("origin", "moz-extension://8a1b2c3d")
            ],
            &[]
        ));
        // Mas `http://tauri.localhost` com porta ou sufixo não é a webview.
        for falso in [
            "http://tauri.localhost:8080",
            "http://tauri.localhost.evil.example",
            "http://evil.example/tauri://localhost",
        ] {
            assert!(
                !ws_ok(&[("host", "127.0.0.1:3888"), ("origin", falso)], &[]),
                "origem falsa aceita: {falso}"
            );
        }
    }

    // ── 5. esquema efetivo e lista de origens ─────────────────────────────

    #[test]
    fn esquema_efetivo_segue_a_feature_tls() {
        let sem_tls = GatewayConfig::default();
        assert_eq!(esquema_efetivo(&sem_tls), "http");
        let com_paths = GatewayConfig {
            tls_cert_path: Some("cert.pem".into()),
            tls_key_path: Some("key.pem".into()),
            ..Default::default()
        };
        // Sem a feature, `server.rs` avisa e serve HTTP puro — e a guarda
        // tem de comparar contra `http`, senão recusa o console inteiro.
        let esperado = if cfg!(feature = "tls") {
            "https"
        } else {
            "http"
        };
        assert_eq!(esquema_efetivo(&com_paths), esperado);
        // Só um dos dois paths nunca é TLS.
        let so_cert = GatewayConfig {
            tls_cert_path: Some("cert.pem".into()),
            ..Default::default()
        };
        assert_eq!(esquema_efetivo(&so_cert), "http");
    }

    #[test]
    fn origens_validas_descarta_curinga_e_vazio() {
        let cfg = GatewayConfig {
            allowed_origins: vec![
                "*".into(),
                "".into(),
                "  ".into(),
                "https://meu.dominio/".into(),
                "http://localhost:3888".into(),
            ],
            ..Default::default()
        };
        assert_eq!(
            origens_validas(&cfg),
            vec![
                "https://meu.dominio".to_string(),
                "http://localhost:3888".to_string()
            ]
        );
    }

    // ── 6. gramática de authority (migrada do `learning_auth`, que agora
    //    importa estas primitivas daqui) ─────────────────────────────────

    #[test]
    fn parse_authority_rejeita_fora_da_gramatica() {
        // A gramática inteira num só lugar: o `None` de cada lixo é o que o
        // passo 1, a âncora e a comparação de authority têm em comum.
        for lixo in [
            "[::1]lixo",
            "[::1]:3888:80",
            "host:porta:porta",
            "127.0.0.1:99999",
            "127.0.0.1:0",
            "usuario@127.0.0.1",
            "127.0.0.1:3888/qualquer-caminho",
            "127.0.0.1:3888?q=1",
            "127.0.0.1:3888#frag",
            ":3888",
            "localhost:",
            "",
            "::1",
        ] {
            assert!(
                parse_authority(lixo).is_none(),
                "authority aceita fora da gramática: {lixo}"
            );
        }
        assert_eq!(parse_authority("localhost"), Some(("localhost", None)));
        assert_eq!(
            parse_authority("localhost:3888"),
            Some(("localhost", Some(3888)))
        );
        assert_eq!(parse_authority("[::1]"), Some(("::1", None)));
        assert_eq!(parse_authority("[::1]:3888"), Some(("::1", Some(3888))));
    }

    #[test]
    fn host_de_loopback_rejeita_sobra_depois_do_colchete() {
        // O buraco antigo: o corte no primeiro `]` fazia `[::1]lixo` contar
        // como loopback na âncora. Agora o `Host` passa pelo mesmo parse
        // estrito do passo 1 e não parseia — fail-closed.
        assert!(!loopback("[::1]lixo"));
        for bom in [
            "127.0.0.1:3888",
            "localhost:3888",
            "LOCALHOST",
            "[::1]:3888",
            "[::1]",
        ] {
            assert!(loopback(bom), "loopback negado: {bom}");
        }
        for ruim in ["evil.example:3888", "localhost.evil.com", "192.168.0.1"] {
            assert!(!loopback(ruim), "não-loopback aceito: {ruim}");
        }
    }
}
