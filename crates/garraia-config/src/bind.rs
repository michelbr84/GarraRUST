//! O bind do gateway: de onde ele vem e quando o boot tem de recusar (#1261).
//!
//! # Precedencia
//!
//! `garraia start` e `garraia restart` ligam o socket em **flag > env >
//! default do clap** (`--host`/`HOST`, `--port`/`PORT`, `127.0.0.1:3888`).
//! `gateway.host`/`gateway.port` do arquivo **nao** entram: o arg do clap
//! sempre traz um valor e o `main.rs` o escreve por cima da config. As chaves
//! do arquivo estao deprecadas (ninguem as le para ligar o socket).
//!
//! Os comandos cliente (`status`, `stop`, `doctor`, `admin`) precisam do
//! mesmo endereco para achar o gateway, e por isso usam
//! [`bind_do_ambiente`] + [`host_para_cliente`] — e nao o arquivo, que pode
//! dizer uma porta que o gateway nunca abriu.
//!
//! # Recusa
//!
//! [`decidir`] e a regra, pura: um bind em que **qualquer** endereco
//! resolvido nao e loopback, sem credencial de gateway e sem o opt-out
//! explicito `gateway.allow_unauthenticated_network_bind`, e recusado. TLS
//! nao isenta: TLS sem credencial continua aberto para quem alcanca a porta.
//! [`verificar`] resolve o host (um nome pode cair em rede) e aplica a regra;
//! resolucao que falha tambem recusa (fail-closed — o bind falharia de
//! qualquer jeito).

use std::fmt;
use std::net::{SocketAddr, ToSocketAddrs};

use crate::model::GatewayConfig;

/// A env que o clap le em `--host`.
pub const HOST_ENV: &str = "HOST";

/// A env irma de [`HOST_ENV`], lida por `--port`.
pub const PORT_ENV: &str = "PORT";

/// O `default_value` do clap para `--host` em `start`/`restart`.
///
/// Espelho de `crates/garraia-cli/src/main.rs`; o teste
/// `defaults_do_clap_espelham_o_main_da_cli` (em `check.rs`) le aquele fonte
/// e prende os literais a estas constantes.
pub const HOST_DEFAULT: &str = "127.0.0.1";

/// O `default_value` irmao para `--port`.
pub const PORT_DEFAULT: u16 = 3888;

/// De onde veio cada metade do bind que um `garraia start` sem flags usaria.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FonteDoBind {
    /// A env [`HOST_ENV`] / [`PORT_ENV`].
    Env,
    /// O `default_value` do clap ([`HOST_DEFAULT`] / [`PORT_DEFAULT`]).
    DefaultDoClap,
}

/// O par host/porta resolvido a partir do ambiente, com a origem de cada
/// metade.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BindDoAmbiente {
    pub host: String,
    pub host_source: FonteDoBind,
    pub port: u16,
    pub port_source: FonteDoBind,
}

/// Funcao pura: dadas as envs, qual bind um `garraia start` sem flags usaria.
/// Mesma precedencia do clap: env nao vazia, senao o `default_value`. O
/// arquivo nao entra.
///
/// Uma `PORT` que nao e um `u16` cai no default: o clap recusaria esse valor
/// no `start`, entao nao ha bind para reportar.
pub fn bind_de(host_da_env: Option<&str>, porta_da_env: Option<&str>) -> BindDoAmbiente {
    let host_da_env = host_da_env.map(str::trim).filter(|h| !h.is_empty());
    let porta_da_env = porta_da_env.map(str::trim).filter(|p| !p.is_empty());
    let (host, host_source) = match host_da_env {
        Some(h) => (h.to_string(), FonteDoBind::Env),
        None => (HOST_DEFAULT.to_string(), FonteDoBind::DefaultDoClap),
    };
    let (port, port_source) = match porta_da_env.and_then(|p| p.parse::<u16>().ok()) {
        Some(p) => (p, FonteDoBind::Env),
        None => (PORT_DEFAULT, FonteDoBind::DefaultDoClap),
    };
    BindDoAmbiente {
        host,
        host_source,
        port,
        port_source,
    }
}

/// [`bind_de`] sobre as envs reais do processo.
pub fn bind_do_ambiente() -> BindDoAmbiente {
    bind_de(
        std::env::var(HOST_ENV).ok().as_deref(),
        std::env::var(PORT_ENV).ok().as_deref(),
    )
}

/// O host que um **cliente** deve discar para falar com um gateway ligado em
/// `host`: `0.0.0.0` e `::` significam "toda interface" no servidor, mas nao
/// sao destino — o cliente local usa o loopback da mesma familia.
pub fn host_para_cliente(host: &str) -> String {
    let h = host.trim();
    let sem_colchetes = h
        .strip_prefix('[')
        .and_then(|r| r.strip_suffix(']'))
        .unwrap_or(h);
    match sem_colchetes.parse::<std::net::IpAddr>() {
        Ok(ip) if ip.is_unspecified() && ip.is_ipv4() => "127.0.0.1".to_string(),
        Ok(ip) if ip.is_unspecified() => "::1".to_string(),
        Ok(ip) => ip.to_string(),
        _ => h.to_string(),
    }
}

/// `http://host:porta` para um cliente, com os colchetes que um IPv6 literal
/// exige numa URL.
pub fn url_http(host: &str, port: u16) -> String {
    format!("http://{}", formatar_pedido(host, port))
}

/// O par host/porta que os comandos cliente usam para achar o gateway
/// local: o bind do ambiente, com o endereco nao especificado trocado por
/// loopback. O host sai sem colchetes (serve para `(host, porta)`); para URL
/// use [`url_http`].
pub fn endereco_do_cliente() -> (String, u16) {
    let bind = bind_do_ambiente();
    (host_para_cliente(&bind.host), bind.port)
}

/// O veredito sobre um bind que pode subir.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VereditoDoBind {
    /// Todo endereco resolvido e loopback: so a propria maquina alcanca.
    Loopback,
    /// Alcancavel da rede, com credencial de gateway configurada.
    ExpostoComCredencial,
    /// Alcancavel da rede, sem credencial, com o opt-out explicito. Sobe, e
    /// o boot avisa alto ([`aviso_de_opt_out`]).
    ExpostoPorOptOut,
}

/// Por que o boot foi recusado.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BindRecusado {
    /// Pelo menos um endereco resolvido nao e loopback, e nao ha credencial
    /// nem opt-out.
    ExpostoSemCredencial {
        /// O que o operador pediu (`host:porta`), para a mensagem.
        pedido: String,
        /// O primeiro endereco resolvido que nao e loopback.
        exposto: SocketAddr,
    },
    /// O host nao resolveu para nenhum endereco — o bind falharia de todo
    /// jeito, e nao ha como afirmar que ele fica local.
    NaoResolveu { pedido: String, erro: String },
}

/// A regra, pura. `enderecos` vazio recusa (fail-closed): sem endereco nao ha
/// como afirmar loopback.
pub fn decidir(
    pedido: &str,
    enderecos: &[SocketAddr],
    credencial_configurada: bool,
    opt_out: bool,
) -> Result<VereditoDoBind, BindRecusado> {
    if enderecos.is_empty() {
        return Err(BindRecusado::NaoResolveu {
            pedido: pedido.to_string(),
            erro: "no address".to_string(),
        });
    }
    let Some(exposto) = enderecos.iter().find(|a| !a.ip().is_loopback()) else {
        return Ok(VereditoDoBind::Loopback);
    };
    if credencial_configurada {
        return Ok(VereditoDoBind::ExpostoComCredencial);
    }
    if opt_out {
        return Ok(VereditoDoBind::ExpostoPorOptOut);
    }
    Err(BindRecusado::ExpostoSemCredencial {
        pedido: pedido.to_string(),
        exposto: *exposto,
    })
}

/// Resolve `host:port` e aplica [`decidir`] com a credencial e o opt-out de
/// `gateway`. `host`/`port` sao os do bind real (flag > env > default), NUNCA
/// `gateway.host`/`gateway.port` do arquivo.
pub fn verificar(
    host: &str,
    port: u16,
    gateway: &GatewayConfig,
) -> Result<VereditoDoBind, BindRecusado> {
    let pedido = formatar_pedido(host, port);
    let h = host.trim();
    // `[::]` (a grafia que o `SocketAddr` aceita) chega aqui com colchetes;
    // a tupla `(host, porta)` quer o IP nu.
    let h = h
        .strip_prefix('[')
        .and_then(|r| r.strip_suffix(']'))
        .unwrap_or(h);
    let enderecos: Vec<SocketAddr> = match (h, port).to_socket_addrs() {
        Ok(it) => it.collect(),
        Err(e) => {
            return Err(BindRecusado::NaoResolveu {
                pedido,
                erro: e.to_string(),
            });
        }
    };
    decidir(
        &pedido,
        &enderecos,
        gateway.api_key_configurada(),
        gateway.allow_unauthenticated_network_bind,
    )
}

fn formatar_pedido(host: &str, port: u16) -> String {
    let h = host.trim();
    if h.contains(':') && !h.starts_with('[') {
        format!("[{h}]:{port}")
    } else {
        format!("{h}:{port}")
    }
}

impl fmt::Display for BindRecusado {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let bin = garraia_common::executavel::nome();
        match self {
            BindRecusado::ExpostoSemCredencial { pedido, exposto } => write!(
                f,
                "refusing to start: the gateway would listen on {pedido} ({exposto}), which is \
                 reachable from the network, and no gateway credential is configured. Every \
                 /api/* route and the /ws agent channel (with its file and device tools) would \
                 answer anyone who can reach this port.\n\
                 Fix it with ONE of:\n  \
                 - run `{bin} init` (it writes gateway.api_key for you), then start again;\n  \
                 - set gateway.api_key in the config file;\n  \
                 - export GARRAIA_GATEWAY_API_KEY=<a long random secret> (the env var wins \
                 over the file; nothing is written to disk);\n  \
                 - or listen locally only: `{bin} start --host 127.0.0.1` (and unset the \
                 HOST env var if it is set).\n\
                 A deployment that is open on purpose, behind an authenticating proxy or a \
                 firewall, can set gateway.allow_unauthenticated_network_bind: true in the \
                 config file (there is no env var or flag for it). See \
                 docs/auth-config.md section 5.1."
            ),
            BindRecusado::NaoResolveu { pedido, erro } => write!(
                f,
                "refusing to start: the gateway bind address {pedido} does not resolve \
                 ({erro}), so there is no way to tell whether it stays on loopback. Pass an IP \
                 literal, for example `{bin} start --host 127.0.0.1`."
            ),
        }
    }
}

impl std::error::Error for BindRecusado {}

/// O aviso de cada boot com o opt-out ligado: sobe, mas nunca em silencio.
pub fn aviso_de_opt_out(host: &str, port: u16) -> String {
    format!(
        "gateway.allow_unauthenticated_network_bind is true: the gateway listens on {} \
         WITHOUT a gateway credential, so every /api/* route and the /ws agent channel \
         answer anyone who can reach this port. Only do this behind an authenticating \
         proxy or a firewall; otherwise set gateway.api_key or GARRAIA_GATEWAY_API_KEY and \
         remove the opt-out.",
        formatar_pedido(host, port)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn a(s: &str) -> SocketAddr {
        s.parse().expect("socket addr de teste")
    }

    #[test]
    fn precedencia_env_vence_default_e_o_arquivo_nunca_entra() {
        let b = bind_de(None, None);
        assert_eq!((b.host.as_str(), b.port), (HOST_DEFAULT, PORT_DEFAULT));
        assert_eq!(b.host_source, FonteDoBind::DefaultDoClap);

        let b = bind_de(Some("0.0.0.0"), Some("4000"));
        assert_eq!((b.host.as_str(), b.port), ("0.0.0.0", 4000));
        assert_eq!(b.host_source, FonteDoBind::Env);
        assert_eq!(b.port_source, FonteDoBind::Env);

        let b = bind_de(Some("   "), Some("nao-e-porta"));
        assert_eq!((b.host.as_str(), b.port), (HOST_DEFAULT, PORT_DEFAULT));
    }

    #[test]
    fn cliente_troca_o_nao_especificado_por_loopback() {
        assert_eq!(host_para_cliente("0.0.0.0"), "127.0.0.1");
        assert_eq!(host_para_cliente("::"), "::1");
        assert_eq!(host_para_cliente("[::]"), "::1");
        assert_eq!(host_para_cliente("[::1]"), "::1");
        assert_eq!(host_para_cliente("192.168.1.10"), "192.168.1.10");
        assert_eq!(host_para_cliente("localhost"), "localhost");
        assert_eq!(url_http("::1", 3888), "http://[::1]:3888");
        assert_eq!(url_http("127.0.0.1", 3888), "http://127.0.0.1:3888");
    }

    #[test]
    fn recusa_bind_exposto_sem_credencial() {
        for addr in [
            "0.0.0.0:3888",
            "[::]:3888",
            "192.168.1.10:3888",
            "8.8.8.8:1",
        ] {
            let r = decidir(addr, &[a(addr)], false, false);
            assert!(
                matches!(r, Err(BindRecusado::ExpostoSemCredencial { .. })),
                "{addr}: {r:?}"
            );
        }
    }

    #[test]
    fn loopback_sobe_sem_credencial() {
        for addr in ["127.0.0.1:3888", "[::1]:3888", "127.0.0.2:1"] {
            assert_eq!(
                decidir(addr, &[a(addr)], false, false),
                Ok(VereditoDoBind::Loopback)
            );
        }
    }

    #[test]
    fn credencial_libera_o_bind_exposto() {
        assert_eq!(
            decidir("x", &[a("0.0.0.0:3888")], true, false),
            Ok(VereditoDoBind::ExpostoComCredencial)
        );
    }

    #[test]
    fn opt_out_sobe_com_aviso() {
        assert_eq!(
            decidir("x", &[a("0.0.0.0:3888")], false, true),
            Ok(VereditoDoBind::ExpostoPorOptOut)
        );
    }

    /// Um nome que resolve para loopback E para rede e exposto: basta um.
    #[test]
    fn resolucao_mista_conta_como_exposta() {
        let r = decidir("h:1", &[a("127.0.0.1:1"), a("10.0.0.5:1")], false, false);
        match r {
            Err(BindRecusado::ExpostoSemCredencial { exposto, .. }) => {
                assert_eq!(exposto, a("10.0.0.5:1"));
            }
            outro => panic!("{outro:?}"),
        }
    }

    #[test]
    fn sem_endereco_recusa() {
        assert!(matches!(
            decidir("h:1", &[], true, true),
            Err(BindRecusado::NaoResolveu { .. })
        ));
    }

    #[test]
    fn verificar_usa_a_credencial_e_o_opt_out_da_config() {
        let mut g = GatewayConfig::default();
        assert!(verificar("0.0.0.0", 0, &g).is_err());
        assert!(matches!(
            verificar("[::]", 0, &g),
            Err(BindRecusado::ExpostoSemCredencial { .. })
        ));
        assert_eq!(verificar("[::1]", 0, &g), Ok(VereditoDoBind::Loopback));
        assert_eq!(verificar("127.0.0.1", 0, &g), Ok(VereditoDoBind::Loopback));
        assert_eq!(verificar("localhost", 0, &g), Ok(VereditoDoBind::Loopback));

        // Chave em branco nao e credencial (#1241).
        g.api_key = Some("   ".into());
        assert!(verificar("0.0.0.0", 0, &g).is_err());

        g.api_key = Some("uma-credencial".into());
        assert_eq!(
            verificar("0.0.0.0", 0, &g),
            Ok(VereditoDoBind::ExpostoComCredencial)
        );

        let g = GatewayConfig {
            api_key_env: crate::auth::gateway_api_key_de(Some("da-env".into())),
            ..Default::default()
        };
        assert_eq!(
            verificar("0.0.0.0", 0, &g),
            Ok(VereditoDoBind::ExpostoComCredencial),
            "GARRAIA_GATEWAY_API_KEY conta como credencial"
        );

        let g = GatewayConfig {
            allow_unauthenticated_network_bind: true,
            ..Default::default()
        };
        assert_eq!(
            verificar("0.0.0.0", 0, &g),
            Ok(VereditoDoBind::ExpostoPorOptOut)
        );
    }

    /// TLS nao isenta: TLS sem credencial continua aberto.
    #[test]
    fn tls_sem_credencial_continua_recusado() {
        let g = GatewayConfig {
            tls_cert_path: Some("/c.pem".into()),
            tls_key_path: Some("/k.pem".into()),
            ..Default::default()
        };
        assert!(verificar("0.0.0.0", 0, &g).is_err());
    }

    #[test]
    fn host_que_nao_resolve_recusa() {
        let g = GatewayConfig::default();
        assert!(matches!(
            verificar("nao existe .invalid", 1, &g),
            Err(BindRecusado::NaoResolveu { .. })
        ));
    }

    #[test]
    fn mensagem_diz_como_corrigir_e_nunca_mostra_segredo() {
        let err = decidir("0.0.0.0:3888", &[a("0.0.0.0:3888")], false, false).expect_err("recusa");
        let msg = err.to_string();
        for trecho in [
            " init`",
            "gateway.api_key",
            "GARRAIA_GATEWAY_API_KEY",
            "--host 127.0.0.1",
            "allow_unauthenticated_network_bind",
        ] {
            assert!(msg.contains(trecho), "falta `{trecho}` em: {msg}");
        }
        assert!(!msg.contains("`garra "), "o binario e o instalado: {msg}");
    }

    #[test]
    fn opt_out_so_existe_no_arquivo() {
        let g: GatewayConfig =
            serde_yaml::from_str("allow_unauthenticated_network_bind: true\n").expect("yaml");
        assert!(g.allow_unauthenticated_network_bind);
        let g: GatewayConfig =
            toml::from_str("allow_unauthenticated_network_bind = true\n").expect("toml");
        assert!(g.allow_unauthenticated_network_bind);
        assert!(!GatewayConfig::default().allow_unauthenticated_network_bind);
        // `api_key_env` e serde(skip): um arquivo nao consegue injeta-la.
        let g: GatewayConfig = serde_yaml::from_str("api_key_env: \"x\"\n").expect("yaml");
        assert!(g.api_key_env.is_none());
    }
}
