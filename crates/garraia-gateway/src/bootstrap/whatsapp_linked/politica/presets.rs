//! Presets nomeados sobre [`Alcance`] (#1434; ADR 0025 §3-4).
//!
//! Um preset e so um [`Alcance`] canonico com nome: a autoridade continua
//! sendo `level`/`write` no config, nunca um rotulo separado. Por isso a
//! mesma `Alcance` sempre resolve para o mesmo nome (ou `custom`, se nao bate
//! com nenhum preset) — nao ha estado paralelo que possa divergir do que o
//! portao realmente aplica, e um preset futuro so pode ESTREITAR (nunca
//! alargar em silencio), porque [`politica_do_nivel`] (garraia-agents) ja e a
//! unica fonte de verdade do que cada nivel libera.
//!
//! [`politica_do_nivel`]: garraia_agents::modes::politica_do_nivel

use super::Alcance;

/// Um preset nomeado.
///
/// [`Preset::Developer`] e [`Preset::FullPod`] compilam para a MESMA
/// [`Alcance`] ([`Alcance::COMPLETO`]): ADR 0025 §3 e explicito que `full` e
/// "sem restricao propria — o modo e o perfil de execucao decidem". A
/// distincao entre os dois nomes e so a intencao de quem atribuiu, no
/// instante da atribuicao — nao existe em NENHUM lugar depois disso (nem na
/// config, nem no audit: `Mutacao::acao()` devolve `"preset"` para as duas).
/// De proposito: guardar essa intencao em algum estado que a leitura possa
/// reafirmar sozinha e exatamente o tipo de campo que fica dessincronizado e
/// um dia mente sobre o que o portao aplica. Quem le a politica depois so
/// pode saber que a identidade tem `full` — nunca qual dos dois nomes foi
/// digitado.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Preset {
    /// [`Alcance::CHAT`]: nenhuma ferramenta. Seguro para qualquer contexto,
    /// inclusive remoto/nao confiavel — e o que `no_tools` sempre foi.
    ChatOnly,
    /// [`Alcance::LEITURA`]: leitura (arquivo, web, dispositivo, memoria,
    /// inspecao do runtime, MCP somente-leitura); nunca escrita, shell,
    /// dispositivo de acao, mensagem ou agendamento. Seguro para contexto
    /// remoto/nao confiavel (a mesma classe que `LEITURA` sempre teve).
    Read,
    /// [`Alcance::COMPLETO`] (`full` + `write`). Pensado para o perfil
    /// `standard`: o piso de modo e o perfil de execucao continuam sendo quem
    /// decide o que de fato fica disponivel (ADR 0025 §3/§4; ADR 0024 nunca
    /// registra `bash` sem sandbox) — o preset nunca abre mais do que o
    /// perfil permite.
    Developer,
    /// [`Alcance::COMPLETO`] (`full` + `write`), a MESMA `Alcance` de
    /// [`Preset::Developer`]. Pensado para `execution.profile =
    /// isolated-pod`, onde o piso do dono de fato chega a `code` (com
    /// `bash`). A distincao com `Developer` e so a intencao operacional de
    /// quem atribuiu — nao fica registrada em lugar nenhum depois (nem
    /// config, nem audit); o teto que o portao aplica e identico, e SEMPRE
    /// limitado pelo perfil de execucao real E pelo piso de modo
    /// (`default_mode`), nunca pelo nome do preset.
    FullPod,
}

impl Preset {
    /// Todos, do mais estreito ao mais amplo — os dois ultimos empatam no
    /// topo (`Developer` e `FullPod` sao a MESMA `Alcance`; ver o enum).
    pub const ALL: [Preset; 4] = [Self::ChatOnly, Self::Read, Self::Developer, Self::FullPod];

    /// O nome estavel (config, CLI, API, audit). Minusculo, `snake_case`.
    pub fn nome(self) -> &'static str {
        match self {
            Self::ChatOnly => "chat_only",
            Self::Read => "read",
            Self::Developer => "developer",
            Self::FullPod => "full_pod",
        }
    }

    /// Aceita o nome canonico com `_` ou `-`. Vazio ou desconhecido: `None`
    /// (fail-closed — quem chama decide o erro).
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().replace('-', "_").as_str() {
            "chat_only" => Some(Self::ChatOnly),
            "read" => Some(Self::Read),
            "developer" => Some(Self::Developer),
            "full_pod" => Some(Self::FullPod),
            _ => None,
        }
    }

    /// O [`Alcance`] canonico deste preset — a UNICA coisa que o motor de
    /// mutacao grava (`level` + `write`); nunca um rotulo a parte.
    pub fn alcance(self) -> Alcance {
        match self {
            Self::ChatOnly => Alcance::CHAT,
            Self::Read => Alcance::LEITURA,
            Self::Developer | Self::FullPod => Alcance::COMPLETO,
        }
    }
}

/// Os nomes validos, para a mensagem de erro de quem recusa (CLI e API).
pub const NOMES: &str = "chat_only | read | developer | full_pod";

/// O rotulo de exibicao de uma [`Alcance`] efetiva (CLI, API e console): o
/// nome do preset quando ela bate EXATAMENTE com um, senao `custom`.
///
/// Computado ao vivo a partir do `level`/`write` autoritativo — nunca de um
/// campo armazenado — entao nao ha como um rotulo ficar preso a um preset que
/// a config nao reflete mais (#1434: evolucao de preset nao concede
/// capacidade nova em silencio). [`Preset::Developer`] e [`Preset::FullPod`]
/// convergem na mesma `Alcance`; o rotulo usa `full` para as duas, porque a
/// leitura nao tem — e nao deveria fingir ter — como distinguir a intencao
/// original.
///
/// So-leitura: o espaco de valores inclui `full` e `custom`, que a acao
/// `preset` (CLI e API) recusa como nome de preset — quem consumir este
/// campo para reatribuir precisa passar por `level`/`write`, nao por aqui.
pub fn rotulo_efetivo(a: Alcance) -> &'static str {
    if a == Alcance::CHAT {
        "chat_only"
    } else if a == Alcance::LEITURA {
        "read"
    } else if a == Alcance::COMPLETO {
        "full"
    } else {
        "custom"
    }
}
