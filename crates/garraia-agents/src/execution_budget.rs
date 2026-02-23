use std::collections::VecDeque;
use std::hash::{Hash, Hasher};
use std::collections::hash_map::DefaultHasher;
use serde_json::Value;

/// Window size for loop detection.
/// Only triggers if the last N calls have the SAME signature (tool + args).
const JANELA_LOOP: usize = 3;

/// Signature of a tool call: tool name + hash of its arguments.
/// Two calls are considered "the same" only if both name AND args match.
#[derive(Debug, Clone, PartialEq, Eq)]
struct AssinaturaFerramenta {
    nome: String,
    hash_args: u64,
}

/// Compute a deterministic hash of the tool arguments (JSON payload).
fn calcular_hash_args(payload: &Value) -> u64 {
    let mut hasher = DefaultHasher::new();
    payload.to_string().hash(&mut hasher);
    hasher.finish()
}

/// Execution budget for controlling tool execution in agent runtime.
/// Prevents infinite loops while allowing legitimate long-running tasks.
///
/// Loop detection uses a **signature-based** approach:
/// - A "signature" = tool name + hash of arguments
/// - Only blocks when the last `JANELA_LOOP` calls have the **exact same** signature
/// - Different arguments to the same tool (e.g. `bash("ls")` then `bash("cat file")`)
///   are NOT considered a loop
pub struct ExecutionBudget {
    /// Max tool calls per turn (conversation turn)
    max_per_turn: usize,
    /// Max tool calls per task (entire execution)
    max_per_task: usize,
    /// Timeout for each tool execution in seconds
    tool_timeout_secs: u64,
    /// Current tool calls this turn
    current_turn_calls: usize,
    /// Current tool calls this task
    current_task_calls: usize,
    /// Sliding window of recent tool call signatures for loop detection
    historico_assinaturas: VecDeque<AssinaturaFerramenta>,
}

impl ExecutionBudget {
    /// Create a budget with default values:
    /// - 50 calls per turn (allows multi-turn agent loops)
    /// - 100 calls per task (allows long-running tasks)
    /// - 30 second timeout per tool
    pub fn padrao() -> Self {
        Self {
            max_per_turn: 50,
            max_per_task: 100,
            tool_timeout_secs: 30,
            current_turn_calls: 0,
            current_task_calls: 0,
            historico_assinaturas: VecDeque::with_capacity(JANELA_LOOP),
        }
    }

    /// Check if turn limit was reached (but task limit not)
    /// Used for auto-reset strategy
    pub fn atingiu_limite_turno(&self) -> bool {
        self.current_turn_calls >= self.max_per_turn
            && self.current_task_calls < self.max_per_task
    }

    /// Check if we can call another tool
    pub fn pode_chamar_ferramenta(&self) -> bool {
        self.current_turn_calls < self.max_per_turn
            && self.current_task_calls < self.max_per_task
    }

    /// Register a tool call with its payload for signature-based loop detection
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

    /// Detect if a tool is being called in a loop.
    ///
    /// Returns `true` only when the last `JANELA_LOOP` calls have the
    /// **exact same signature** (same tool name AND same arguments).
    ///
    /// Examples:
    /// - bash("ls"), bash("cat f"), bash("pwd")       → false ✅ (different args)
    /// - bash("cargo check") x3                       → true  ❌ (real loop)
    /// - bash("ls"), file_read("x"), bash("ls")       → false ✅ (different tools in between)
    pub fn detectar_loop_ferramenta(&self) -> bool {
        if self.historico_assinaturas.len() < JANELA_LOOP {
            return false;
        }

        let primeira = &self.historico_assinaturas[0];

        self.historico_assinaturas
            .iter()
            .all(|sig| sig.nome == primeira.nome && sig.hash_args == primeira.hash_args)
    }

    /// Get the tool timeout duration
    pub fn timeout(&self) -> std::time::Duration {
        std::time::Duration::from_secs(self.tool_timeout_secs)
    }

    /// Reset for a new turn (after assistant responds)
    pub fn resetar_turno(&mut self) {
        self.current_turn_calls = 0;
        self.historico_assinaturas.clear();
    }

    /// Full reset for a new task (new user message)
    pub fn resetar_tarefa(&mut self) {
        self.current_turn_calls = 0;
        self.current_task_calls = 0;
        self.historico_assinaturas.clear();
    }

    /// Get current status as string
    pub fn status(&self) -> String {
        format!(
            "turn={}/{} task={}/{}",
            self.current_turn_calls, self.max_per_turn,
            self.current_task_calls, self.max_per_task
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

        // 3 bash calls with DIFFERENT arguments — should NOT be a loop
        budget.registrar_chamada("bash", &json!({"command": "ls"}));
        budget.registrar_chamada("bash", &json!({"command": "cat file.txt"}));
        budget.registrar_chamada("bash", &json!({"command": "pwd"}));

        assert!(!budget.detectar_loop_ferramenta());
    }

    #[test]
    fn test_loop_same_args() {
        let mut budget = ExecutionBudget::padrao();

        // 3 bash calls with IDENTICAL arguments — IS a loop
        budget.registrar_chamada("bash", &json!({"command": "cargo check"}));
        budget.registrar_chamada("bash", &json!({"command": "cargo check"}));
        budget.registrar_chamada("bash", &json!({"command": "cargo check"}));

        assert!(budget.detectar_loop_ferramenta());
    }

    #[test]
    fn test_no_loop_under_window() {
        let mut budget = ExecutionBudget::padrao();

        // Only 2 identical calls — below JANELA_LOOP threshold
        budget.registrar_chamada("bash", &json!({"command": "cargo check"}));
        budget.registrar_chamada("bash", &json!({"command": "cargo check"}));

        assert!(!budget.detectar_loop_ferramenta());
    }

    #[test]
    fn test_no_loop_mixed_tools() {
        let mut budget = ExecutionBudget::padrao();

        // Different tools interleaved — not a loop
        budget.registrar_chamada("bash", &json!({"command": "ls"}));
        budget.registrar_chamada("file_read", &json!({"path": "test.txt"}));
        budget.registrar_chamada("bash", &json!({"command": "ls"}));

        assert!(!budget.detectar_loop_ferramenta());
    }

    #[test]
    fn test_loop_breaks_after_different_call() {
        let mut budget = ExecutionBudget::padrao();

        // Start repeating...
        budget.registrar_chamada("bash", &json!({"command": "cargo check"}));
        budget.registrar_chamada("bash", &json!({"command": "cargo check"}));

        // Then a different call breaks the pattern
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
        assert_eq!(budget.current_task_calls, 2); // Task calls should NOT reset
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

        // Default is 10 per turn
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

        // Fill window with identical calls
        budget.registrar_chamada("bash", &json!({"command": "cargo check"}));
        budget.registrar_chamada("bash", &json!({"command": "cargo check"}));

        // Third call is different — pushes out the oldest, window is now mixed
        budget.registrar_chamada("bash", &json!({"command": "ls"}));

        assert!(!budget.detectar_loop_ferramenta());

        // Now fill again with the new command
        budget.registrar_chamada("bash", &json!({"command": "ls"}));
        budget.registrar_chamada("bash", &json!({"command": "ls"}));

        // Window is now [ls, ls, ls] — loop detected
        assert!(budget.detectar_loop_ferramenta());
    }
}
