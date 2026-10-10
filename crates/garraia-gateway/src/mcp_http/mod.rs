//! #1513 — o gateway como **servidor** MCP, em `POST /mcp` (Streamable HTTP).
//!
//! Antes disto o Garra falava MCP em duas direcoes incompletas: como *cliente*
//! de servidores de terceiros (`crate::mcp`, `McpTransportType::StreamableHttp`)
//! e como *servidor* por stdio com uma tool so (`garra mcp-server`, tool
//! `garra_ask`). Faltava a combinacao que um orquestrador externo precisa —
//! servidor, por HTTP —, e sem ela nao havia como apontar o Paperclip (ou
//! qualquer host MCP que so aceite URL) para este Garra.
//!
//! ## As seis tools
//!
//! | Tool | O que da | Escrita? |
//! |---|---|---|
//! | `garra_status` | versao, uptime, canais, conversas em memoria | nao |
//! | `garra_list_chats` | as conversas vivas, por canal | nao |
//! | `garra_read_history` | ultimas N mensagens, com segredos redigidos | nao |
//! | `garra_pair_status` | pareamento e allowlist dos canais | nao |
//! | `garra_send_message` | mandar mensagem num canal real | **sim** |
//! | `garra_ask` | pergunta ao LLM do Garra (#1612) | gasta inferencia |
//!
//! `garra_ask` (#1612) chegou a esta ponte depois de a #1513 deixa-la de fora.
//! Ela continua existindo no servidor stdio (`garra mcp-server`), com o mesmo
//! envelope `garra.ask.v1`; as duas superficies compartilham o nucleo em
//! `garraia-ask`. Aqui ela so aparece com `gateway.mcp_http.allow_ask`, e gasta a
//! chave de provider do dono — por isso tem politica, lista de modelos e teto de
//! chamadas proprios, separados dos de envio.
//!
//! ## Quatro travas, em camadas diferentes, de proposito
//!
//! 1. **A rota so existe com `gateway.mcp_http.enabled`** (default `false`).
//!    Desligada, `/mcp` nao e registrada e um pedido cai no 404 do fallback —
//!    nao num 403 que confirmaria que a ponte existe.
//! 2. **A rota exige `gateway.api_key`.** Nao "e coberta pelo gate quando ha
//!    chave": ela **recusa subir sem chave**. O gate do `crate::gateway_auth` e
//!    passa-direto com `api_key: None` (invariante dele, e o certo para
//!    `/api/*`), so que esta ponte entrega a lista de conversas do dono e o
//!    historico delas a quem chamar. "Quem alcanca a porta e o dono" e aceitavel
//!    para o console local; nao e aceitavel como unica prova de identidade de um
//!    orquestrador. Entao a precondicao virou explicita, e o boot avisa alto
//!    quando ela nao e satisfeita ([`Montagem::SemCredencial`]).
//! 3. **Escrita exige um segundo interruptor E a allowlist de destino.** Ver
//!    [`politica`].
//! 4. **Inferencia (`garra_ask`) exige `allow_ask`, a lista de modelos e um teto
//!    proprio por minuto.** Sem a lista, so o modelo default do projeto passa.
//!    Nenhum dos tres interruptores destrava o outro.
//!
//! Alem das tres, `/mcp` herda de graca o que o router ja tem: a guarda
//! anti-CSRF do [`crate::origin_guard`] (um `POST` de dentro de um navegador
//! carrega `Origin`, e morre ali; um cliente MCP nao carrega, e passa), o rate
//! limit do `gateway.rate_limit`, e a validacao de `Host` do proprio rmcp, cujo
//! default so aceita loopback — anti-DNS-rebinding, e alinhado com o "bind
//! permanece 127.0.0.1" da spec. A consequencia e deliberada: um gateway em
//! `0.0.0.0` continua nao servindo MCP para a LAN.

pub mod ferramentas;
pub mod handler;
pub mod politica;

use std::sync::Arc;

use axum::Router;
use rmcp::transport::streamable_http_server::session::local::LocalSessionManager;
use rmcp::transport::streamable_http_server::{StreamableHttpServerConfig, StreamableHttpService};

use crate::channel_send::SendBudget;
use crate::push_channels::PushMounted;
use crate::state::SharedState;

pub use handler::ManipuladorMcpHttp;
pub use politica::PoliticaMcpHttp;

/// O caminho da ponte. Uma constante porque tres lugares dependem dela: o
/// router aqui, o gate de `api_key` em [`crate::gateway_auth`] e a doc.
pub const ROTA: &str = "/mcp";

/// O que o boot decide sobre montar a ponte.
///
/// Enum e nao `bool` porque as duas maneiras de nao montar pedem tratamento
/// diferente: uma e o default silencioso, a outra e um erro de configuracao que
/// tem de aparecer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Montagem {
    /// `gateway.mcp_http.enabled` e `false`. O default, e nao ha nada a dizer.
    Desligada,
    /// Ligada, porem sem `gateway.api_key`. **Nao monta**, e avisa: o operador
    /// pediu a ponte e vai encontrar 404, entao ele precisa saber por que.
    SemCredencial,
    /// Monta.
    Montar,
}

/// O contrato de transporte da ponte: **sem sessao, resposta em JSON**.
///
/// Os dois defaults do rmcp que esta funcao troca, e por que:
///
/// - `legacy_session_mode: false` — as cinco tools sao stateless (cada chamada
///   le o `AppState` de agora e responde), entao uma sessao MCP so guardaria
///   estado que ninguem usa: mais um id para expirar, restaurar e vazar. A spec
///   ja caminha nessa direcao — a SEP-2567 removeu sessoes da versao
///   `2026-07-28` —, e sessao e opcional para o cliente, entao um host que
///   esperava `Mcp-Session-Id` continua funcionando sem ele.
/// - `json_response: true` — sem sessao, a resposta de um pedido simples e um
///   `application/json` em vez de um frame SSE. E o que faz `curl -d @pedido.json`
///   ser um jeito honesto de conferir a ponte, e o que deixa o teste de contrato
///   assertar JSON em vez de parsear `data:`. O rmcp ainda cai para
///   `text/event-stream` sozinho se o handler emitir notificacao antes da
///   resposta final — nada se perde.
///
/// `allowed_hosts` fica no default do rmcp (**so loopback**). E a validacao
/// anti-DNS-rebinding do proprio SDK, e ela coincide com o "bind permanece
/// 127.0.0.1" da spec da #1513. A consequencia e deliberada e vale repetir: num
/// gateway em `0.0.0.0`, `/mcp` continua nao atendendo a LAN.
fn configuracao_do_transporte() -> StreamableHttpServerConfig {
    StreamableHttpServerConfig::default()
        .with_legacy_session_mode(false)
        .with_json_response(true)
}

/// A decisao, pura, a partir da secao `gateway:`.
///
/// Separada de [`build_mcp_http_routes`] para que a precondicao de credencial
/// seja testavel sem montar router nenhum — e a trava cuja regressao seria
/// silenciosa (a ponte subiria, aberta, e tudo continuaria "funcionando").
pub fn decidir_montagem(gateway: &garraia_config::GatewayConfig) -> Montagem {
    if !gateway.mcp_http.enabled {
        return Montagem::Desligada;
    }
    if !gateway.api_key_configurada() {
        return Montagem::SemCredencial;
    }
    Montagem::Montar
}

/// O sub-router da ponte, ou um `Router` vazio quando ela nao sobe.
///
/// `Router` vazio e nao `Option<Router>` para o `build_router` seguir sendo uma
/// expressao so: um `.merge(Router::new())` nao registra rota nenhuma, e `/mcp`
/// volta a ser um caminho inexistente como qualquer outro.
///
/// **Montado antes dos `.layer()` do `build_router`**, e nao depois: e assim que
/// a ponte fica por dentro do gate de `api_key`, da guarda anti-CSRF e do rate
/// limit, em vez de por fora deles.
pub fn build_mcp_http_routes(state: SharedState, push: PushMounted) -> Router {
    match decidir_montagem(&state.config.gateway) {
        Montagem::Desligada => Router::new(),
        Montagem::SemCredencial => {
            tracing::warn!(
                "gateway.mcp_http.enabled esta ligado, mas gateway.api_key nao esta \
                 configurada: a ponte MCP em {ROTA} NAO subiu. Ela entrega a lista de \
                 conversas e o historico delas, entao exige credencial. Configure \
                 `gateway.api_key` (ou GARRAIA_GATEWAY_API_KEY) e reinicie."
            );
            Router::new()
        }
        Montagem::Montar => {
            let politica = PoliticaMcpHttp::da_config(&state.config);
            tracing::info!(
                rota = ROTA,
                envio_liberado = politica.anuncia_envio(),
                destinos_liberados = politica.destinos_liberados(),
                ask_liberado = politica.anuncia_ask(),
                "ponte MCP Streamable HTTP montada (host: so loopback)"
            );
            // Um orcamento para a ponte inteira, criado aqui e nao no
            // `service_factory`: o factory roda por sessao MCP, e um teto por
            // sessao e um teto que o chamador zera abrindo outra sessao.
            let orcamento = Arc::new(SendBudget::default());
            // #1612: o teto de `garra_ask` e lido no boot, como o de envio e uma
            // constante. Mudar `ask_budget_per_minute` pede reinicio. `with_max`
            // ja garante o minimo de 1.
            let orcamento_ask = Arc::new(SendBudget::with_max(
                state.config.gateway.mcp_http.ask_budget_per_minute,
            ));
            let servico = StreamableHttpService::new(
                move || {
                    Ok(ManipuladorMcpHttp::novo(
                        &state,
                        push,
                        orcamento.clone(),
                        orcamento_ask.clone(),
                    ))
                },
                Arc::new(LocalSessionManager::default()),
                configuracao_do_transporte(),
            );
            Router::new().route_service(ROTA, servico)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use garraia_config::GatewayConfig;

    fn gateway(enabled: bool, chave: Option<&str>) -> GatewayConfig {
        let mut g = GatewayConfig {
            api_key: chave.map(str::to_string),
            ..Default::default()
        };
        g.mcp_http.enabled = enabled;
        g
    }

    /// O default da instalacao nao tem ponte.
    #[test]
    fn default_nao_monta() {
        assert_eq!(
            decidir_montagem(&GatewayConfig::default()),
            Montagem::Desligada
        );
    }

    /// A trava que importa: ligar a ponte sem credencial nao a abre.
    #[test]
    fn ligada_sem_api_key_nao_monta() {
        assert_eq!(
            decidir_montagem(&gateway(true, None)),
            Montagem::SemCredencial
        );
    }

    /// `api_key: "   "` e o mesmo que ausente — a normalizacao e a do #1241,
    /// compartilhada com o gate e com o `garra config check`, para as tres
    /// superficies nao divergirem sobre a mesma chave em branco.
    #[test]
    fn api_key_em_branco_nao_conta_como_credencial() {
        assert_eq!(
            decidir_montagem(&gateway(true, Some("   "))),
            Montagem::SemCredencial
        );
    }

    #[test]
    fn ligada_com_api_key_monta() {
        assert_eq!(
            decidir_montagem(&gateway(true, Some("uma-credencial"))),
            Montagem::Montar
        );
    }

    /// Credencial sem o interruptor tambem nao monta: a chave nao liga a ponte,
    /// so a habilita. Quem quer a ponte pede a ponte.
    #[test]
    fn api_key_sozinha_nao_liga_a_ponte() {
        assert_eq!(
            decidir_montagem(&gateway(false, Some("uma-credencial"))),
            Montagem::Desligada
        );
    }
}
