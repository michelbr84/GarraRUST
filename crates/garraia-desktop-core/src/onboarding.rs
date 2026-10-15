//! Maquina de estados do Assistente de Primeiro Uso (onboarding) do desktop.
//!
//! # Forma
//!
//! Pura e deterministica, no mesmo espirito do [`crate::state`] e do
//! `spinner.rs` da CLI: nada aqui le relogio, dorme, abre socket ou toca
//! disco. A tela so muda quando alguem chama uma acao ([`Onboarding::avancar`],
//! [`Onboarding::voltar`], [`Onboarding::escolher_modo`], …) ou alimenta um
//! evento do pareamento ([`Onboarding::aplicar_evento`]). Quem tem o relogio, o
//! sidecar e a janela e a casca Tauri; aqui mora so a logica, que entra nos
//! gates de CI e tem teste.
//!
//! # O fluxo
//!
//! ```text
//! BoasVindas → ModoWhatsApp → Qr(fase) → Personalidade → Pronto
//! ```
//!
//! A tela [`Tela::Qr`] carrega a **fase do pareamento**, mapeada dos eventos
//! NDJSON que o `garra whatsapp link --machine` emite (fatia 1, #1636): o
//! consumidor parseia cada linha num [`EventoNdjson`] e o entrega a
//! [`Onboarding::aplicar_evento`]. Eventos e status desconhecidos sao
//! **ignorados** de proposito — uma fase nova no futuro nao pode travar uma
//! versao antiga do app.
//!
//! # Invariantes
//!
//! - Nenhum estado terminal avanca sozinho: so uma acao move a tela.
//! - [`FaseQr::Falhou`] nao e beco sem saida — [`Onboarding::tentar_de_novo`]
//!   volta a [`FaseQr::Conectando`], de onde um `qr` novo traz a
//!   [`FaseQr::AguardandoQr`] seguinte.
//! - `Avancar` so passa da tela do QR quando o pareamento **concluiu**; nao da
//!   para pular o vinculo.

use serde::{Deserialize, Serialize};

/// Versao do formato persistido em `onboarding.json`. Sobe quando o formato
/// muda de forma incompativel; hoje 1.
pub const VERSAO_PERSISTIDA: u32 = 1;

/// Como o WhatsApp sera usado. A diferenca e de **politica de acesso**, nao de
/// mecanismo: os dois pareiam igual (ver ADR 0025). `Proprio` deixa
/// desconhecidos falarem num nivel; `Dedicado` e default-deny.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Modo {
    /// O numero pessoal do usuario.
    Proprio,
    /// Um numero separado, so para o GarraIA.
    Dedicado,
}

/// O estilo da persona que o assistente assume.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EstiloPersona {
    Amigavel,
    Profissional,
    Divertido,
}

/// O idioma da persona. Default do projeto: pt-BR.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Idioma {
    /// Default do projeto.
    #[default]
    PtBr,
    En,
}

/// O que o usuario respondeu na tela de personalidade. Em construcao enquanto a
/// tela esta aberta; gravado como persona (`agent.system_prompt`) pela casca,
/// fora desta crate.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Personalidade {
    /// Nome que o assistente usa. Vazio = sem nome escolhido ainda.
    pub nome: String,
    pub estilo: EstiloPersona,
    pub idioma: Idioma,
}

impl Default for Personalidade {
    fn default() -> Self {
        Self {
            nome: String::new(),
            estilo: EstiloPersona::Amigavel,
            idioma: Idioma::PtBr,
        }
    }
}

/// A fase do pareamento, mapeada dos eventos NDJSON da fatia 1.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "fase")]
pub enum FaseQr {
    /// Conectando / preparando o bridge, antes de o QR aparecer.
    Conectando,
    /// Um QR esta na tela, aguardando leitura.
    AguardandoQr { attempt: u32, max: u32 },
    /// O QR foi lido; sincronizando.
    Autenticado,
    /// Pareado e canal ligado.
    Concluido,
    /// Falhou; `code` e o `code` do evento `error` (snake_case).
    Falhou { code: String },
}

/// A tela atual do wizard.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "tela")]
pub enum Tela {
    /// Primeira tela: apresentacao.
    BoasVindas,
    /// Escolha entre numero proprio e dedicado.
    ModoWhatsApp,
    /// O pareamento por QR, com a fase corrente.
    Qr(FaseQr),
    /// Ajuste da persona.
    Personalidade(Personalidade),
    /// Fim: tudo pronto.
    Pronto,
}

/// Um evento NDJSON do `garra whatsapp link --machine` (esquema `schema: 1` da
/// fatia 1). Desserializavel com serde; o consumidor parseia cada linha do
/// stdout do sidecar.
///
/// Tolerante a futuro: um `event` que esta crate nao conhece vira
/// [`EventoNdjson::Desconhecido`] (via `#[serde(other)]`), e campos extras
/// (como `schema`) sao ignorados. Assim, acrescentar evento na CLI nao quebra
/// uma versao antiga do app.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum EventoNdjson {
    /// Progresso: o `status` nomeia a fase (`connecting`, `authenticated`,
    /// `done`, `waiting_qr`, …). String crua para tolerar fase nova.
    Status { status: String },
    /// Um QR novo e sua matriz.
    Qr {
        #[serde(default)]
        attempt: u32,
        #[serde(default)]
        max_attempts: u32,
        #[serde(default)]
        previous_expired: bool,
        #[serde(default)]
        width: u32,
        #[serde(default)]
        modules: Vec<String>,
    },
    /// Falha terminal; `code` em snake_case.
    Error {
        code: String,
        #[serde(default)]
        message: Option<String>,
    },
    /// Texto de progresso, cosmetico.
    Log {
        #[serde(default)]
        message: Option<String>,
    },
    /// Qualquer `event` que esta versao nao conhece.
    #[serde(other)]
    Desconhecido,
}

impl EventoNdjson {
    /// A fase do QR que este evento implica, ou `None` quando ele nao muda a
    /// fase (texto cosmetico, status pre-QR sem informacao nova, desconhecido).
    ///
    /// O mapeamento e proposital:
    /// - `qr` → [`FaseQr::AguardandoQr`] (so ele traz `attempt`/`max`);
    /// - progresso pre-QR (`connecting`, `detecting_node`, `installing_deps`,
    ///   `deps_ready`, `starting_bridge`) → [`FaseQr::Conectando`];
    /// - pos-scan (`authenticated`, `session_saved`, `channel_enabled`) →
    ///   [`FaseQr::Autenticado`];
    /// - `done` → [`FaseQr::Concluido`];
    /// - `error` → [`FaseQr::Falhou`];
    /// - `waiting_qr`, `log`, desconhecido → `None` (mantem a fase atual).
    pub fn fase(&self) -> Option<FaseQr> {
        match self {
            EventoNdjson::Qr {
                attempt,
                max_attempts,
                ..
            } => Some(FaseQr::AguardandoQr {
                attempt: *attempt,
                max: *max_attempts,
            }),
            EventoNdjson::Status { status } => match status.as_str() {
                "connecting" | "detecting_node" | "installing_deps" | "deps_ready"
                | "starting_bridge" => Some(FaseQr::Conectando),
                "authenticated" | "session_saved" | "channel_enabled" => Some(FaseQr::Autenticado),
                "done" => Some(FaseQr::Concluido),
                // `waiting_qr` nao tem `attempt`; o evento `qr` ja pos a fase.
                _ => None,
            },
            EventoNdjson::Error { code, .. } => Some(FaseQr::Falhou { code: code.clone() }),
            EventoNdjson::Log { .. } | EventoNdjson::Desconhecido => None,
        }
    }
}

/// O que persiste em `onboarding.json` (padrao do `chat-bar.json`). A casca
/// grava e le; aqui fica so o tipo e o round-trip.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OnboardingPersistido {
    /// O usuario ja concluiu o onboarding?
    pub concluido: bool,
    /// O modo escolhido, quando houve escolha.
    pub modo: Option<Modo>,
    /// Versao do formato ([`VERSAO_PERSISTIDA`]).
    pub versao: u32,
}

/// A maquina de estados do wizard. Guarda a tela, o modo escolhido e a persona
/// em construcao; o `cancelado` e uma saida de uma via.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Onboarding {
    tela: Tela,
    modo: Option<Modo>,
    personalidade: Personalidade,
    cancelado: bool,
}

impl Default for Onboarding {
    fn default() -> Self {
        Self::novo()
    }
}

impl Onboarding {
    /// Comeca em [`Tela::BoasVindas`].
    pub fn novo() -> Self {
        Self {
            tela: Tela::BoasVindas,
            modo: None,
            personalidade: Personalidade::default(),
            cancelado: false,
        }
    }

    /// A tela corrente.
    pub fn tela(&self) -> &Tela {
        &self.tela
    }

    /// O modo escolhido, quando houve escolha.
    pub fn modo(&self) -> Option<Modo> {
        self.modo
    }

    /// A persona em construcao.
    pub fn personalidade(&self) -> &Personalidade {
        &self.personalidade
    }

    /// O usuario cancelou o wizard?
    pub fn cancelado(&self) -> bool {
        self.cancelado
    }

    /// Escolhe o modo na tela [`Tela::ModoWhatsApp`]. Sem efeito em outra tela
    /// ou depois de cancelar.
    pub fn escolher_modo(&mut self, modo: Modo) {
        if self.cancelado {
            return;
        }
        if self.tela == Tela::ModoWhatsApp {
            self.modo = Some(modo);
        }
    }

    /// Define a persona na tela [`Tela::Personalidade`]. Sem efeito em outra
    /// tela ou depois de cancelar.
    pub fn definir_persona(&mut self, persona: Personalidade) {
        if self.cancelado {
            return;
        }
        if matches!(self.tela, Tela::Personalidade(_)) {
            self.personalidade = persona.clone();
            self.tela = Tela::Personalidade(persona);
        }
    }

    /// Avanca para a proxima tela, quando a tela atual permite.
    ///
    /// Guardas: `ModoWhatsApp` so passa com um modo escolhido; `Qr` so passa
    /// quando a fase e [`FaseQr::Concluido`]. Nos demais casos e um no-op — nao
    /// da para pular o vinculo nem escolher modo nenhum.
    pub fn avancar(&mut self) {
        if self.cancelado {
            return;
        }
        self.tela = match &self.tela {
            Tela::BoasVindas => Tela::ModoWhatsApp,
            Tela::ModoWhatsApp if self.modo.is_some() => Tela::Qr(FaseQr::Conectando),
            Tela::Qr(FaseQr::Concluido) => Tela::Personalidade(self.personalidade.clone()),
            Tela::Personalidade(_) => Tela::Pronto,
            // ModoWhatsApp sem modo, Qr nao concluido, Pronto: ficam.
            outra => outra.clone(),
        };
    }

    /// Volta uma tela, quando faz sentido.
    ///
    /// `BoasVindas` e `Pronto` nao voltam sozinhas (terminais nas pontas);
    /// `Qr` volta para a escolha de modo (re-parear do zero); `Personalidade`
    /// volta para a tela do QR ja concluida.
    pub fn voltar(&mut self) {
        if self.cancelado {
            return;
        }
        self.tela = match &self.tela {
            Tela::ModoWhatsApp => Tela::BoasVindas,
            Tela::Qr(_) => Tela::ModoWhatsApp,
            Tela::Personalidade(_) => Tela::Qr(FaseQr::Concluido),
            // BoasVindas e Pronto: ficam.
            outra => outra.clone(),
        };
    }

    /// Cancela o wizard. A partir daqui toda acao e no-op.
    pub fn cancelar(&mut self) {
        self.cancelado = true;
    }

    /// Alimenta um evento NDJSON do sidecar. So tem efeito na tela [`Tela::Qr`]:
    /// atualiza a fase conforme [`EventoNdjson::fase`]; eventos que nao mudam a
    /// fase (ou fora da tela do QR) sao ignorados.
    pub fn aplicar_evento(&mut self, evento: &EventoNdjson) {
        if self.cancelado {
            return;
        }
        if let Tela::Qr(_) = self.tela
            && let Some(fase) = evento.fase()
        {
            self.tela = Tela::Qr(fase);
        }
    }

    /// Depois de [`FaseQr::Falhou`], volta a [`FaseQr::Conectando`] — o sidecar
    /// recomeca o pareamento do zero, e um `qr` novo traz a [`FaseQr::AguardandoQr`]
    /// seguinte. Sem efeito se a tela nao for um QR falho.
    pub fn tentar_de_novo(&mut self) {
        if self.cancelado {
            return;
        }
        if matches!(self.tela, Tela::Qr(FaseQr::Falhou { .. })) {
            self.tela = Tela::Qr(FaseQr::Conectando);
        }
    }

    /// Snapshot para gravar em `onboarding.json`.
    pub fn persistido(&self) -> OnboardingPersistido {
        OnboardingPersistido {
            concluido: self.tela == Tela::Pronto,
            modo: self.modo,
            versao: VERSAO_PERSISTIDA,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Linha `qr` real, no formato que o `garra whatsapp link --machine` emite
    /// (fatia 1, #1636) — a ordem dos campos nao importa para o serde.
    const LINHA_QR: &str = r#"{"schema":1,"event":"qr","attempt":1,"max_attempts":5,"previous_expired":false,"width":3,"modules":["101","010","111"]}"#;

    fn parse(linha: &str) -> EventoNdjson {
        serde_json::from_str(linha).unwrap_or_else(|e| panic!("parse {linha:?}: {e}"))
    }

    #[test]
    fn o_fluxo_feliz_vai_de_boas_vindas_a_pronto() {
        let mut w = Onboarding::novo();
        assert_eq!(w.tela(), &Tela::BoasVindas);

        w.avancar();
        assert_eq!(w.tela(), &Tela::ModoWhatsApp);

        w.escolher_modo(Modo::Proprio);
        assert_eq!(w.modo(), Some(Modo::Proprio));
        w.avancar();
        assert_eq!(w.tela(), &Tela::Qr(FaseQr::Conectando));

        // O sidecar vai falando: QR, autenticado, concluido.
        w.aplicar_evento(&parse(LINHA_QR));
        assert_eq!(
            w.tela(),
            &Tela::Qr(FaseQr::AguardandoQr { attempt: 1, max: 5 })
        );
        w.aplicar_evento(&parse(
            r#"{"schema":1,"event":"status","status":"authenticated"}"#,
        ));
        assert_eq!(w.tela(), &Tela::Qr(FaseQr::Autenticado));
        w.aplicar_evento(&parse(r#"{"schema":1,"event":"status","status":"done"}"#));
        assert_eq!(w.tela(), &Tela::Qr(FaseQr::Concluido));

        w.avancar();
        assert!(matches!(w.tela(), Tela::Personalidade(_)));

        w.definir_persona(Personalidade {
            nome: "Garra".into(),
            estilo: EstiloPersona::Divertido,
            idioma: Idioma::PtBr,
        });
        assert_eq!(w.personalidade().nome, "Garra");
        w.avancar();
        assert_eq!(w.tela(), &Tela::Pronto);

        let p = w.persistido();
        assert!(p.concluido);
        assert_eq!(p.modo, Some(Modo::Proprio));
        assert_eq!(p.versao, VERSAO_PERSISTIDA);
    }

    #[test]
    fn a_tela_de_modo_nao_avanca_sem_escolha() {
        let mut w = Onboarding::novo();
        w.avancar(); // BoasVindas -> ModoWhatsApp
        assert_eq!(w.tela(), &Tela::ModoWhatsApp);
        w.avancar(); // sem modo: no-op
        assert_eq!(w.tela(), &Tela::ModoWhatsApp, "sem modo nao passa");
        w.escolher_modo(Modo::Dedicado);
        w.avancar();
        assert_eq!(w.tela(), &Tela::Qr(FaseQr::Conectando));
    }

    #[test]
    fn nao_da_para_pular_o_vinculo() {
        let mut w = Onboarding::novo();
        w.avancar();
        w.escolher_modo(Modo::Proprio);
        w.avancar(); // -> Qr(Conectando)
        w.avancar(); // QR nao concluido: no-op
        assert_eq!(
            w.tela(),
            &Tela::Qr(FaseQr::Conectando),
            "avancar nao pula o pareamento inacabado"
        );
    }

    #[test]
    fn voltar_percorre_as_telas_e_as_pontas_ficam() {
        let mut w = Onboarding::novo();
        w.voltar();
        assert_eq!(w.tela(), &Tela::BoasVindas, "BoasVindas e ponta: nao volta");

        w.avancar(); // ModoWhatsApp
        w.voltar();
        assert_eq!(w.tela(), &Tela::BoasVindas);

        w.avancar();
        w.escolher_modo(Modo::Proprio);
        w.avancar(); // Qr
        w.voltar();
        assert_eq!(w.tela(), &Tela::ModoWhatsApp, "do QR volta para o modo");
    }

    #[test]
    fn nenhum_terminal_avanca_sozinho() {
        // BoasVindas nao se move sem acao.
        let w = Onboarding::novo();
        assert_eq!(w.tela(), &Tela::BoasVindas);

        // Pronto fica em Pronto mesmo chamando avancar/voltar.
        let mut w = Onboarding::novo();
        w.avancar();
        w.escolher_modo(Modo::Proprio);
        w.avancar();
        w.aplicar_evento(&parse(r#"{"schema":1,"event":"status","status":"done"}"#));
        w.avancar(); // Personalidade
        w.avancar(); // Pronto
        assert_eq!(w.tela(), &Tela::Pronto);
        w.avancar();
        assert_eq!(w.tela(), &Tela::Pronto, "Pronto e terminal");
    }

    #[test]
    fn falhou_permite_tentar_de_novo() {
        let mut w = Onboarding::novo();
        w.avancar();
        w.escolher_modo(Modo::Proprio);
        w.avancar(); // Qr(Conectando)
        w.aplicar_evento(&parse(
            r#"{"schema":1,"event":"error","code":"qr_expired","message":"Nenhum QR foi lido."}"#,
        ));
        assert_eq!(
            w.tela(),
            &Tela::Qr(FaseQr::Falhou {
                code: "qr_expired".into()
            })
        );
        w.tentar_de_novo();
        assert_eq!(
            w.tela(),
            &Tela::Qr(FaseQr::Conectando),
            "tentar de novo volta a conectar/aguardar"
        );
    }

    #[test]
    fn cancelar_congela_toda_acao() {
        let mut w = Onboarding::novo();
        w.cancelar();
        assert!(w.cancelado());
        w.avancar();
        w.escolher_modo(Modo::Proprio);
        w.aplicar_evento(&parse(LINHA_QR));
        assert_eq!(w.tela(), &Tela::BoasVindas, "cancelado nao muda mais nada");
        assert_eq!(w.modo(), None);
    }

    #[test]
    fn cada_evento_ndjson_parseia_e_mapeia_a_fase() {
        // qr: traz attempt/max.
        let qr = parse(LINHA_QR);
        assert_eq!(
            qr,
            EventoNdjson::Qr {
                attempt: 1,
                max_attempts: 5,
                previous_expired: false,
                width: 3,
                modules: vec!["101".into(), "010".into(), "111".into()],
            }
        );
        assert_eq!(qr.fase(), Some(FaseQr::AguardandoQr { attempt: 1, max: 5 }));

        let casos = [
            (
                r#"{"schema":1,"event":"status","status":"connecting"}"#,
                Some(FaseQr::Conectando),
            ),
            (
                r#"{"schema":1,"event":"status","status":"detecting_node"}"#,
                Some(FaseQr::Conectando),
            ),
            (
                r#"{"schema":1,"event":"status","status":"authenticated"}"#,
                Some(FaseQr::Autenticado),
            ),
            (
                r#"{"schema":1,"event":"status","status":"channel_enabled"}"#,
                Some(FaseQr::Autenticado),
            ),
            (
                r#"{"schema":1,"event":"status","status":"done"}"#,
                Some(FaseQr::Concluido),
            ),
            // waiting_qr sem attempt nao mexe na fase.
            (
                r#"{"schema":1,"event":"status","status":"waiting_qr"}"#,
                None,
            ),
            (
                r#"{"schema":1,"event":"error","code":"bridge_failed","message":"x"}"#,
                Some(FaseQr::Falhou {
                    code: "bridge_failed".into(),
                }),
            ),
            (
                r#"{"schema":1,"event":"log","message":"conectando…"}"#,
                None,
            ),
        ];
        for (linha, esperada) in casos {
            assert_eq!(parse(linha).fase(), esperada, "linha: {linha}");
        }
    }

    #[test]
    fn evento_desconhecido_vira_desconhecido_e_e_ignorado() {
        // Um `event` que esta versao nao conhece (compatibilidade futura).
        let ev = parse(r#"{"schema":2,"event":"garra_fase_nova","algo":42}"#);
        assert_eq!(ev, EventoNdjson::Desconhecido);
        assert_eq!(ev.fase(), None);

        // E, alimentado na tela do QR, nao mexe na fase.
        let mut w = Onboarding::novo();
        w.avancar();
        w.escolher_modo(Modo::Proprio);
        w.avancar(); // Qr(Conectando)
        w.aplicar_evento(&ev);
        assert_eq!(w.tela(), &Tela::Qr(FaseQr::Conectando));
    }

    #[test]
    fn eventos_fora_da_tela_do_qr_sao_ignorados() {
        let mut w = Onboarding::novo(); // BoasVindas
        w.aplicar_evento(&parse(r#"{"schema":1,"event":"status","status":"done"}"#));
        assert_eq!(w.tela(), &Tela::BoasVindas, "evento so vale na tela do QR");
    }

    #[test]
    fn round_trip_do_persistido() {
        for p in [
            OnboardingPersistido {
                concluido: true,
                modo: Some(Modo::Dedicado),
                versao: VERSAO_PERSISTIDA,
            },
            OnboardingPersistido {
                concluido: false,
                modo: None,
                versao: VERSAO_PERSISTIDA,
            },
        ] {
            let json = serde_json::to_string(&p).expect("serialize");
            let de: OnboardingPersistido = serde_json::from_str(&json).expect("deserialize");
            assert_eq!(p, de, "round-trip via {json}");
        }
    }

    #[test]
    fn o_idioma_default_e_pt_br() {
        assert_eq!(Idioma::default(), Idioma::PtBr);
        assert_eq!(Personalidade::default().idioma, Idioma::PtBr);
    }

    /// Tabela de transicoes: de uma tela, uma acao leva a outra tela.
    #[test]
    fn tabela_de_transicoes() {
        // (prepara, acao, tela esperada)
        type Acao = fn(&mut Onboarding);
        let avancar: Acao = |w| w.avancar();
        let voltar: Acao = |w| w.voltar();

        // BoasVindas --avancar--> ModoWhatsApp
        let mut w = Onboarding::novo();
        avancar(&mut w);
        assert_eq!(w.tela(), &Tela::ModoWhatsApp);

        // ModoWhatsApp(+modo) --avancar--> Qr(Conectando)
        w.escolher_modo(Modo::Proprio);
        avancar(&mut w);
        assert_eq!(w.tela(), &Tela::Qr(FaseQr::Conectando));

        // Qr(Concluido) --avancar--> Personalidade
        w.aplicar_evento(&parse(r#"{"schema":1,"event":"status","status":"done"}"#));
        avancar(&mut w);
        assert!(matches!(w.tela(), Tela::Personalidade(_)));

        // Personalidade --voltar--> Qr(Concluido)
        voltar(&mut w);
        assert_eq!(w.tela(), &Tela::Qr(FaseQr::Concluido));

        // Personalidade --avancar--> Pronto
        avancar(&mut w); // volta para Personalidade? nao: estamos em Qr(Concluido) agora
        // (reposiciona: Qr(Concluido) --avancar--> Personalidade)
        assert!(matches!(w.tela(), Tela::Personalidade(_)));
    }
}
