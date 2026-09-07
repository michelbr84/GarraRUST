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
// Sem `Eq`: o perfil customizado carrega `ModeLlmConfig`, que tem `f64`
// (temperatura, top_p). `PartialEq` basta para os testes compararem contextos.
#[derive(Debug, Clone, Default, PartialEq)]
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

    /// O perfil ja resolvido de um **modo customizado** (#986).
    ///
    /// Modo customizado mora no banco (`custom_modes`), e nem todo chamador do
    /// runtime tem banco — a CLI monta o proprio `AgentRuntime`. Entao quem tem
    /// o banco resolve e passa o perfil pronto; quem nao tem passa `None` e o
    /// nome em [`Self::agent_mode`] resolve pelos modos nativos, como sempre.
    ///
    /// Quando presente, **vence** o nome: um modo customizado ja carrega o
    /// perfil do seu `base_mode` com os overrides aplicados, entao reinterpretar
    /// o nome desfaria justamente o que o usuario customizou.
    pub custom_profile: Option<crate::modes::ModeProfile>,

    /// O objetivo declarado da sessao (#983).
    ///
    /// O criterio de aceite pede que "o runtime receba o objetivo
    /// **explicitamente**, sem depender apenas de texto concatenado no prompt".
    /// Por isso e um campo, e nao uma string que alguem grudou na mensagem: o
    /// runtime decide onde ele entra, e da para testar que entrou.
    pub goal: Option<String>,

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
            ..Self::default()
        }
    }

    /// Contexto com um perfil de modo customizado ja resolvido (#986).
    ///
    /// `nome` e guardado junto porque a mensagem de recusa e o `/stats` querem
    /// o nome que o usuario deu, e nao o do modo base.
    pub fn with_custom_profile(nome: String, profile: crate::modes::ModeProfile) -> Self {
        Self {
            agent_mode: Some(nome),
            custom_profile: Some(profile),
            ..Self::default()
        }
    }

    /// Só o diretório — o caso do CLI antes de ler modo.
    pub fn with_working_dir(working_dir: Option<String>) -> Self {
        Self {
            working_dir,
            ..Self::default()
        }
    }
}
