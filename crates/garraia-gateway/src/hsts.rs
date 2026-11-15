//! HSTS — `Strict-Transport-Security` em todas as respostas do gateway (#1895,
//! ROADMAP §5.1).
//!
//! O gateway já manda `Content-Security-Policy`, `X-Content-Type-Options`,
//! `X-Frame-Options` e `Referrer-Policy` nas respostas do admin
//! (`admin::middleware::security_headers`), mas não mandava HSTS em lugar
//! nenhum: quem servia https — por TLS nativo (`feature tls` +
//! `gateway.tls_cert_path`/`tls_key_path`) ou atrás de um proxy TLS — não
//! instruía o navegador a continuar em https, e o primeiro pedido em `http://`
//! (link antigo, digitação, downgrade em rede hostil) era servido em claro.
//!
//! # Quando o header sai
//!
//! Dois sinais, porque os dois transportes reais do projeto têm esquema
//! diferente:
//!
//! - **TLS nativo neste processo**: `origin_guard::esquema_efetivo` já é a fonte
//!   única dessa decisão (`cfg!(feature = "tls")` + cert **e** chave
//!   configurados) e é dela que o guard de `Origin` e o CORS partem. Duplicar o
//!   `cfg!` aqui daria uma segunda régua para o mesmo fato.
//! - **Proxy TLS na frente**: o processo fala http e o único sinal do esquema
//!   original é `X-Forwarded-Proto`. O primeiro valor da lista vence
//!   (`https, http` = o cliente falou https com o proxy), comparação
//!   case-insensitive, como o resto do parsing de header do repo.
//!
//! Instalação default (`127.0.0.1:3888`, sem TLS, sem proxy) não recebe header
//! nenhum: o comportamento atual é preservado byte a byte — é o que o teste de
//! router real trava.
//!
//! # O que o header NÃO tem, de propósito
//!
//! Sem `includeSubDomains` e sem `preload`. Instalação self-hosted não controla
//! o domínio inteiro: `includeSubDomains` forçaria https em qualquer outro
//! serviço em http do mesmo domínio, e `preload` é irreversível na prática
//! (lista embutida nos navegadores). Um ano de `max-age` sem os dois é o
//! hardening que o roadmap pede, com raio de dano limitado a este host.
//!
//! Mentir o `X-Forwarded-Proto` não ganha nada para quem mente: o header só
//! vale para o navegador do próprio remetente, e **nenhuma decisão de
//! autorização** neste crate lê esse sinal.

use axum::body::Body;
use axum::extract::State;
use axum::http::{HeaderValue, Request, header::STRICT_TRANSPORT_SECURITY};
use axum::middleware::Next;
use axum::response::Response;

/// Valor do header: um ano de `max-age`, sem `includeSubDomains`/`preload` —
/// o motivo está no doc do módulo.
pub const VALOR: &str = "max-age=31536000";

/// Nome do cabeçalho do proxy que informa o esquema original do pedido.
const X_FORWARDED_PROTO: &str = "x-forwarded-proto";

/// Estado do middleware: se **este** processo serve TLS nativo.
///
/// Vem do boot (`build_router`), não do pedido: o transporte não muda no meio
/// da vida do processo.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Hsts {
    servindo_tls: bool,
}

impl Hsts {
    /// Constrói o estado a partir do transporte efetivo do processo.
    pub fn novo(servindo_tls: bool) -> Self {
        Self { servindo_tls }
    }

    /// `true` quando o processo serve https por TLS nativo.
    pub fn servindo_tls(&self) -> bool {
        self.servindo_tls
    }
}

/// O pedido chegou por https segundo o proxy? Primeiro valor da lista, `trim`,
/// case-insensitive. Qualquer outro valor (ausente, `http`, vazio, lixo) é
/// `false` — fail-closed: header a mais num transporte duvidoso não se
/// justifica.
pub fn encaminhado_https(valor: Option<&str>) -> bool {
    valor
        .and_then(|v| v.split(',').next())
        .map(|v| v.trim().eq_ignore_ascii_case("https"))
        .unwrap_or(false)
}

/// Decisão pura: o header sai quando o transporte efetivo é https, por TLS
/// nativo **ou** por proxy declarando `X-Forwarded-Proto: https`.
pub fn valor(servindo_tls: bool, encaminhado: Option<&str>) -> Option<HeaderValue> {
    if servindo_tls || encaminhado_https(encaminhado) {
        return Some(HeaderValue::from_static(VALOR));
    }
    None
}

/// Middleware: acrescenta HSTS a **toda** resposta do router quando o
/// transporte efetivo é https — inclusive respostas de erro (401/403/429), que
/// também não podem ser servidas em claro.
pub async fn hsts(State(estado): State<Hsts>, req: Request<Body>, next: Next) -> Response {
    let encaminhado = req
        .headers()
        .get(X_FORWARDED_PROTO)
        .and_then(|v| v.to_str().ok())
        .map(str::to_string);
    let mut resposta = next.run(req).await;
    if let Some(valor) = valor(estado.servindo_tls, encaminhado.as_deref()) {
        resposta
            .headers_mut()
            .insert(STRICT_TRANSPORT_SECURITY, valor);
    }
    resposta
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A tabela da decisão. Cada linha é um transporte real do projeto.
    #[test]
    fn tabela_do_transporte() {
        // (servindo_tls, x-forwarded-proto, esperado)
        let casos: &[(bool, Option<&str>, bool)] = &[
            // default de instalação: http puro, sem proxy → nada muda
            (false, None, false),
            (false, Some("http"), false),
            (false, Some(""), false),
            (false, Some("lixo"), false),
            // TLS nativo → sai, independente do proxy
            (true, None, true),
            (true, Some("http"), true),
            // proxy TLS → sai
            (false, Some("https"), true),
            (false, Some("HTTPS"), true),
            (false, Some("  https  "), true),
            // lista: o primeiro valor é o do cliente
            (false, Some("https, http"), true),
            (false, Some("http, https"), false),
        ];

        for (servindo_tls, encaminhado, esperado) in casos {
            let obtido = valor(*servindo_tls, *encaminhado).is_some();
            assert_eq!(
                obtido, *esperado,
                "servindo_tls={servindo_tls} xfp={encaminhado:?}"
            );
        }
    }

    /// O valor emitido é o combinado: um ano, sem `includeSubDomains`/`preload`.
    #[test]
    fn valor_do_header() {
        let header = valor(true, None).expect("https tem header");
        assert_eq!(header.to_str().unwrap(), VALOR);
        assert_eq!(VALOR, "max-age=31536000");
        assert!(!VALOR.contains("includeSubDomains"));
        assert!(!VALOR.contains("preload"));
    }

    /// O estado carrega o transporte do boot sem reinterpretação.
    #[test]
    fn estado_reflete_o_boot() {
        assert!(Hsts::novo(true).servindo_tls());
        assert!(!Hsts::novo(false).servindo_tls());
    }
}
