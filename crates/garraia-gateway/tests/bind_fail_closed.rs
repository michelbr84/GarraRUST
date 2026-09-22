//! #1261 (decisao A): `GatewayServer::run` recusa um bind nao-loopback sem
//! credencial de gateway ANTES de ligar o socket.
//!
//! Teste de efeito, nao de string: `run()` tem de voltar `Err` rapido (sem
//! subir MCP, canais nem listener), e a porta pedida tem de continuar livre
//! depois — ninguem chegou a escutar nela.
//!
//! Os casos que SOBEM (loopback, credencial, opt-out) sao provados sobre a
//! mesma funcao que `run()` chama (`server::refuse_exposed_bind`): subir o
//! gateway inteiro num teste exige Postgres e o resto do boot (ver o
//! `#[ignore]` de `gateway_integration.rs`).

use std::net::TcpListener;
use std::time::Duration;

use garraia_config::AppConfig;
use garraia_gateway::GatewayServer;

fn porta_livre() -> u16 {
    TcpListener::bind("0.0.0.0:0")
        .expect("porta efemera")
        .local_addr()
        .expect("local_addr")
        .port()
}

fn config_em(host: &str, port: u16) -> AppConfig {
    let mut config = AppConfig::default();
    config.gateway.host = host.into();
    config.gateway.port = port;
    config.gateway.api_key = None;
    config.gateway.api_key_env = None;
    config
}

#[tokio::test]
async fn run_recusa_bind_exposto_sem_credencial_e_nada_escuta() {
    for host in ["0.0.0.0", "[::]"] {
        let porta = porta_livre();
        let resultado = tokio::time::timeout(
            Duration::from_secs(10),
            GatewayServer::new(config_em(host, porta)).run(),
        )
        .await
        .unwrap_or_else(|_| panic!("{host}: a recusa tem de ser imediata, nao servir"));

        let err = resultado.expect_err("bind exposto sem credencial nao sobe");
        let msg = err.to_string();
        assert!(msg.contains("refusing to start"), "{host}: {msg}");

        // Efeito: ninguem escutou na porta pedida.
        TcpListener::bind(("0.0.0.0", porta))
            .unwrap_or_else(|e| panic!("{host}: a porta {porta} tinha de seguir livre, mas: {e}"));
    }
}

/// TLS nao isenta: TLS sem credencial continua aberto para quem alcanca a
/// porta, entao continua recusado.
#[tokio::test]
async fn run_recusa_bind_exposto_com_tls_sem_credencial() {
    let porta = porta_livre();
    let mut config = config_em("0.0.0.0", porta);
    config.gateway.tls_cert_path = Some("/nao/existe/cert.pem".into());
    config.gateway.tls_key_path = Some("/nao/existe/key.pem".into());
    let resultado = tokio::time::timeout(Duration::from_secs(10), GatewayServer::new(config).run())
        .await
        .expect("recusa imediata");
    let err = resultado.expect_err("TLS sem credencial recusa");
    assert!(err.to_string().contains("refusing to start"), "{err}");
}

#[test]
fn os_casos_que_sobem_passam_pela_mesma_regra() {
    use garraia_gateway::server::refuse_exposed_bind;

    // Loopback sem credencial: o laptop.
    assert!(refuse_exposed_bind(&config_em("127.0.0.1", 0).gateway).is_ok());

    // Exposto com credencial no arquivo.
    let mut c = config_em("0.0.0.0", 0);
    c.gateway.api_key = Some("uma-credencial-longa".into());
    assert!(refuse_exposed_bind(&c.gateway).is_ok());

    // Exposto com a credencial de GARRAIA_GATEWAY_API_KEY.
    let mut c = config_em("0.0.0.0", 0);
    c.gateway.api_key_env = garraia_config::auth::gateway_api_key_de(Some("da-env".into()));
    assert!(refuse_exposed_bind(&c.gateway).is_ok());

    // Exposto com o opt-out explicito do arquivo.
    let mut c = config_em("0.0.0.0", 0);
    c.gateway.allow_unauthenticated_network_bind = true;
    assert!(refuse_exposed_bind(&c.gateway).is_ok());
}
