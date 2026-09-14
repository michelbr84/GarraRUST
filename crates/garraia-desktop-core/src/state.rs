//! Estado ligado/desligado dos módulos do desktop.
//!
//! # Forma
//!
//! O estado é **puro e determinístico**, no mesmo espírito do `spinner.rs` da
//! CLI: nada aqui lê relógio, dorme, abre socket ou toca disco. Um módulo só
//! muda de estado quando alguém chama [`ModuleState::request`] (intenção do
//! usuário) ou [`ModuleState::observe`] (fato vindo do mundo). Quem observa o
//! mundo é [`crate::supervise`]; quem decide o ritmo é a casca.
//!
//! Isso é o que torna o ciclo de vida testável sem processo real e sem `sleep`.
//!
//! # Duas verdades separadas
//!
//! [`Desired`] é o que o usuário pediu. [`Power`] é o que de fato está
//! acontecendo. Separá-las é o que permite a UI mostrar "ligando…" sem mentir
//! que já ligou, e é o que permite reconciliar depois de uma queda: o módulo
//! que caiu sozinho continua com `Desired::On` e aparece como
//! [`Power::Failed`], não como "desligado pelo usuário".

use serde::{Deserialize, Serialize};

/// Módulos do control center que podem ser ligados e desligados.
///
/// A lista é fechada de propósito: são os módulos que o desktop de hoje já
/// tem, mais o gateway que ele já roda como sidecar. Aba nova não precisa de
/// variante nova — só módulo com ciclo de vida próprio precisa.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ModuleId {
    /// Overlay do papagaio (`Alt+G`).
    Bird,
    /// Chat Bar (`Ctrl+Space`).
    ChatBar,
    /// Gateway `garraia start`, rodado como sidecar.
    Gateway,
    /// Ícone e menu de bandeja.
    Tray,
}

impl ModuleId {
    /// Todos os módulos, em ordem estável — a mesma que a UI usa para listar.
    ///
    /// A ordem é também a indexação interna de [`Modules`], então acrescentar
    /// variante exige acrescentar aqui: o `match` de [`ModuleId::index`] não
    /// compila sem isso.
    pub const ALL: [ModuleId; 4] = [
        ModuleId::Bird,
        ModuleId::ChatBar,
        ModuleId::Gateway,
        ModuleId::Tray,
    ];

    /// Posição do módulo em [`ModuleId::ALL`].
    ///
    /// É o que torna [`Modules::get`] uma função **total**: indexar um array
    /// de tamanho fixo por essa posição não tem caminho de `None` para
    /// destratar, e portanto não tem `unwrap` nem panic em código de produção.
    const fn index(self) -> usize {
        match self {
            ModuleId::Bird => 0,
            ModuleId::ChatBar => 1,
            ModuleId::Gateway => 2,
            ModuleId::Tray => 3,
        }
    }

    /// Identificador estável para config, log e JSON.
    pub fn as_str(self) -> &'static str {
        match self {
            ModuleId::Bird => "bird",
            ModuleId::ChatBar => "chat_bar",
            ModuleId::Gateway => "gateway",
            ModuleId::Tray => "tray",
        }
    }
}

/// O que o usuário pediu.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Desired {
    On,
    Off,
}

/// O que de fato está acontecendo.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Power {
    Off,
    Starting,
    On,
    Stopping,
    /// Tentou ligar e não conseguiu, ou morreu sem ninguém mandar.
    Failed,
}

impl Power {
    /// `true` quando há trabalho em curso — a UI mostra spinner, não toggle.
    pub fn is_transitional(self) -> bool {
        matches!(self, Power::Starting | Power::Stopping)
    }
}

/// Fato vindo do mundo. Nunca é intenção: é relato.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PowerEvent {
    /// Subiu e está de pé.
    Started,
    /// Parou porque mandaram parar.
    Stopped,
    /// Caiu sozinho, ou não conseguiu subir.
    Crashed,
}

/// Estado de um módulo: intenção, realidade e quantas vezes falhou seguido.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModuleState {
    id: ModuleId,
    desired: Desired,
    power: Power,
    /// Falhas consecutivas desde o último `Started`. É o contador que a
    /// política de restart de [`crate::supervise`] consulta para desistir em
    /// vez de entrar em laço de restart.
    failures: u32,
}

impl ModuleState {
    /// Nasce desligado e sem ninguém tendo pedido nada.
    pub fn new(id: ModuleId) -> Self {
        Self {
            id,
            desired: Desired::Off,
            power: Power::Off,
            failures: 0,
        }
    }

    pub fn id(&self) -> ModuleId {
        self.id
    }

    pub fn desired(&self) -> Desired {
        self.desired
    }

    pub fn power(&self) -> Power {
        self.power
    }

    pub fn failures(&self) -> u32 {
        self.failures
    }

    /// `true` quando realidade e intenção batem e nada está em trânsito.
    pub fn is_settled(&self) -> bool {
        match self.desired {
            Desired::On => self.power == Power::On,
            Desired::Off => self.power == Power::Off,
        }
    }

    /// Registra a intenção do usuário e move o estado para o trânsito
    /// correspondente.
    ///
    /// É **idempotente**: pedir `On` de um módulo já ligado não o reinicia e
    /// não zera contador. Pedir `On` de um módulo em [`Power::Failed`] conta
    /// como nova tentativa e volta para [`Power::Starting`] — é o botão
    /// "tentar de novo" da UI.
    ///
    /// Devolve `true` quando algo mudou, para a casca saber se precisa agir.
    pub fn request(&mut self, desired: Desired) -> bool {
        let before = (self.desired, self.power);
        self.desired = desired;
        self.power = match (desired, self.power) {
            // Já está onde deveria: nada a fazer.
            (Desired::On, Power::On) => Power::On,
            (Desired::Off, Power::Off) => Power::Off,
            // Já está indo para lá: não reinicia a transição.
            (Desired::On, Power::Starting) => Power::Starting,
            (Desired::Off, Power::Stopping) => Power::Stopping,
            // Desligar o que nunca subiu é desligar, não é parar.
            (Desired::Off, Power::Failed) => Power::Off,
            (Desired::On, _) => Power::Starting,
            (Desired::Off, _) => Power::Stopping,
        };
        before != (self.desired, self.power)
    }

    /// Aplica um fato observado no mundo.
    ///
    /// Um `Crashed` **não** apaga a intenção: o módulo continua com
    /// `Desired::On` e vai para [`Power::Failed`]. É assim que a UI distingue
    /// "o usuário desligou" de "caiu", e é o que dá à política de restart o
    /// direito de tentar de novo sem inventar intenção que o usuário não teve.
    pub fn observe(&mut self, event: PowerEvent) {
        match event {
            PowerEvent::Started => {
                self.power = Power::On;
                self.failures = 0;
            }
            PowerEvent::Stopped => {
                self.power = Power::Off;
            }
            PowerEvent::Crashed => {
                self.failures = self.failures.saturating_add(1);
                self.power = match self.desired {
                    // Caiu enquanto era para estar de pé: isso é falha.
                    Desired::On => Power::Failed,
                    // Caiu enquanto estava sendo desligado: era o esperado.
                    Desired::Off => Power::Off,
                };
            }
        }
    }
}

/// Conjunto de módulos do control center.
///
/// Array de tamanho fixo indexado por [`ModuleId::index`], e não mapa: a
/// ordem da listagem é contrato de UI e de snapshot de teste, e a totalidade
/// da indexação é o que deixa [`Modules::get`] sem `Option` para destratar —
/// ou seja, sem `unwrap` em código de produção.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Modules {
    modules: [ModuleState; ModuleId::ALL.len()],
}

impl Default for Modules {
    fn default() -> Self {
        Self::new()
    }
}

impl Modules {
    /// Todos os módulos conhecidos, todos desligados.
    pub fn new() -> Self {
        Self {
            modules: ModuleId::ALL.map(ModuleState::new),
        }
    }

    pub fn get(&self, id: ModuleId) -> &ModuleState {
        &self.modules[id.index()]
    }

    pub fn get_mut(&mut self, id: ModuleId) -> &mut ModuleState {
        &mut self.modules[id.index()]
    }

    /// Atalho para [`ModuleState::request`].
    pub fn request(&mut self, id: ModuleId, desired: Desired) -> bool {
        self.get_mut(id).request(desired)
    }

    /// Atalho para [`ModuleState::observe`].
    pub fn observe(&mut self, id: ModuleId, event: PowerEvent) {
        self.get_mut(id).observe(event);
    }

    /// Itera em ordem estável.
    pub fn iter(&self) -> impl Iterator<Item = &ModuleState> {
        self.modules.iter()
    }

    /// Módulos cuja realidade ainda não alcançou a intenção — a lista de
    /// trabalho de quem reconcilia.
    pub fn pending(&self) -> impl Iterator<Item = &ModuleState> {
        self.modules.iter().filter(|m| !m.is_settled())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nasce_desligado_e_sem_intencao() {
        let m = ModuleState::new(ModuleId::Bird);
        assert_eq!(m.power(), Power::Off);
        assert_eq!(m.desired(), Desired::Off);
        assert_eq!(m.failures(), 0);
        assert!(m.is_settled());
    }

    #[test]
    fn ligar_passa_por_starting_antes_de_on() {
        let mut m = ModuleState::new(ModuleId::Gateway);
        assert!(m.request(Desired::On));
        assert_eq!(m.power(), Power::Starting);
        assert!(m.power().is_transitional());
        assert!(!m.is_settled());

        m.observe(PowerEvent::Started);
        assert_eq!(m.power(), Power::On);
        assert!(m.is_settled());
    }

    #[test]
    fn pedir_o_que_ja_esta_e_no_op() {
        let mut m = ModuleState::new(ModuleId::ChatBar);
        m.request(Desired::On);
        m.observe(PowerEvent::Started);

        // Segundo pedido não reinicia nada.
        assert!(!m.request(Desired::On));
        assert_eq!(m.power(), Power::On);

        // Nem o terceiro, nem enquanto está subindo.
        let mut subindo = ModuleState::new(ModuleId::ChatBar);
        subindo.request(Desired::On);
        assert!(!subindo.request(Desired::On));
        assert_eq!(subindo.power(), Power::Starting);
    }

    #[test]
    fn desligar_o_que_nunca_subiu_e_no_op() {
        let mut m = ModuleState::new(ModuleId::Tray);
        assert!(!m.request(Desired::Off));
        assert_eq!(m.power(), Power::Off);
    }

    #[test]
    fn queda_nao_apaga_a_intencao_do_usuario() {
        let mut m = ModuleState::new(ModuleId::Gateway);
        m.request(Desired::On);
        m.observe(PowerEvent::Started);
        m.observe(PowerEvent::Crashed);

        // Caiu, mas o usuário nunca pediu para desligar.
        assert_eq!(m.power(), Power::Failed);
        assert_eq!(m.desired(), Desired::On);
        assert_eq!(m.failures(), 1);
        assert!(!m.is_settled());
    }

    #[test]
    fn morte_durante_o_desligamento_e_desligamento_normal() {
        let mut m = ModuleState::new(ModuleId::Gateway);
        m.request(Desired::On);
        m.observe(PowerEvent::Started);
        m.request(Desired::Off);
        assert_eq!(m.power(), Power::Stopping);

        // O processo morre porque mandaram matar — não é falha.
        m.observe(PowerEvent::Crashed);
        assert_eq!(m.power(), Power::Off);
        assert!(m.is_settled());
    }

    #[test]
    fn falhas_consecutivas_acumulam_e_started_zera() {
        let mut m = ModuleState::new(ModuleId::Gateway);
        m.request(Desired::On);
        m.observe(PowerEvent::Crashed);
        m.observe(PowerEvent::Crashed);
        assert_eq!(m.failures(), 2);

        m.observe(PowerEvent::Started);
        assert_eq!(m.failures(), 0);
        assert_eq!(m.power(), Power::On);
    }

    #[test]
    fn retry_de_um_modulo_falho_volta_para_starting() {
        let mut m = ModuleState::new(ModuleId::Gateway);
        m.request(Desired::On);
        m.observe(PowerEvent::Crashed);
        assert_eq!(m.power(), Power::Failed);

        // Botão "tentar de novo".
        assert!(m.request(Desired::On));
        assert_eq!(m.power(), Power::Starting);
        // Histórico de falha não é apagado por uma nova tentativa.
        assert_eq!(m.failures(), 1);
    }

    #[test]
    fn desligar_um_modulo_falho_o_desliga_sem_passar_por_stopping() {
        let mut m = ModuleState::new(ModuleId::Gateway);
        m.request(Desired::On);
        m.observe(PowerEvent::Crashed);

        assert!(m.request(Desired::Off));
        // Não há processo para parar: já está parado.
        assert_eq!(m.power(), Power::Off);
        assert!(m.is_settled());
    }

    #[test]
    fn conjunto_nasce_completo_e_em_ordem_estavel() {
        let mods = Modules::new();
        let ids: Vec<_> = mods.iter().map(|m| m.id()).collect();
        assert_eq!(ids, ModuleId::ALL.to_vec());
        assert!(mods.iter().all(|m| m.power() == Power::Off));
        assert_eq!(mods.pending().count(), 0);
    }

    #[test]
    fn pending_lista_so_quem_ainda_nao_chegou() {
        let mut mods = Modules::new();
        mods.request(ModuleId::Gateway, Desired::On);
        mods.request(ModuleId::Bird, Desired::On);
        assert_eq!(mods.pending().count(), 2);

        mods.observe(ModuleId::Gateway, PowerEvent::Started);
        let pendentes: Vec<_> = mods.pending().map(|m| m.id()).collect();
        assert_eq!(pendentes, vec![ModuleId::Bird]);
    }

    #[test]
    fn estado_serializa_com_nomes_estaveis() {
        let mut m = ModuleState::new(ModuleId::ChatBar);
        m.request(Desired::On);
        let json = serde_json::to_value(&m).expect("ModuleState e serializavel");
        assert_eq!(json["id"], "chat_bar");
        assert_eq!(json["desired"], "on");
        assert_eq!(json["power"], "starting");
    }

    #[test]
    fn as_str_bate_com_a_serializacao() {
        for id in ModuleId::ALL {
            let json = serde_json::to_value(id).expect("ModuleId e serializavel");
            assert_eq!(json, id.as_str());
        }
    }
}
