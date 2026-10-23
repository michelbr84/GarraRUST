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

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// Versao do formato persistido em `onboarding.json`. Sobe quando o formato
/// muda de forma incompativel; hoje 1.
pub const VERSAO_PERSISTIDA: u32 = 1;

/// Nome do arquivo que marca o estado do onboarding, em `app_config_dir()`.
/// Mesmo padrao do `chat-bar.json` da casca.
pub const ARQUIVO_ONBOARDING: &str = "onboarding.json";

/// Os argumentos do sidecar que iniciam o pareamento em modo-maquina (fatia 1).
///
/// E uma constante, nao uma montagem com opcoes, de proposito: o pareamento e
/// **igual** nos dois modos (`Proprio`/`Dedicado`) — o que muda e a politica de
/// acesso aplicada DEPOIS do `done` (ver [`args_aplicar_modo`]). O `--machine`
/// recusa `--allow`/`--owner` (fatia 1), entao nada de politica viaja aqui: o
/// canal nasce fail-closed e so abre quando a casca roda [`args_aplicar_modo`].
pub const ARGS_PAREAMENTO: [&str; 3] = ["whatsapp", "link", "--machine"];

/// Os argumentos do sidecar que aplicam a politica de acesso do modo escolhido,
/// rodados DEPOIS do pareamento concluir (ADR 0025; `docs/whatsapp.md`).
///
/// A semantica (confirmada em `docs/whatsapp.md` §Comandos e no `main.rs`
/// `AccessCommands`):
///
/// - [`Modo::Proprio`] → `whatsapp access open --yes`: o numero pessoal do
///   usuario, e o Garra responde **qualquer** um que mandar mensagem, com o
///   default do desconhecido. `open` AFROUXA a admissao (qualquer numero
///   entra), entao a tela de modo tem de dizer isso em letras grandes — e o
///   `--yes` existe porque, sem terminal, `open` sem confirmacao recusaria
///   (exit 64).
/// - [`Modo::Dedicado`] → `whatsapp access restricted`: so quem esta declarado
///   ou pareou por codigo fala com o Garra. E o **default seguro** (fail-closed)
///   da politica; aplica-lo explicitamente torna a escolha auditavel e, o que
///   importa para o botao de settings, **reverte** um `open` anterior quando o
///   usuario troca de volta para dedicado.
///
/// Nenhum dos dois AFROUXA um default de seguranca em silencio: `restricted` e
/// o proprio piso seguro, e `open` so e emitido quando o usuario escolhe
/// "proprio" numa tela que explica o que isso significa.
pub fn args_aplicar_modo(modo: Modo) -> Vec<&'static str> {
    match modo {
        Modo::Proprio => vec!["whatsapp", "access", "open", "--yes"],
        Modo::Dedicado => vec!["whatsapp", "access", "restricted"],
    }
}

/// O caminho de `onboarding.json` dentro de `config_dir` (a casca passa o
/// `app_config_dir()`). Separado para o teste cobrir a juncao sem tocar disco.
pub fn arquivo_onboarding(config_dir: &Path) -> PathBuf {
    config_dir.join(ARQUIVO_ONBOARDING)
}

/// `true` quando o wizard ainda deve abrir — ninguem concluiu o onboarding.
///
/// Fail-open de proposito: ausencia de arquivo (primeira execucao), arquivo
/// ilegivel ou `concluido: false` contam como primeiro uso. So um
/// [`OnboardingPersistido`] com `concluido: true` suprime o wizard — e e
/// exatamente o que a casca grava no fim ([`Onboarding::persistido`]).
pub fn primeiro_uso(persistido: Option<&OnboardingPersistido>) -> bool {
    !persistido.map(|p| p.concluido).unwrap_or(false)
}

/// Tamanho maximo (em chars) do nome da persona. Um nome mais longo e cortado.
pub const NOME_MAX_CHARS: usize = 40;

/// Limpa o nome da persona que vem da webview: tira caracteres de controle e
/// quebras de linha (que poluiriam o `system_prompt`) e corta em
/// [`NOME_MAX_CHARS`]. Nao sobe privilegio nem injeta nada — e um campo de
/// texto livre que vira prosa do prompt —, mas um nome com `\n` ou NUL deixaria
/// o YAML/sistema feio e e facil de barrar aqui, no lugar testado.
pub fn sanitizar_nome(nome: &str) -> String {
    let limpo: String = nome.chars().filter(|c| !c.is_control()).collect();
    limpo
        .trim()
        .chars()
        .take(NOME_MAX_CHARS)
        .collect::<String>()
        .trim()
        .to_string()
}

/// Normaliza um numero de telefone pessoal para o formato que o
/// `garraia whatsapp allow` aceita: `+<digitos>` em faixa E.164 (8 a 15
/// digitos, com DDI). Ignora espacos, hifens, parenteses e um `+` inicial.
///
/// Puro e testado: o numero do usuario (modo dedicado) e validado AQUI antes de
/// chegar ao CLI, para a tela recusar na hora um numero obviamente invalido em
/// vez de descobrir no fim. O CLI ainda valida de novo (exit 65) — isto nao o
/// substitui, so adianta o erro e padroniza o formato.
pub fn normalizar_numero(input: &str) -> Result<String, String> {
    let digitos: String = input.chars().filter(|c| c.is_ascii_digit()).collect();
    // MESMA faixa que o CLI `whatsapp allow` / o portao aceitam: 6 a 15 digitos
    // (E.164; Niue e Tokelau tem 7 — ver `acesso.rs` e
    // `whatsapp_linked_numero::normalizar_numero`). Usar um piso diferente aqui
    // recusaria um numero que o CLI gravaria, ou mandaria ao CLI um que ele
    // recusa — este passo so adianta o erro e padroniza, nunca diverge da regra.
    if digitos.len() < 6 || digitos.len() > 15 {
        return Err(format!(
            "numero invalido: use o formato com codigo do pais, ex.: +55 11 99999-8888 \
(tem de ter entre 6 e 15 digitos, veio com {})",
            digitos.len()
        ));
    }
    Ok(format!("+{digitos}"))
}

/// Argumentos do sidecar que autorizam um numero a falar com o Garra —
/// `garraia whatsapp allow <numero> --yes`.
///
/// E o caminho oficial do CLI (ADR 0025, `docs/whatsapp.md` §Comandos) para
/// "o dono fala com o proprio assistente": no modo **dedicado** o chip do Garra
/// e pareado e o usuario fala dele pelo numero PESSOAL, que precisa estar
/// autorizado — senao o portao fica com 0 autorizados e descarta o proprio
/// usuario em silencio (`acesso.rs`: o numero vinculado nao se autoriza
/// sozinho). Nao e `owner` de proposito: dono so existe em `isolated-pod`
/// (`execution.profile`), e o desktop roda em `standard`. O `--yes` so importa
/// para `--owner`, mas vai junto para blindar contra qualquer prompt sem TTY.
pub fn args_autorizar_numero(numero_normalizado: &str) -> Vec<String> {
    vec![
        "whatsapp".to_string(),
        "allow".to_string(),
        numero_normalizado.to_string(),
        "--yes".to_string(),
    ]
}

/// Monta a persona (`agent.system_prompt`) a partir do que o usuario escolheu
/// na tela de personalidade.
///
/// Puro e deterministico: a casca so grava a string de volta no `config.yml`
/// (via `ConfigLoader::set_agent_system_prompt`). O texto segue o idioma — o
/// default do projeto e pt-BR — e o tom muda com o [`EstiloPersona`]. O nome
/// passa por [`sanitizar_nome`]; vazio cai em "Garra". O prompt diz
/// explicitamente para responder no idioma escolhido, que e o que o runtime do
/// WhatsApp le.
pub fn montar_system_prompt(persona: &Personalidade) -> String {
    let nome_limpo = sanitizar_nome(&persona.nome);
    let nome = if nome_limpo.is_empty() {
        "Garra"
    } else {
        &nome_limpo
    };
    match persona.idioma {
        Idioma::PtBr => {
            let tom = match persona.estilo {
                EstiloPersona::Amigavel => "Seja caloroso, proximo e acolhedor",
                EstiloPersona::Profissional => "Seja objetivo, cordial e profissional",
                EstiloPersona::Divertido => "Seja leve, bem-humorado e descontraido",
            };
            format!(
                "Voce e {nome}, um assistente pessoal que conversa pelo WhatsApp. \
Responda sempre em portugues do Brasil. {tom}. \
Seja breve e util, e peca o que faltar antes de agir."
            )
        }
        Idioma::En => {
            let tom = match persona.estilo {
                EstiloPersona::Amigavel => "Be warm, close and welcoming",
                EstiloPersona::Profissional => "Be objective, polite and professional",
                EstiloPersona::Divertido => "Be light, good-humored and relaxed",
            };
            format!(
                "You are {nome}, a personal assistant chatting over WhatsApp. \
Always reply in English. {tom}. \
Be brief and helpful, and ask for missing details before acting."
            )
        }
    }
}

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

    // --- Fatia 3: costura com a casca (comando do sidecar, modo->config, persona) ---

    #[test]
    fn o_pareamento_e_so_link_machine_sem_politica() {
        // O pareamento e igual nos dois modos; a politica so entra depois.
        assert_eq!(ARGS_PAREAMENTO, ["whatsapp", "link", "--machine"]);
        // Nunca pode carregar --allow/--owner (o --machine os recusa) nem um
        // suposto --modo: a politica e um passo separado.
        for proibido in ["--allow", "--owner", "--modo"] {
            assert!(
                !ARGS_PAREAMENTO.contains(&proibido),
                "o pareamento nao pode carregar {proibido}"
            );
        }
    }

    #[test]
    fn modo_proprio_abre_a_admissao_e_o_dedicado_a_fecha() {
        // Proprio = qualquer um fala (open), e sem terminal precisa do --yes.
        let proprio = args_aplicar_modo(Modo::Proprio);
        assert_eq!(proprio, ["whatsapp", "access", "open", "--yes"]);
        assert!(
            proprio.contains(&"--yes"),
            "sem terminal o open precisa de --yes, senao recusa (exit 64)"
        );

        // Dedicado = o default seguro (fail-closed). NUNCA pode virar open.
        let dedicado = args_aplicar_modo(Modo::Dedicado);
        assert_eq!(dedicado, ["whatsapp", "access", "restricted"]);
        assert!(
            !dedicado.contains(&"open"),
            "o modo dedicado nunca pode afrouxar a admissao"
        );
    }

    #[test]
    fn o_arquivo_de_onboarding_fica_no_config_dir() {
        let p = arquivo_onboarding(Path::new("/tmp/cfg"));
        assert_eq!(p.file_name().unwrap(), "onboarding.json");
        assert!(p.ends_with("onboarding.json"));
    }

    #[test]
    fn primeiro_uso_so_e_falso_com_concluido_true() {
        // Sem arquivo (None) ou ilegivel -> primeiro uso (fail-open).
        assert!(primeiro_uso(None), "sem estado persistido abre o wizard");
        // concluido: false -> ainda e primeiro uso.
        let incompleto = OnboardingPersistido {
            concluido: false,
            modo: Some(Modo::Proprio),
            versao: VERSAO_PERSISTIDA,
        };
        assert!(primeiro_uso(Some(&incompleto)));
        // concluido: true -> nao abre mais.
        let feito = OnboardingPersistido {
            concluido: true,
            modo: Some(Modo::Dedicado),
            versao: VERSAO_PERSISTIDA,
        };
        assert!(!primeiro_uso(Some(&feito)), "concluido suprime o wizard");
    }

    #[test]
    fn o_system_prompt_segue_idioma_nome_e_tom() {
        let base = Personalidade {
            nome: "Papa-Léguas".into(),
            estilo: EstiloPersona::Amigavel,
            idioma: Idioma::PtBr,
        };
        let pt = montar_system_prompt(&base);
        assert!(pt.contains("Papa-Léguas"), "o nome escolhido aparece: {pt}");
        assert!(
            pt.contains("portugues do Brasil"),
            "pt-BR instrui a responder em pt-BR: {pt}"
        );

        // O idioma troca o prompt inteiro.
        let en = montar_system_prompt(&Personalidade {
            idioma: Idioma::En,
            ..base.clone()
        });
        assert!(en.contains("English"), "en instrui a responder em ingles");
        assert_ne!(pt, en, "idioma muda o prompt");

        // O tom troca com o estilo.
        let prof = montar_system_prompt(&Personalidade {
            estilo: EstiloPersona::Profissional,
            ..base.clone()
        });
        let div = montar_system_prompt(&Personalidade {
            estilo: EstiloPersona::Divertido,
            ..base.clone()
        });
        assert_ne!(pt, prof, "amigavel != profissional");
        assert_ne!(prof, div, "profissional != divertido");

        // Nome vazio cai em "Garra".
        let anon = montar_system_prompt(&Personalidade {
            nome: "   ".into(),
            ..base
        });
        assert!(anon.contains("Garra"), "nome vazio cai em Garra: {anon}");
    }

    #[test]
    fn o_nome_e_saneado_sem_controle_nem_newline_e_com_teto() {
        // Controle e quebra de linha somem.
        assert_eq!(sanitizar_nome("Ga\nrra\t\u{7}"), "Garra");
        // Espacos nas pontas vao embora.
        assert_eq!(sanitizar_nome("  Garra  "), "Garra");
        // Teto de NOME_MAX_CHARS chars (conta chars, nao bytes).
        let longo = "á".repeat(80);
        assert_eq!(sanitizar_nome(&longo).chars().count(), NOME_MAX_CHARS);
        // So controle vira vazio (montar_system_prompt entao cai em Garra).
        assert_eq!(sanitizar_nome("\n\t\r"), "");
        let prompt = montar_system_prompt(&Personalidade {
            nome: "Garra\nINJETADO".into(),
            estilo: EstiloPersona::Amigavel,
            idioma: Idioma::PtBr,
        });
        assert!(
            prompt.contains("GarraINJETADO"),
            "a quebra de linha some e o resto fica colado: {prompt}"
        );
    }

    #[test]
    fn normalizar_numero_aceita_e164_e_recusa_o_resto() {
        // Formato humano com DDI vira +digitos.
        assert_eq!(
            normalizar_numero("+55 11 99999-8888").unwrap(),
            "+5511999998888"
        );
        assert_eq!(
            normalizar_numero("(55) 11 98888 0000").unwrap(),
            "+5511988880000"
        );
        // Piso de 6 digitos, igual ao CLI (Niue/Tokelau tem 7): 6 passa, 5 nao.
        assert_eq!(normalizar_numero("683 1234").unwrap(), "+6831234");
        assert_eq!(normalizar_numero("123456").unwrap(), "+123456");
        // Curto demais (< 6), longo demais (> 15) e vazio sao recusados.
        for ruim in ["", "123", "12345", "0123456789012345678", "abc", "+"] {
            assert!(
                normalizar_numero(ruim).is_err(),
                "{ruim:?} tinha de ser recusado"
            );
        }
    }

    #[test]
    fn args_autorizar_numero_usa_allow_com_o_numero_normalizado() {
        let n = normalizar_numero("+55 11 99999-8888").unwrap();
        assert_eq!(
            args_autorizar_numero(&n),
            vec!["whatsapp", "allow", "+5511999998888", "--yes"]
        );
        // Nunca `--owner`: dono so existe em isolated-pod; o desktop e standard.
        assert!(
            !args_autorizar_numero(&n).iter().any(|a| a == "--owner"),
            "autorizar o proprio usuario nunca e via owner no desktop"
        );
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
