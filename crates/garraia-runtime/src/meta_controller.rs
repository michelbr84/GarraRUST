use serde::{Deserialize, Serialize};

/// Meta-controller — controla budget de ferramentas e previne loops.
/// É o "cérebro operacional" que garante que o agente não entre em loop infinito.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetaController {
    /// Número de chamadas de ferramenta neste turno.
    pub tool_calls_this_turn: u32,

    /// Nome da última ferramenta chamada.
    pub last_tool_name: Option<String>,

    /// Contador de vezes que a mesma ferramenta foi chamada em sequência.
    pub repeated_same_tool: u32,

    /// Histórico de nomes de ferramentas chamadas neste turno.
    pub history: Vec<String>,
}

impl MetaController {
    /// Cria um novo meta-controller zerado.
    pub fn new() -> Self {
        Self {
            tool_calls_this_turn: 0,
            last_tool_name: None,
            repeated_same_tool: 0,
            history: Vec::new(),
        }
    }

    /// Reseta o estado para um novo turno.
    pub fn reset_turn(&mut self) {
        self.tool_calls_this_turn = 0;
        self.last_tool_name = None;
        self.repeated_same_tool = 0;
        self.history.clear();
    }

    /// Verifica se ainda pode chamar ferramentas neste turno.
    pub fn can_call_tool(&self, max_per_turn: u32) -> bool {
        self.tool_calls_this_turn < max_per_turn
    }

    /// Registra uma chamada de ferramenta.
    pub fn record_tool_call(&mut self, tool_name: &str) {
        self.tool_calls_this_turn += 1;
        self.history.push(tool_name.to_string());

        match &self.last_tool_name {
            Some(last) if last == tool_name => {
                self.repeated_same_tool += 1;
            }
            _ => {
                self.repeated_same_tool = 0;
            }
        }

        self.last_tool_name = Some(tool_name.to_string());
    }

    /// Detecta se o agente está em loop (mesma ferramenta 3+ vezes seguidas).
    pub fn detect_loop(&self) -> bool {
        self.repeated_same_tool >= 2
    }

    /// Retorna quantas chamadas foram feitas neste turno.
    pub fn calls_made(&self) -> u32 {
        self.tool_calls_this_turn
    }
}

impl Default for MetaController {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn novo_controller_zerado() {
        let mc = MetaController::new();
        assert_eq!(mc.tool_calls_this_turn, 0);
        assert!(mc.last_tool_name.is_none());
        assert_eq!(mc.repeated_same_tool, 0);
        assert!(mc.history.is_empty());
    }

    #[test]
    fn budget_bloqueia_apos_n_chamadas() {
        let mut mc = MetaController::new();
        let max = 3;

        mc.record_tool_call("bash");
        mc.record_tool_call("file_read");
        mc.record_tool_call("web_fetch");

        assert!(!mc.can_call_tool(max)); // 3/3 = bloqueado
        assert_eq!(mc.calls_made(), 3);
    }

    #[test]
    fn budget_permite_dentro_do_limite() {
        let mut mc = MetaController::new();
        mc.record_tool_call("bash");
        assert!(mc.can_call_tool(4)); // 1/4 = ok
    }

    #[test]
    fn detecta_loop_mesma_ferramenta() {
        let mut mc = MetaController::new();

        mc.record_tool_call("bash");
        assert!(!mc.detect_loop()); // 1x

        mc.record_tool_call("bash");
        assert!(!mc.detect_loop()); // 2x (repeated=1)

        mc.record_tool_call("bash");
        assert!(mc.detect_loop()); // 3x (repeated=2) → LOOP!
    }

    #[test]
    fn nao_detecta_loop_ferramentas_diferentes() {
        let mut mc = MetaController::new();

        mc.record_tool_call("bash");
        mc.record_tool_call("file_read");
        mc.record_tool_call("web_fetch");

        assert!(!mc.detect_loop());
    }

    #[test]
    fn reset_limpa_tudo() {
        let mut mc = MetaController::new();
        mc.record_tool_call("bash");
        mc.record_tool_call("bash");
        mc.record_tool_call("bash");

        mc.reset_turn();

        assert_eq!(mc.tool_calls_this_turn, 0);
        assert!(mc.last_tool_name.is_none());
        assert_eq!(mc.repeated_same_tool, 0);
        assert!(mc.history.is_empty());
        assert!(mc.can_call_tool(4));
    }

    #[test]
    fn historico_registra_todas_chamadas() {
        let mut mc = MetaController::new();
        mc.record_tool_call("bash");
        mc.record_tool_call("file_read");
        mc.record_tool_call("bash");

        assert_eq!(mc.history, vec!["bash", "file_read", "bash"]);
    }

    #[test]
    fn loop_reseta_ao_trocar_ferramenta() {
        let mut mc = MetaController::new();

        mc.record_tool_call("bash");
        mc.record_tool_call("bash"); // repeated=1
        mc.record_tool_call("file_read"); // troca → repeated=0

        assert!(!mc.detect_loop());
    }
}
