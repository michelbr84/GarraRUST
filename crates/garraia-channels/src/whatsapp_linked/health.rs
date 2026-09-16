//! A classificacao de saude do canal — **uma** fonte para as duas superficies.
//!
//! `garra whatsapp status` (processo da CLI) e `GET /api/diagnostics` (processo
//! do gateway) respondem a mesma pergunta para o mesmo usuario: "o WhatsApp
//! vinculado esta de pe, e se nao esta, o que eu faco?". Cada um tem acesso a
//! uma parte diferente da verdade — a CLI ve o disco, o gateway ve o disco **e**
//! a ponte — mas a regra que transforma fatos em veredito e proximo passo mora
//! aqui, e so aqui.
//!
//! Isto nao e purismo: e a mesma licao do `KNOWN_CHANNELS` × `PushChannelStates`
//! (#1079), onde duas fontes divergentes fizeram o console classificar como
//! `offline` um canal que estava recebendo webhook e respondendo. Um `status`
//! que diz "tudo certo" enquanto o `/api/diagnostics` diz "ponte caida" e a
//! mesma classe de defeito, com o agravante de que aqui o usuario acabou de
//! entregar a conta de WhatsApp dele ao produto.
//!
//! # O que NAO esta aqui
//!
//! Nada de cripto. [`DiskFacts::read`] so faz `stat` — tres deles. A prova de
//! que o blob **abre** (decifrar com a chave atual) e cara em modo passphrase
//! (PBKDF2 600k) e continua sendo do chamador que pode pagar por ela: a CLI,
//! uma vez por comando. Um `/api/diagnostics` que derivasse chave a cada
//! request seria um DoS contra o proprio gateway.

use std::path::Path;

use super::bridge::deps_installed;
use super::session::SessionStore;

/// O que quem pergunta sabe sobre a **ponte**.
///
/// [`BridgeView::Unknown`] nao e "caida": e "quem esta perguntando nao
/// supervisiona a ponte". A CLI esta sempre nesse caso — ela roda num processo
/// que nao tem o filho `node`, e afirmar `Down` dali seria inventar um defeito.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum BridgeView {
    /// Quem pergunta nao acompanha a ponte (a CLI).
    #[default]
    Unknown,
    /// O supervisor existe mas ainda nao subiu a ponte.
    NotStarted,
    /// A ponte ja subiu alguma vez e agora nao esta conectada.
    Down,
    /// Conectada ao WhatsApp.
    Connected,
}

/// Fatos de disco. Tres `stat`, sem cripto e sem processo.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiskFacts {
    /// Ha `session.enc`.
    pub session_present: bool,
    /// Ha `session.enc.prev` — credencial viva de um re-vinculo que nao
    /// terminou (F1 da auditoria R4).
    pub archive_present: bool,
    /// Ha `node_modules` no diretorio da ponte.
    pub deps_installed: bool,
}

impl DiskFacts {
    /// Le os tres fatos. **Nao cria nada** — de proposito: o
    /// `/api/diagnostics` e uma rota de leitura e nao pode materializar
    /// diretorio nem chave por ter sido chamada.
    pub fn read(store: &SessionStore, bridge_dir: &Path) -> Self {
        Self {
            session_present: store.exists(),
            archive_present: store.archive_path().is_file(),
            deps_installed: deps_installed(bridge_dir),
        }
    }
}

/// O veredito que as duas superficies mostram.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinkHealth {
    /// Nao ha sessao em disco: ninguem vinculou este aparelho ainda. **Nao e
    /// defeito** — e um canal opcional que o operador nao ligou.
    NotLinked,
    /// Ha sessao, mas a ponte nao tem dependencias instaladas.
    MissingDependencies,
    /// Ha sessao e dependencias, e a ponte nao esta de pe. Aqui **e** defeito:
    /// alguem vinculou o aparelho e o canal nao esta funcionando.
    BridgeDown,
    /// Ha sessao e a ponte esta conectada.
    Connected,
    /// Ha sessao e dependencias; quem pergunta nao sabe o estado da ponte.
    Linked,
}

/// A regra. Tabela fechada, sem I/O — o teste dela nao precisa de disco.
///
/// A ordem importa:
///
/// 1. **Ausencia de sessao vence tudo.** Sem ela nao ha canal para estar
///    quebrado — e um canal opcional que o operador nao ligou.
/// 2. **Falta de dependencia vence a queda.** Reiniciar uma ponte sem
///    `node_modules` nao adianta; instalar adianta. E o mesmo fato que a ponte
///    real reporta como `started.baileys_version == null`, so que legivel por
///    um `stat` — o que permite ao `/api/diagnostics` responde-lo **antes** de
///    qualquer processo subir, e a CLI responde-lo sem ter processo nenhum.
pub fn classify(facts: &DiskFacts, bridge: BridgeView) -> LinkHealth {
    if !facts.session_present {
        return LinkHealth::NotLinked;
    }
    if !facts.deps_installed {
        return LinkHealth::MissingDependencies;
    }
    match bridge {
        BridgeView::Connected => LinkHealth::Connected,
        BridgeView::Down | BridgeView::NotStarted => LinkHealth::BridgeDown,
        BridgeView::Unknown => LinkHealth::Linked,
    }
}

impl LinkHealth {
    /// O operador ja vinculou este aparelho?
    ///
    /// E o bit que o `/api/channels` usa para decidir entre `optional` (nao
    /// ligado, nao e defeito) e `offline` (ligado e quebrado).
    pub fn provisioned(self) -> bool {
        !matches!(self, LinkHealth::NotLinked)
    }

    /// Esta tudo bem?
    pub fn healthy(self) -> bool {
        matches!(self, LinkHealth::Connected | LinkHealth::Linked)
    }

    /// O proximo passo, em pt-BR. `None` quando nao ha nada a fazer.
    ///
    /// Sempre um **comando que o usuario pode colar**, com o caminho real
    /// interpolado: "rode `npm ci`" sem dizer onde e o tipo de conselho que
    /// manda a pessoa procurar.
    pub fn next_step(self, bridge_dir: &Path) -> Option<String> {
        match self {
            LinkHealth::NotLinked => Some("rode `garra whatsapp link`".to_string()),
            LinkHealth::MissingDependencies => {
                Some(format!("rode `npm ci` em {}", bridge_dir.display()))
            }
            LinkHealth::BridgeDown => Some(
                "a ponte nao esta de pe: confira se `node` 20+ esta na PATH e \
veja o log do gateway; `garra whatsapp status` confirma a sessao"
                    .to_string(),
            ),
            LinkHealth::Connected | LinkHealth::Linked => None,
        }
    }

    /// Mesmo passo em ingles, para a metade em ingles da CLI.
    pub fn next_step_en(self, bridge_dir: &Path) -> Option<String> {
        match self {
            LinkHealth::NotLinked => Some("run `garra whatsapp link`".to_string()),
            LinkHealth::MissingDependencies => {
                Some(format!("run `npm ci` in {}", bridge_dir.display()))
            }
            LinkHealth::BridgeDown => Some(
                "the bridge is not up: check that `node` 20+ is on PATH and \
read the gateway log; `garra whatsapp status` confirms the session"
                    .to_string(),
            ),
            LinkHealth::Connected | LinkHealth::Linked => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn facts(session: bool, deps: bool) -> DiskFacts {
        DiskFacts {
            session_present: session,
            archive_present: false,
            deps_installed: deps,
        }
    }

    /// A tabela inteira, de uma vez. Cada linha e um estado que alguma das
    /// duas superficies mostra.
    #[test]
    fn tabela_de_classificacao() {
        use BridgeView as B;
        use LinkHealth as H;
        let casos: &[(bool, bool, BridgeView, LinkHealth, &str)] = &[
            (false, false, B::Unknown, H::NotLinked, "sem sessao, CLI"),
            (
                false,
                true,
                B::Connected,
                H::NotLinked,
                "sem sessao vence tudo",
            ),
            (
                true,
                false,
                B::Unknown,
                H::MissingDependencies,
                "sem node_modules",
            ),
            (
                true,
                false,
                B::NotStarted,
                H::MissingDependencies,
                "supervisor parado e sem deps",
            ),
            (
                true,
                false,
                B::Down,
                H::MissingDependencies,
                "sem deps vence a queda: reiniciar nao conserta, instalar conserta",
            ),
            (true, true, B::Connected, H::Connected, "caminho feliz"),
            (true, true, B::Down, H::BridgeDown, "vinculado e a ponte caiu"),
            (
                true,
                true,
                B::NotStarted,
                H::BridgeDown,
                "vinculado e o supervisor nao subiu",
            ),
            (
                true,
                true,
                B::Unknown,
                H::Linked,
                "a CLI nao afirma sobre a ponte",
            ),
        ];
        for &(sessao, deps, ponte, esperado, porque) in casos {
            assert_eq!(
                classify(&facts(sessao, deps), ponte),
                esperado,
                "{porque} (sessao={sessao} deps={deps} ponte={ponte:?})"
            );
        }
    }

    /// Sem sessao o canal esta *opcional*, nao quebrado — e a diferenca entre
    /// `optional` e `offline` no `/api/channels`.
    #[test]
    fn so_nao_vinculado_e_nao_aprovisionado() {
        assert!(!LinkHealth::NotLinked.provisioned());
        for h in [
            LinkHealth::MissingDependencies,
            LinkHealth::BridgeDown,
            LinkHealth::Connected,
            LinkHealth::Linked,
        ] {
            assert!(h.provisioned(), "{h:?} ja tem sessao em disco");
        }
    }

    /// Todo estado ruim tem proximo passo, e nenhum estado bom tem.
    #[test]
    fn todo_estado_ruim_tem_proximo_passo_acionavel() {
        let dir = PathBuf::from("/tmp/garraia/whatsapp/bridge");
        for h in [
            LinkHealth::NotLinked,
            LinkHealth::MissingDependencies,
            LinkHealth::BridgeDown,
        ] {
            let pt = h.next_step(&dir).unwrap_or_else(|| panic!("{h:?} sem passo"));
            let en = h
                .next_step_en(&dir)
                .unwrap_or_else(|| panic!("{h:?} sem passo em ingles"));
            assert!(!pt.trim().is_empty() && !en.trim().is_empty());
        }
        for h in [LinkHealth::Connected, LinkHealth::Linked] {
            assert!(h.next_step(&dir).is_none(), "{h:?} nao tem o que consertar");
            assert!(h.next_step_en(&dir).is_none());
        }
    }

    /// O caminho real entra na frase — "rode `npm ci`" sozinho manda a pessoa
    /// procurar onde.
    #[test]
    fn o_passo_do_npm_ci_cita_o_diretorio_real() {
        let dir = PathBuf::from("/home/ana/.garraia/data/whatsapp/bridge");
        let passo = LinkHealth::MissingDependencies
            .next_step(&dir)
            .expect("passo");
        assert!(passo.contains("npm ci"), "{passo}");
        assert!(
            passo.contains("/home/ana/.garraia/data/whatsapp/bridge"),
            "{passo}"
        );
    }

    /// O passo de "nao vinculado" cita o comando que existe. `garra whatsapp`
    /// abre o menu; `garra whatsapp link` e o que pareia.
    #[test]
    fn o_passo_de_nao_vinculado_cita_o_comando_de_link() {
        let passo = LinkHealth::NotLinked
            .next_step(Path::new("/x"))
            .expect("passo");
        assert!(passo.contains("garra whatsapp link"), "{passo}");
    }

    #[test]
    fn disk_facts_le_o_disco_sem_criar_nada() {
        let dir = tempfile::tempdir().expect("tempdir");
        let store = SessionStore::new(dir.path().join("whatsapp/default"));
        let bridge = dir.path().join("whatsapp/bridge");

        let f = DiskFacts::read(&store, &bridge);
        assert_eq!(f, facts(false, false));
        assert!(
            !store.dir().exists(),
            "ler o estado nao pode criar o diretorio da sessao"
        );
        assert!(!bridge.exists(), "ler o estado nao pode criar a ponte");
    }
}
