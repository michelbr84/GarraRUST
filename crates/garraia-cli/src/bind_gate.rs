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

/// #1816: quem ja ocupa a porta do gateway.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PortaOcupada {
    /// Um gateway GarraIA, com a versao que o `/api/health` dele informou.
    PorGateway(String),
    /// Outro programa, ou um gateway que nao respondeu a tempo.
    PorOutro,
}

/// A porta ja tem dono? Roda **antes** do boot: sem esta checagem, um
/// segundo `garraia start` (o sidecar do Desktop numa maquina que ja tem o
/// servico do systemd, por exemplo) subia canais, scheduler e a ponte do
/// WhatsApp com a MESMA sessao, so para morrer no bind — e no caminho apagava
/// o pidfile do outro gateway.
///
/// Erro de bind que nao e `AddrInUse` (host invalido, permissao) devolve
/// `None`: o bind de verdade, no gateway, decide e explica.
pub fn porta_ocupada(host: &str, port: u16) -> Option<PortaOcupada> {
    match std::net::TcpListener::bind((host, port)) {
        Ok(_) => None,
        Err(e) if e.kind() == std::io::ErrorKind::AddrInUse => Some(
            garraia_desktop_core::provedor::gateway_na_porta(
                port,
                std::time::Duration::from_millis(800),
            )
            .map(PortaOcupada::PorGateway)
            .unwrap_or(PortaOcupada::PorOutro),
        ),
        Err(_) => None,
    }
}

/// A mensagem do `start` recusado por porta ocupada: o que esta la e o que
/// fazer.
pub fn mensagem_porta_ocupada(host: &str, port: u16, quem: &PortaOcupada) -> String {
    match quem {
        PortaOcupada::PorGateway(versao) => format!(
            "error: ja existe um gateway GarraIA {versao} ouvindo em {host}:{port}. Nao subo um \
             segundo: os dois dividiriam a sessao do WhatsApp e o pidfile, mesmo em outra porta. \
             Use esse, ou pare-o antes (`garraia stop`, ou `systemctl --user stop garraia` se \
             ele for um servico)."
        ),
        PortaOcupada::PorOutro => format!(
            "error: a porta {host}:{port} ja esta em uso por outro programa. Libere a porta ou \
             suba o gateway em outra com `--port`."
        ),
    }
}

/// O resultado de tentar a trava do diretorio de config (#1816, revisao M1).
#[derive(Debug)]
pub enum Trava {
    /// Este processo e o unico gateway deste diretorio enquanto o `File` viver.
    Obtida(std::fs::File),
    /// O sistema de arquivos nao suporta trava (ex.: alguns FS de rede): segue
    /// sem ela — a checagem de porta continua valendo.
    SemSuporte,
}

/// A checagem de porta tem janela: o bind de verdade e no FIM do boot, entao
/// dois `garraia start` ao mesmo tempo (o servico do systemd e o autostart do
/// Desktop no login) passavam os dois e subiam duas pontes com a mesma sessao.
/// Uma trava exclusiva em `<dir>/gateway.lock`, segura a vida inteira do
/// processo, fecha isso — e vale mesmo em portas diferentes, que e o caso em
/// que os dois dividiriam a sessao e o pidfile sem nada perceber.
///
/// Espera ate `espera` pela trava: o restart legitimo (o Desktop matando o
/// sidecar e subindo outro) pode chegar antes de o processo antigo terminar
/// de morrer. `Err(())`: outro gateway segura o diretorio.
pub fn travar_diretorio(dir: &std::path::Path, espera: std::time::Duration) -> Result<Trava, ()> {
    if std::fs::create_dir_all(dir).is_err() {
        return Ok(Trava::SemSuporte);
    }
    let Ok(arquivo) = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(dir.join("gateway.lock"))
    else {
        return Ok(Trava::SemSuporte);
    };
    let fim = std::time::Instant::now() + espera;
    loop {
        match arquivo.try_lock() {
            Ok(()) => return Ok(Trava::Obtida(arquivo)),
            Err(std::fs::TryLockError::WouldBlock) => {
                if std::time::Instant::now() >= fim {
                    return Err(());
                }
                std::thread::sleep(std::time::Duration::from_millis(200));
            }
            Err(std::fs::TryLockError::Error(_)) => return Ok(Trava::SemSuporte),
        }
    }
}

/// A mensagem do `start` recusado porque outro gateway segura o diretorio.
pub fn mensagem_diretorio_ocupado(dir: &std::path::Path) -> String {
    format!(
        "error: ja existe um gateway GarraIA rodando com o diretorio de configuracao {}. \
         Nao subo um segundo: os dois dividiriam a sessao do WhatsApp e o pidfile. Use esse, \
         ou pare-o antes (`garraia stop`, ou `systemctl --user stop garraia` se ele for um \
         servico).",
        dir.display()
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// #1816 (revisao M1): a trava e exclusiva por diretorio, vale entre
    /// handles do mesmo processo (como entre processos) e solta ao fechar.
    #[test]
    fn trava_do_diretorio_e_exclusiva_e_solta_ao_fechar() {
        let dir = tempfile::tempdir().unwrap();
        let espera = std::time::Duration::from_millis(300);
        let primeira = travar_diretorio(dir.path(), espera).expect("primeira trava");
        assert!(matches!(primeira, Trava::Obtida(_)));
        let inicio = std::time::Instant::now();
        assert!(
            travar_diretorio(dir.path(), espera).is_err(),
            "a segunda espera e desiste"
        );
        assert!(inicio.elapsed() >= espera);
        drop(primeira);
        assert!(matches!(
            travar_diretorio(dir.path(), espera),
            Ok(Trava::Obtida(_))
        ));
        let msg = mensagem_diretorio_ocupado(dir.path());
        assert!(
            msg.contains("gateway GarraIA") && !msg.contains("--port"),
            "{msg}"
        );
    }

    /// #1816: porta livre passa; ocupada por um programa qualquer e recusada
    /// como "outro"; ocupada por um gateway GarraIA diz a versao dele.
    #[test]
    fn porta_ocupada_distingue_livre_outro_programa_e_gateway() {
        use std::io::{Read, Write};
        use std::net::TcpListener;

        let livre = TcpListener::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap()
            .port();
        assert_eq!(porta_ocupada("127.0.0.1", livre), None);

        let mudo = TcpListener::bind("127.0.0.1:0").unwrap();
        let porta_muda = mudo.local_addr().unwrap().port();
        assert_eq!(
            porta_ocupada("127.0.0.1", porta_muda),
            Some(PortaOcupada::PorOutro)
        );

        let gw = TcpListener::bind("127.0.0.1:0").unwrap();
        let porta_gw = gw.local_addr().unwrap().port();
        std::thread::spawn(move || {
            if let Ok((mut s, _)) = gw.accept() {
                let mut buf = [0u8; 512];
                let _ = s.read(&mut buf);
                let _ = s.write_all(
                    b"HTTP/1.0 200 OK\r\n\r\n{\"status\":\"healthy\",\"version\":\"0.5.1\",\"gateway_url\":\"x\"}",
                );
            }
        });
        assert_eq!(
            porta_ocupada("127.0.0.1", porta_gw),
            Some(PortaOcupada::PorGateway("0.5.1".into()))
        );
        let msg = mensagem_porta_ocupada(
            "127.0.0.1",
            porta_gw,
            &PortaOcupada::PorGateway("0.5.1".into()),
        );
        assert!(msg.contains("0.5.1") && !msg.contains("--port"), "{msg}");
        drop(mudo);
    }

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
