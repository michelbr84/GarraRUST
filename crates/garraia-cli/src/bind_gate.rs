//! A recusa de bind exposto sem credencial, do lado da CLI (#1261).
//!
//! O gateway (`GatewayServer::run`) toma a mesma decisao antes de ligar o
//! socket. Aqui ela roda **antes**, por dois motivos concretos:
//!
//! - `garraia start -d` / `restart -d`: depois do fork o tracing aponta para
//!   `~/.garraia/garraia.log`, e a recusa do gateway nunca chegaria ao
//!   terminal. Tem de sair em stderr do processo pai, com exit nao-zero.
//! - `garraia restart`: a recusa tem de acontecer ANTES de derrubar o daemon
//!   que esta rodando — senao um `restart` recusado deixa o operador sem
//!   gateway nenhum.
//!
//! A decisao e a de `garraia_config::bind::verificar`, sobre o host/porta ja
//! resolvidos pelo clap (flag > env > default) — nunca `gateway.host` do
//! arquivo.

use garraia_config::GatewayConfig;
use garraia_config::bind::{BindRecusado, VereditoDoBind, aviso_de_opt_out, verificar};

/// O resultado da pre-checagem, ja pronto para a CLI imprimir.
#[derive(Debug)]
pub enum Preflight {
    /// Pode subir, sem nada a dizer.
    Sobe,
    /// Pode subir com o opt-out explicito; o texto e o aviso do boot.
    SobeComAviso(String),
}

/// Aplica a regra do #1261 a `gateway.host`/`gateway.port` **depois** de a
/// CLI ter escrito neles o bind resolvido pelo clap.
pub fn preflight(gateway: &GatewayConfig) -> Result<Preflight, BindRecusado> {
    match verificar(&gateway.host, gateway.port, gateway)? {
        VereditoDoBind::Loopback | VereditoDoBind::ExpostoComCredencial => Ok(Preflight::Sobe),
        VereditoDoBind::ExpostoPorOptOut => Ok(Preflight::SobeComAviso(aviso_de_opt_out(
            &gateway.host,
            gateway.port,
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn gw(host: &str) -> GatewayConfig {
        GatewayConfig {
            host: host.into(),
            port: 0,
            ..Default::default()
        }
    }

    #[test]
    fn recusa_o_bind_resolvido_nao_o_do_arquivo() {
        // O `main.rs` escreve o bind do clap em `gateway.host`; a decisao
        // olha para ele.
        assert!(preflight(&gw("0.0.0.0")).is_err());
        assert!(matches!(preflight(&gw("127.0.0.1")), Ok(Preflight::Sobe)));
    }

    #[test]
    fn opt_out_sobe_com_aviso() {
        let mut g = gw("0.0.0.0");
        g.allow_unauthenticated_network_bind = true;
        match preflight(&g) {
            Ok(Preflight::SobeComAviso(aviso)) => {
                assert!(
                    aviso.contains("allow_unauthenticated_network_bind"),
                    "{aviso}"
                );
            }
            outro => panic!("{outro:?}"),
        }
    }

    #[test]
    fn credencial_sobe_calado() {
        let mut g = gw("0.0.0.0");
        g.api_key = Some("uma-credencial".into());
        assert!(matches!(preflight(&g), Ok(Preflight::Sobe)));
    }
}
