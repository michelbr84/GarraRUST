use std::collections::VecDeque;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

use serde_json::Value;

/// Tamanho da janela para detecção de loop.
/// Só dispara se as últimas N chamadas tiverem a MESMA assinatura (ferramenta + argumentos).
const JANELA_LOOP: usize = 3;

/// Assinatura de uma chamada de ferramenta: nome + hash dos argumentos.
/// Duas chamadas são consideradas "iguais" apenas se nome E argumentos forem idênticos.
#[derive(Debug, Clone, PartialEq, Eq)]
struct AssinaturaFerramenta {
    nome: String,
    hash_args: u64,
}

/// Calcula um hash determinístico dos argumentos da ferramenta (payload JSON).
fn calcular_hash_args(payload: &Value) -> u64 {
    let mut hasher = DefaultHasher::new();
    payload.to_string().hash(&mut hasher);
    hasher.finish()
}

/// Orçamento de execução para controlar chamadas de ferramentas no runtime do agente.
/// Evita loops infinitos, mas permite tarefas legítimas de longa duração.
///
/// A detecção de loop utiliza abordagem baseada em **assinatura**:
/// - Uma "assinatura" = nome da ferramenta + hash dos argumentos
/// - Só bloqueia quando as últimas `JANELA_LOOP` chamadas têm a MESMA assinatura
/// - Argumentos diferentes para a mesma ferramenta (ex: `bash("ls")` e `bash("cat file")`)
///   NÃO são considerados loop
pub struct ExecutionBudget {
    /// Máximo de chamadas de ferramenta por turno (um turno da conversa)
    max_per_turn: usize,
    /// Máximo de chamadas de ferramenta por tarefa (execução completa)
    max_per_task: usize,
    /// Timeout de cada execução de ferramenta em segundos
    tool_timeout_secs: u64,
    /// Quantidade atual de chamadas neste turno
    current_turn_calls: usize,
    /// Quantidade atual de chamadas nesta tarefa
    current_task_calls: usize,
    /// Janela deslizante com assinaturas recentes para detecção de loop
    historico_assinaturas: VecDeque<AssinaturaFerramenta>,
}

impl ExecutionBudget {
    /// Cria um orçamento com valores padrão:
    /// - 10 chamadas por turno
    /// - 30 chamadas por tarefa
    /// - 30 segundos de timeout por ferramenta
    pub fn padrao() -> Self {
        Self {
            max_per_turn: 10,
            max_per_task: 30,
            tool_timeout_secs: 30,
            current_turn_calls: 0,
            current_task_calls: 0,
            historico_assinaturas: VecDeque::with_capacity(JANELA_LOOP),
        }
    }

    /// Verifica se o limite por turno foi atingido (mas não o limite total da tarefa).
    /// Usado para estratégia de auto-reset entre turnos.
    pub fn atingiu_limite_turno(&self) -> bool {
        self.current_turn_calls >= self.max_per_turn && self.current_task_calls < self.max_per_task
    }

    /// Verifica se ainda é permitido chamar outra ferramenta.
    pub fn pode_chamar_ferramenta(&self) -> bool {
        self.current_turn_calls < self.max_per_turn && self.current_task_calls < self.max_per_task
    }

    /// Registra uma chamada de ferramenta com seu payload,
    /// para controle de orçamento e detecção de loop por assinatura.
    pub fn registrar_chamada(&mut self, tool_name: &str, payload: &Value) {
        self.current_turn_calls += 1;
        self.current_task_calls += 1;

        let assinatura = AssinaturaFerramenta {
            nome: tool_name.to_string(),
            hash_args: calcular_hash_args(payload),
        };

        if self.historico_assinaturas.len() == JANELA_LOOP {
            self.historico_assinaturas.pop_front();
        }

        self.historico_assinaturas.push_back(assinatura);
    }

    /// Detecta se uma ferramenta está sendo chamada em loop.
    ///
    /// Retorna `true` apenas quando as últimas `JANELA_LOOP` chamadas
    /// possuem exatamente a MESMA assinatura (mesmo nome E mesmos argumentos).
    ///
    /// Exemplos:
    /// - bash("ls"), bash("cat f"), bash("pwd")  → false (argumentos diferentes)
    /// - bash("cargo check") x3                  → true  (loop real)
    /// - bash("ls"), file_read("x"), bash("ls")  → false (ferramentas diferentes no meio)
    pub fn detectar_loop_ferramenta(&self) -> bool {
        if self.historico_assinaturas.len() < JANELA_LOOP {
            return false;
        }

        let primeira = &self.historico_assinaturas[0];

        self.historico_assinaturas
            .iter()
            .all(|sig| sig.nome == primeira.nome && sig.hash_args == primeira.hash_args)
    }

    /// Retorna a duração de timeout configurada para execução de ferramentas.
    pub fn timeout(&self) -> std::time::Duration {
        std::time::Duration::from_secs(self.tool_timeout_secs)
    }

    /// Reseta o orçamento para um novo turno (após resposta do assistente).
    pub fn resetar_turno(&mut self) {
        self.current_turn_calls = 0;
        self.historico_assinaturas.clear();
    }

    /// Reseta completamente o orçamento para uma nova tarefa (nova mensagem do usuário).
    pub fn resetar_tarefa(&mut self) {
        self.current_turn_calls = 0;
        self.current_task_calls = 0;
        self.historico_assinaturas.clear();
    }

    /// Retorna o status atual do orçamento em formato textual.
    pub fn status(&self) -> String {
        format!(
            "turn={}/{} task={}/{}",
            self.current_turn_calls, self.max_per_turn, self.current_task_calls, self.max_per_task
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_padrao_creation() {
        let budget = ExecutionBudget::padrao();
        assert!(budget.pode_chamar_ferramenta());
        assert_eq!(budget.max_per_turn, 10);
        assert_eq!(budget.max_per_task, 30);
    }

    #[test]
    fn test_registrar_chamada() {
        let mut budget = ExecutionBudget::padrao();
        budget.registrar_chamada("bash", &json!({"command": "ls"}));
        assert_eq!(budget.current_turn_calls, 1);
        assert_eq!(budget.current_task_calls, 1);
    }

    #[test]
    fn test_no_loop_different_args() {
        let mut budget = ExecutionBudget::padrao();

        // 3 chamadas bash com argumentos DIFERENTES — não deve ser loop
        budget.registrar_chamada("bash", &json!({"command": "ls"}));
        budget.registrar_chamada("bash", &json!({"command": "cat file.txt"}));
        budget.registrar_chamada("bash", &json!({"command": "pwd"}));

        assert!(!budget.detectar_loop_ferramenta());
    }

    #[test]
    fn test_loop_same_args() {
        let mut budget = ExecutionBudget::padrao();

        // 3 chamadas bash com argumentos IDÊNTICOS — é loop
        budget.registrar_chamada("bash", &json!({"command": "cargo check"}));
        budget.registrar_chamada("bash", &json!({"command": "cargo check"}));
        budget.registrar_chamada("bash", &json!({"command": "cargo check"}));

        assert!(budget.detectar_loop_ferramenta());
    }

    #[test]
    fn test_no_loop_under_window() {
        let mut budget = ExecutionBudget::padrao();

        // Apenas 2 chamadas idênticas — abaixo do limite da janela
        budget.registrar_chamada("bash", &json!({"command": "cargo check"}));
        budget.registrar_chamada("bash", &json!({"command": "cargo check"}));

        assert!(!budget.detectar_loop_ferramenta());
    }

    #[test]
    fn test_no_loop_mixed_tools() {
        let mut budget = ExecutionBudget::padrao();

        // Ferramentas diferentes intercaladas — não é loop
        budget.registrar_chamada("bash", &json!({"command": "ls"}));
        budget.registrar_chamada("file_read", &json!({"path": "test.txt"}));
        budget.registrar_chamada("bash", &json!({"command": "ls"}));

        assert!(!budget.detectar_loop_ferramenta());
    }

    #[test]
    fn test_loop_breaks_after_different_call() {
        let mut budget = ExecutionBudget::padrao();

        // Começa repetindo...
        budget.registrar_chamada("bash", &json!({"command": "cargo check"}));
        budget.registrar_chamada("bash", &json!({"command": "cargo check"}));

        // Uma chamada diferente quebra o padrão
        budget.registrar_chamada("bash", &json!({"command": "cat Cargo.toml"}));

        assert!(!budget.detectar_loop_ferramenta());
    }

    #[test]
    fn test_reset_turno() {
        let mut budget = ExecutionBudget::padrao();
        budget.registrar_chamada("bash", &json!({"command": "ls"}));
        budget.registrar_chamada("bash", &json!({"command": "ls"}));

        budget.resetar_turno();

        assert_eq!(budget.current_turn_calls, 0);
        assert_eq!(budget.current_task_calls, 2); // Chamadas da tarefa NÃO são resetadas
        assert!(budget.historico_assinaturas.is_empty());
    }

    #[test]
    fn test_reset_tarefa() {
        let mut budget = ExecutionBudget::padrao();
        budget.registrar_chamada("bash", &json!({"command": "ls"}));

        budget.resetar_tarefa();

        assert_eq!(budget.current_turn_calls, 0);
        assert_eq!(budget.current_task_calls, 0);
        assert!(budget.historico_assinaturas.is_empty());
    }

    #[test]
    fn test_exceeds_max_per_turn() {
        let mut budget = ExecutionBudget::padrao();

        // Padrão é 10 por turno
        for i in 0..10 {
            budget.registrar_chamada("bash", &json!({"command": format!("cmd_{}", i)}));
        }

        assert!(!budget.pode_chamar_ferramenta());
    }

    #[test]
    fn test_status_display() {
        let mut budget = ExecutionBudget::padrao();
        budget.registrar_chamada("bash", &json!({"command": "ls"}));

        let status = budget.status();
        assert_eq!(status, "turn=1/10 task=1/30");
    }

    #[test]
    fn test_sliding_window_eviction() {
        let mut budget = ExecutionBudget::padrao();

        // Preenche a janela com chamadas idênticas
        budget.registrar_chamada("bash", &json!({"command": "cargo check"}));
        budget.registrar_chamada("bash", &json!({"command": "cargo check"}));

        // Terceira chamada diferente — remove a mais antiga, janela fica mista
        budget.registrar_chamada("bash", &json!({"command": "ls"}));

        assert!(!budget.detectar_loop_ferramenta());

        // Agora preenche novamente com o novo comando
        budget.registrar_chamada("bash", &json!({"command": "ls"}));
        budget.registrar_chamada("bash", &json!({"command": "ls"}));

        // Janela agora é [ls, ls, ls] — loop detectado
        assert!(budget.detectar_loop_ferramenta());
    }
}
