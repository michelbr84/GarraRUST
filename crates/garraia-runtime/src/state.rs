use std::fmt;

/// Estados explícitos da máquina de estados do agente.
/// Cada turno de execução transita entre estes estados.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TaskState {
    /// Preparando contexto, carregando memória e histórico.
    Planning,

    /// Chamando o modelo de linguagem (LLM) para decidir ação.
    Executing,

    /// Executando uma ferramenta (tool call).
    ToolUse,

    /// Aguardando evento externo ou confirmação do usuário.
    Waiting,

    /// Turno concluído com sucesso — resposta gerada.
    Completed,

    /// Turno falhou — erro irrecuperável neste ciclo.
    Failed,
}

impl TaskState {
    /// Verifica se o estado é terminal (não precisa mais transitar).
    pub fn is_terminal(&self) -> bool {
        matches!(self, TaskState::Completed | TaskState::Failed | TaskState::Waiting)
    }

    /// Retorna as transições válidas a partir deste estado.
    pub fn valid_transitions(&self) -> &[TaskState] {
        match self {
            TaskState::Planning => &[TaskState::Executing, TaskState::Failed],
            TaskState::Executing => &[TaskState::ToolUse, TaskState::Completed, TaskState::Failed],
            TaskState::ToolUse => &[TaskState::Executing, TaskState::Waiting, TaskState::Failed],
            TaskState::Waiting => &[TaskState::Executing, TaskState::Failed],
            TaskState::Completed => &[],
            TaskState::Failed => &[],
        }
    }

    /// Verifica se uma transição é válida.
    pub fn can_transition_to(&self, target: TaskState) -> bool {
        self.valid_transitions().contains(&target)
    }
}

impl fmt::Display for TaskState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TaskState::Planning => write!(f, "Planejando"),
            TaskState::Executing => write!(f, "Executando"),
            TaskState::ToolUse => write!(f, "Usando Ferramenta"),
            TaskState::Waiting => write!(f, "Aguardando"),
            TaskState::Completed => write!(f, "Concluído"),
            TaskState::Failed => write!(f, "Falhou"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn estados_terminais() {
        assert!(TaskState::Completed.is_terminal());
        assert!(TaskState::Failed.is_terminal());
        assert!(TaskState::Waiting.is_terminal());
        assert!(!TaskState::Planning.is_terminal());
        assert!(!TaskState::Executing.is_terminal());
        assert!(!TaskState::ToolUse.is_terminal());
    }

    #[test]
    fn transicoes_validas() {
        assert!(TaskState::Planning.can_transition_to(TaskState::Executing));
        assert!(TaskState::Planning.can_transition_to(TaskState::Failed));
        assert!(!TaskState::Planning.can_transition_to(TaskState::Completed));

        assert!(TaskState::Executing.can_transition_to(TaskState::ToolUse));
        assert!(TaskState::Executing.can_transition_to(TaskState::Completed));

        assert!(TaskState::ToolUse.can_transition_to(TaskState::Executing));
        assert!(TaskState::ToolUse.can_transition_to(TaskState::Waiting));
    }

    #[test]
    fn estados_terminais_sem_transicao() {
        assert!(TaskState::Completed.valid_transitions().is_empty());
        assert!(TaskState::Failed.valid_transitions().is_empty());
    }

    #[test]
    fn display_em_portugues() {
        assert_eq!(format!("{}", TaskState::Planning), "Planejando");
        assert_eq!(format!("{}", TaskState::Completed), "Concluído");
        assert_eq!(format!("{}", TaskState::Failed), "Falhou");
    }
}
