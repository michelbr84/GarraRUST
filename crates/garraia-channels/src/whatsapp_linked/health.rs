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
//! Nada de cripto. [`DiskFacts::read`] so faz `stat` (e le, no maximo, os 7
//! bytes do carimbo `pending` da ponte). A prova de que o blob **abre**
//! (decifrar com a chave atual) e cara em modo passphrase (PBKDF2 600k) e
//! continua sendo do chamador que pode pagar por ela: a CLI, uma vez por
//! comando. Um `/api/diagnostics` que derivasse chave a cada
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

/// Fatos de disco. Quatro `stat` (e, no maximo, a leitura dos 7 bytes do
/// carimbo `pending`), sem cripto e sem processo.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiskFacts {
    /// Ha `session.enc`.
    pub session_present: bool,
    /// Ha `session.enc.prev` — credencial viva de um re-vinculo que nao
    /// terminou (F1 da auditoria R4).
    pub archive_present: bool,
    /// Ha `node_modules` no diretorio da ponte, e o carimbo de dependencias
    /// nao esta em `pending` ([`deps_installed`]): um `npm ci` que nao
    /// terminou, ou uma arvore que o preparo da ponte recusou para os
    /// manifestos atuais, contam como "sem dependencias".
    pub deps_installed: bool,
}

impl DiskFacts {
    /// Le os fatos. **Nao cria nada** — de proposito: o
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
    ///
    /// `bin` e o nome do executavel que o usuario tem na maquina
    /// (`garraia_common::executavel::nome()` no chamador): o passo nunca pode
    /// mandar rodar um binario que nao existe (#1329).
    pub fn next_step(self, bridge_dir: &Path, bin: &str) -> Option<String> {
        match self {
            LinkHealth::NotLinked => Some(format!("rode `{bin} whatsapp link`")),
            // "e reinicie o gateway" nao e enfeite: o supervisor prepara a
            // ponte uma vez por boot, e e no boot seguinte que ele adota a
            // arvore que o `npm ci` do usuario instalou.
            LinkHealth::MissingDependencies => Some(format!(
                "rode `npm ci` em {} e reinicie o gateway",
                bridge_dir.display()
            )),
            LinkHealth::BridgeDown => Some(format!(
                "a ponte nao esta de pe: confira se `node` 20+ esta na PATH e \
veja o log do gateway; `{bin} whatsapp status` confirma a sessao"
            )),
            LinkHealth::Connected | LinkHealth::Linked => None,
        }
    }

    /// Mesmo passo em ingles, para a metade em ingles da CLI.
    pub fn next_step_en(self, bridge_dir: &Path, bin: &str) -> Option<String> {
        match self {
            LinkHealth::NotLinked => Some(format!("run `{bin} whatsapp link`")),
            LinkHealth::MissingDependencies => Some(format!(
                "run `npm ci` in {} and restart the gateway",
                bridge_dir.display()
            )),
            LinkHealth::BridgeDown => Some(format!(
                "the bridge is not up: check that `node` 20+ is on PATH and \
read the gateway log; `{bin} whatsapp status` confirms the session"
            )),
            LinkHealth::Connected | LinkHealth::Linked => None,
        }
    }
}

// ---------------------------------------------------------------------------
// Saude de runtime do canal (#1663): degradado, ultimo motivo e terminal.
//
// O `BridgeView` diz se a ponte esta de pe. Isto diz **como ela tem estado**:
// quantas quedas seguidas, por que caiu a ultima vez, e se parou de vez. E a
// diferenca entre "offline" (o console) e "degraded, ultimo motivo X" (o
// `garra_status`), que e o que o operador precisa para decidir se espera ou se
// re-vincula.
// ---------------------------------------------------------------------------

/// Quedas seguidas a partir das quais o canal e `degraded`.
///
/// Nao e configuravel ainda (nao ha campo em `channels.whatsapp_linked`):
/// constante nomeada ate o proximo slice de configuracao. Tres e o numero que
/// separa "uma queda transitaria" de "isto esta caindo em loop".
pub const LIMIAR_DEGRADADO: u32 = 3;

/// Por que o canal parou de vez: nao ha mais reconexao automatica.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MotivoTerminal {
    /// `logged_out` sem prova de morte: o logout foi pedido, ou outro aparelho
    /// assumiu (440). **Nada e apagado** — a sessao gravada continua valendo.
    Deslogado,
    /// O servidor declarou a sessao morta (exit 2 / `session_is_dead`): o
    /// material que o servidor recusou foi descartado.
    SessaoMorta,
}

impl MotivoTerminal {
    /// O que o operador faz agora, em pt-BR.
    pub fn proximo_passo(self) -> &'static str {
        match self {
            Self::Deslogado => "rode `garra whatsapp link` de novo para re-vincular",
            Self::SessaoMorta => "rode `garra whatsapp link` para vincular o aparelho de novo",
        }
    }
}

/// O que o runtime viu em rodada, alem de a ponte estar ou nao de pe.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SaudeDoCanal {
    /// Quedas seguidas desde a ultima vez que a ponte chegou a conectar. Mesma
    /// semantica do contador de reconexao do `serve_with`: zerar quando a
    /// ponte conecta, porque ai a sessao provou que funciona.
    pub falhas_consecutivas: u32,
    /// Ultimo motivo de queda informado pelo bridge. O nome e texto pronto
    /// para log — o enum `DisconnectReason` vive em `protocol`, e guardar o
    /// nome evita acoplar o `/api/diagnostics` a ele.
    pub ultima_queda: Option<(String, Option<i64>)>,
    /// `Some` quando o canal parou de vez (nao reconecta sozinho).
    pub terminal: Option<MotivoTerminal>,
}

impl SaudeDoCanal {
    /// `true` quando ha quedas seguidas o bastante para reportar `degraded`.
    ///
    /// Um canal **saudavel** (zero quedas) nunca entra aqui: e a garantia de
    /// que a mudanca nao afeta quem esta funcionando.
    pub fn degradado(&self) -> bool {
        self.terminal.is_none() && self.falhas_consecutivas >= LIMIAR_DEGRADADO
    }

    /// A linha que o `garra_status` mostra junto do status, ou `None`.
    pub fn motivo(&self) -> Option<String> {
        if let Some(terminal) = self.terminal {
            return Some(format!("parou de vez: {}", terminal.proximo_passo()));
        }
        let (motivo, codigo) = self.ultima_queda.as_ref()?;
        let mut linha = format!("ultimo motivo: {motivo}");
        if let Some(codigo) = codigo {
            linha.push_str(&format!(" (codigo {codigo})"));
        }
        if self.falhas_consecutivas > 1 {
            linha.push_str(&format!(", {} quedas seguidas", self.falhas_consecutivas));
        }
        Some(linha)
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
            (
                true,
                true,
                B::Down,
                H::BridgeDown,
                "vinculado e a ponte caiu",
            ),
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
            let pt = h
                .next_step(&dir, "garraia")
                .unwrap_or_else(|| panic!("{h:?} sem passo"));
            let en = h
                .next_step_en(&dir, "garraia")
                .unwrap_or_else(|| panic!("{h:?} sem passo em ingles"));
            assert!(!pt.trim().is_empty() && !en.trim().is_empty());
        }
        for h in [LinkHealth::Connected, LinkHealth::Linked] {
            assert!(
                h.next_step(&dir, "garraia").is_none(),
                "{h:?} nao tem o que consertar"
            );
            assert!(h.next_step_en(&dir, "garraia").is_none());
        }
    }

    /// O caminho real entra na frase — "rode `npm ci`" sozinho manda a pessoa
    /// procurar onde.
    #[test]
    fn o_passo_do_npm_ci_cita_o_diretorio_real() {
        let dir = PathBuf::from("/home/ana/.garraia/data/whatsapp/bridge");
        let passo = LinkHealth::MissingDependencies
            .next_step(&dir, "garraia")
            .expect("passo");
        assert!(passo.contains("npm ci"), "{passo}");
        assert!(
            passo.contains("/home/ana/.garraia/data/whatsapp/bridge"),
            "{passo}"
        );
        // O `npm ci` a mao so vale no boot seguinte do gateway, que e quem
        // adota a arvore: o passo diz isso, nos dois idiomas.
        assert!(passo.ends_with("e reinicie o gateway"), "{passo}");
        let en = LinkHealth::MissingDependencies
            .next_step_en(&dir, "garraia")
            .expect("step");
        assert!(en.ends_with("and restart the gateway"), "{en}");
    }

    /// O passo de "nao vinculado" cita o comando que existe. `<bin> whatsapp`
    /// abre o menu; `<bin> whatsapp link` e o que pareia.
    #[test]
    fn o_passo_de_nao_vinculado_cita_o_comando_de_link() {
        let passo = LinkHealth::NotLinked
            .next_step(Path::new("/x"), "garraia")
            .expect("passo");
        assert!(passo.contains("garraia whatsapp link"), "{passo}");
    }

    /// #1329: o passo nomeia o binario que o chamador passou, nos dois
    /// idiomas e nos dois estados que citam comando — nunca um `garra` fixo,
    /// que nao existe numa maquina que so tem `garraia`.
    #[test]
    fn o_passo_nomeia_o_binario_que_esta_na_maquina() {
        let dir = Path::new("/x");
        for h in [LinkHealth::NotLinked, LinkHealth::BridgeDown] {
            for bin in ["garraia", "garra"] {
                let pt = h.next_step(dir, bin).expect("passo");
                let en = h.next_step_en(dir, bin).expect("step");
                assert!(pt.contains(&format!("`{bin} whatsapp ")), "{pt}");
                assert!(en.contains(&format!("`{bin} whatsapp ")), "{en}");
            }
            let so_garraia = h.next_step(dir, "garraia").expect("passo");
            assert!(!so_garraia.contains("`garra whatsapp"), "{so_garraia}");
        }
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

    // ------------------------------------------------------------------
    // #1663: degradado, motivo e estado terminal.
    // ------------------------------------------------------------------

    /// Um canal **saudavel** — zero quedas — nunca vira `degraded`. E a
    /// garantia de que a mudanca nao toca em quem esta funcionando.
    #[test]
    fn canal_saudavel_nao_degrada() {
        let saude = SaudeDoCanal::default();
        assert_eq!(saude.falhas_consecutivas, 0);
        assert!(!saude.degradado());
        assert!(saude.motivo().is_none());
    }

    #[test]
    fn abaixo_do_limiar_nao_degrada() {
        let saude = SaudeDoCanal {
            falhas_consecutivas: LIMIAR_DEGRADADO - 1,
            ultima_queda: Some(("network".into(), Some(408))),
            terminal: None,
        };
        assert!(!saude.degradado());
    }

    #[test]
    fn no_limiar_degrada_e_mostra_o_motivo() {
        let saude = SaudeDoCanal {
            falhas_consecutivas: LIMIAR_DEGRADADO,
            ultima_queda: Some(("network".into(), Some(408))),
            terminal: None,
        };
        assert!(saude.degradado());
        let motivo = saude.motivo().expect("motivo");
        assert!(motivo.contains("network"), "{motivo}");
        assert!(motivo.contains("408"), "{motivo}");
        assert!(motivo.contains("3 quedas seguidas"), "{motivo}");
    }

    #[test]
    fn motivo_sem_codigo_nao_inventa_codigo() {
        let saude = SaudeDoCanal {
            falhas_consecutivas: 1,
            ultima_queda: Some(("timeout".into(), None)),
            terminal: None,
        };
        let motivo = saude.motivo().expect("motivo");
        assert!(motivo.contains("timeout"), "{motivo}");
        assert!(!motivo.contains("codigo"), "{motivo}");
        assert!(!motivo.contains("quedas seguidas"), "{motivo}");
    }

    #[test]
    fn estado_terminal_vence_o_degradado() {
        // Um canal que parou de vez nao e "degraded": e algo para o operador
        // re-vincular. Os dois nunca valem ao mesmo tempo.
        let saude = SaudeDoCanal {
            falhas_consecutivas: LIMIAR_DEGRADADO,
            ultima_queda: Some(("logged_out".into(), Some(401))),
            terminal: Some(MotivoTerminal::Deslogado),
        };
        assert!(!saude.degradado());
        let motivo = saude.motivo().expect("motivo");
        assert!(motivo.contains("parou de vez"), "{motivo}");
        assert!(motivo.contains("garra whatsapp link"), "{motivo}");
    }

    #[test]
    fn sessao_morta_aponta_para_re_vinculo() {
        let saude = SaudeDoCanal {
            terminal: Some(MotivoTerminal::SessaoMorta),
            ..SaudeDoCanal::default()
        };
        let motivo = saude.motivo().expect("motivo");
        assert!(motivo.contains("parou de vez"), "{motivo}");
        assert!(motivo.contains("vincular"), "{motivo}");
    }
}
