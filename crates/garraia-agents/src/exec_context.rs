//! O que o chamador sabe sobre a execução e o runtime precisa saber (#982, #980).
//!
//! # Por que um struct, e não mais dois `Option<&str>`
//!
//! O `process_message_with_agent_config` já tinha dez parâmetros e dois
//! `#[allow(clippy::too_many_arguments)]`. Acrescentar modo e diretório faria
//! doze, e a #986 (modos customizados) traria mais um. Um struct com `Default`
//! para o caso vazio para essa progressão: os treze chamadores passam
//! `&ExecContext::default()` quando não têm nada a dizer, e quem tem algo
//! preenche só o campo que conhece.

/// Contexto de execução de um turno.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ExecContext {
    /// O modo que o usuário escolheu para esta sessão, se escolheu.
    ///
    /// **`None` não é "modo Ask"** — é "não escolheu", e a diferença é o que
    /// impede uma regressão grande. O default do [`crate::modes::AgentMode`] é
    /// `Ask`, e o perfil de `ask` nega `file_write`. Se sessão sem modo
    /// resolvesse para `Ask`, ligar a política do #988 quebraria `file_write`
    /// por padrão em **todo** canal — inclusive o CLI, que nunca seta modo.
    ///
    /// O critério de aceite da #988 pede que "o comportamento padrão não seja
    /// regredido", e o comportamento padrão de hoje é: sem política. Então
    /// `None` continua significando sem política.
    /// # Deduzir nao e consentir
    ///
    /// Este campo carrega o modo que o **usuario escolheu** — nunca o que
    /// alguem deduziu por ele. O gateway tem um auto-router (GAR-227) que
    /// classifica a mensagem e grava o modo na sessao; encher este campo com
    /// aquele valor faria a politica de ferramenta valer sem escolha, e quem
    /// nunca digitou `/mode` perderia `file_write` porque a heuristica achou
    /// que a pergunta parecia busca. A auditoria do #988 pegou exatamente
    /// isso.
    ///
    /// Quem preenche daqui do gateway usa
    /// `AppState::chosen_agent_mode_for`, que le a escolha e ignora a
    /// deducao.
    pub agent_mode: Option<String>,

    /// Diretório contra o qual caminho relativo de ferramenta é resolvido.
    ///
    /// **Isto não é um sandbox**, e a #980 promete que é. Ver
    /// [`crate::tools::tool_context`]: o `resolve_tool_path` rejeita `..` e
    /// junta relativo com este diretório, mas não canonicaliza nem confina —
    /// caminho absoluto passa igual, antes e depois. E o `bash_tool` ignora
    /// este campo por completo.
    pub working_dir: Option<String>,
}

impl ExecContext {
    /// Só o modo — o caso dos canais, que sabem a sessão mas não um diretório.
    pub fn with_mode(agent_mode: Option<String>) -> Self {
        Self {
            agent_mode,
            working_dir: None,
        }
    }

    /// Só o diretório — o caso do CLI antes de ler modo.
    pub fn with_working_dir(working_dir: Option<String>) -> Self {
        Self {
            agent_mode: None,
            working_dir,
        }
    }
}
