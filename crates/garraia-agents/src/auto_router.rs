//! GAR-227: Micro-LLM Auto Mode Router
//!
//! Automatically selects an [`AgentMode`] for an incoming message using a
//! two-stage pipeline:
//!
//! 1. **Heuristic** — Fast keyword-based classification. Returns a mode when
//!    the message clearly belongs to one category, or `None` when ambiguous.
//! 2. **LLM classify** — When the heuristic is ambiguous and
//!    `agent.auto_router_llm_enabled = true`, a single short LLM request
//!    (≤5 output tokens, 5 s timeout) classifies the intent.
//!
//! Falls back to `None` (no auto-mode override) if both stages fail or the
//! feature flag is disabled.

use std::time::Duration;

use crate::{AgentMode, AgentRuntime, ChatMessage, ChatRole, LlmRequest, MessagePart};
use tracing::{debug, warn};

// ── Public entry point ────────────────────────────────────────────────────────

/// Attempt to auto-classify the user's message into an [`AgentMode`].
///
/// Returns `None` if no mode could be determined (caller keeps the current mode
/// or uses the default). Never panics; all errors are logged and swallowed.
pub async fn auto_classify(
    text: &str,
    llm_enabled: bool,
    model_override: Option<&str>,
    runtime: Option<&AgentRuntime>,
) -> Option<AgentMode> {
    let heuristic = classify_heuristic(text);

    if heuristic.is_some() {
        debug!(mode = ?heuristic, "auto_router: heuristic match");
        return heuristic;
    }

    // Heuristic was ambiguous — try LLM if enabled and a runtime is available.
    if llm_enabled && let Some(rt) = runtime {
        let mode = classify_with_llm(text, rt, model_override).await;
        if mode.is_some() {
            debug!(mode = ?mode, "auto_router: LLM classify match");
            return mode;
        }
    }

    None
}

// ── Heuristic classifier ──────────────────────────────────────────────────────

/// Keyword-based mode classification.
///
/// Returns a clear mode when the message strongly signals a single intent, or
/// `None` when ambiguous (e.g., "how do I fix this code?").
///
/// # Portugues conta
///
/// As listas nasceram so em ingles, e este projeto e escrito e usado em
/// portugues. Enquanto o modo era decoracao de prompt, um classificador que
/// nunca dispara para "escreve uma funcao que soma" era so um recurso inerte;
/// depois que a `ToolPolicy` passou a valer (#988), `/mode auto` para um
/// usuario que escreve em portugues vira **nenhuma restricao**, em silencio —
/// o mesmo padrao de prometer e nao entregar que o #988 corrigiu.
///
/// O vocabulario pt-BR veio do `AutoRouter` que morava em `agent_mode.rs`,
/// removido neste mesmo trabalho. O que **nao** veio de la e o algoritmo: ele
/// era primeiro-que-casar-vence e devolvia sempre um modo, nunca `None`. Com a
/// politica valendo, classificar "oi" como `code` ou `search` seria pior que
/// nao classificar. O criterio de pontuacao com folga minima e o que permite
/// dizer "nao sei".
pub(crate) fn classify_heuristic(text: &str) -> Option<AgentMode> {
    let lower = text.to_lowercase();
    let t = lower.as_str();

    // Score each mode
    let mut scores: Vec<(AgentMode, u8)> = vec![
        (AgentMode::Code, score_code(t)),
        (AgentMode::Debug, score_debug(t)),
        (AgentMode::Review, score_review(t)),
        (AgentMode::Search, score_search(t)),
        (AgentMode::Architect, score_architect(t)),
        (AgentMode::Ask, score_ask(t)),
    ];

    scores.sort_by_key(|&(_, score)| std::cmp::Reverse(score));
    let (winner_mode, winner_score) = scores[0];
    let runner_score = scores[1].1;

    // Require a minimum score AND a clear lead to avoid ambiguous results.
    if winner_score >= 2 && winner_score >= runner_score + 2 {
        Some(winner_mode)
    } else {
        None
    }
}

fn score_code(t: &str) -> u8 {
    let keywords = [
        "implement",
        "write a function",
        "create a class",
        "fix the bug",
        "add feature",
        "refactor",
        "unit test",
        "write test",
        "add test",
        "compile error",
        "syntax error",
        "runtime error",
        "stack trace",
        "write code",
        "code for",
        "function that",
        "method that",
        // pt-BR
        "implementa",
        "implementar",
        "escreve uma",
        "escrever uma",
        "cria uma",
        "criar uma",
        "cria um",
        "criar um",
        "refatora",
        "refatorar",
        "adiciona um",
        "adicionar um",
        "corrige o bug",
        "corrigir o bug",
        "escreve um teste",
        "erro de compila",
        "erro de sintaxe",
        "gera o codigo",
        "gerar o codigo",
        "funcao que",
        "função que",
        "metodo que",
        "método que",
    ];
    keywords.iter().filter(|&&k| t.contains(k)).count().min(4) as u8
}

fn score_debug(t: &str) -> u8 {
    let keywords = [
        "why does",
        "why is",
        "not working",
        "broken",
        "crash",
        "panic",
        "exception",
        "traceback",
        "diagnose",
        "why my",
        "fails with",
        "error when",
        "bug in",
        "undefined",
        "null pointer",
        // pt-BR
        "por que o",
        "por que a",
        "por que nao",
        "por que não",
        "nao funciona",
        "não funciona",
        "quebrado",
        "travou",
        "panico",
        "pânico",
        "excecao",
        "exceção",
        "diagnostica",
        "diagnosticar",
        "falha com",
        "erro ao",
        "bug no",
        "bug na",
        "deu erro",
        "stack trace",
    ];
    keywords.iter().filter(|&&k| t.contains(k)).count().min(4) as u8
}

fn score_review(t: &str) -> u8 {
    let keywords = [
        "review",
        "check my",
        "look at this",
        "feedback on",
        "audit",
        "is this correct",
        "improve this",
        "what's wrong with",
        "critique",
        "assess",
        "evaluate",
        // pt-BR
        "revisa",
        "revisar",
        "revise",
        "verifica o",
        "verificar o",
        "analisa o",
        "analisar o",
        "audita",
        "auditar",
        "olha esse",
        "olha este",
        "da uma olhada",
        "dá uma olhada",
        "esta correto",
        "está correto",
        "melhora esse",
        "melhorar esse",
        "o que ha de errado",
        "o que há de errado",
        "critica",
        "avalia",
        "avaliar",
    ];
    keywords.iter().filter(|&&k| t.contains(k)).count().min(4) as u8
}

fn score_search(t: &str) -> u8 {
    let keywords = [
        "find ",
        "search for",
        "where is",
        "locate",
        "grep",
        "look for",
        "which file",
        "list all",
        "show me all",
        // pt-BR
        "encontra",
        "encontrar",
        "procura",
        "procurar",
        "onde esta",
        "onde está",
        "onde fica",
        "localiza",
        "localizar",
        "qual arquivo",
        "em que arquivo",
        "lista todos",
        "lista todas",
        "me mostra todos",
        "me mostra todas",
        "busca por",
        "buscar por",
    ];
    keywords.iter().filter(|&&k| t.contains(k)).count().min(4) as u8
}

fn score_architect(t: &str) -> u8 {
    let keywords = [
        "design",
        "architecture",
        "plan",
        "how should i structure",
        "best way to",
        "approach for",
        "system design",
        "data model",
        "schema for",
        "diagram",
        // pt-BR
        "desenha",
        "desenhar",
        "arquitetura",
        "planeja",
        "planejar",
        "como estruturar",
        "melhor forma de",
        "melhor maneira de",
        "abordagem para",
        "desenho do sistema",
        "modelo de dados",
        "esquema para",
        "diagrama",
    ];
    keywords.iter().filter(|&&k| t.contains(k)).count().min(4) as u8
}

fn score_ask(t: &str) -> u8 {
    let keywords = [
        "what is",
        "explain",
        "tell me",
        "how does",
        "what does",
        "describe",
        "difference between",
        "compare",
        "definition of",
        "what are",
        // pt-BR
        "o que e",
        "o que é",
        "o que sao",
        "o que são",
        "explica",
        "explicar",
        "me diz",
        "me diga",
        "como funciona",
        "por que existe",
        "descreve",
        "descrever",
        "define",
        "definir",
        "significa",
    ];
    keywords.iter().filter(|&&k| t.contains(k)).count().min(4) as u8
}

// ── LLM classifier ────────────────────────────────────────────────────────────

const ROUTER_SYSTEM: &str = "\
You are a routing classifier. Read the user message and output exactly ONE word \
from the following list that best describes the user's intent:\n\
code, debug, review, search, architect, ask\n\
Output only the single word. No punctuation, no explanation.";

const ROUTER_MAX_TOKENS: u32 = 5;
const ROUTER_TIMEOUT_SECS: u64 = 5;

/// Make a single short LLM call to classify the user's intent.
async fn classify_with_llm(
    text: &str,
    runtime: &AgentRuntime,
    model_override: Option<&str>,
) -> Option<AgentMode> {
    let provider = runtime.default_provider()?;

    // Corte por **caractere**, nao por byte.
    //
    // Era `&text[..text.len().min(400)]`, e `len()` conta bytes: numa mensagem
    // em portugues com 400+ bytes, o corte cai no meio de um `ç`/`ã`/`é` e o
    // slice entra em panico — derrubando o turno de quem escreveu acentuado,
    // que neste projeto e a maioria. `char_indices` acha o limite real.
    let snippet: &str = match text.char_indices().nth(400) {
        Some((limite, _)) => &text[..limite],
        None => text,
    };

    let model = model_override
        .map(|m| m.to_string())
        .unwrap_or_else(|| provider.provider_id().to_string());

    let request = LlmRequest {
        model,
        messages: vec![ChatMessage {
            role: ChatRole::User,
            content: MessagePart::Text(snippet.to_string()),
        }],
        system: Some(ROUTER_SYSTEM.to_string()),
        max_tokens: Some(ROUTER_MAX_TOKENS),
        temperature: Some(0.0),
        tools: vec![],
    };

    let result = tokio::time::timeout(
        Duration::from_secs(ROUTER_TIMEOUT_SECS),
        provider.complete(&request),
    )
    .await;

    match result {
        Ok(Ok(response)) => {
            let raw = response
                .content
                .iter()
                .filter_map(|b| {
                    if let crate::ContentBlock::Text { text } = b {
                        Some(text.trim().to_lowercase())
                    } else {
                        None
                    }
                })
                .next()
                .unwrap_or_default();

            // Extract the first word in case the model added extra text.
            let word = raw.split_whitespace().next().unwrap_or("");
            let mode = AgentMode::from_str(word);
            if mode.is_none() {
                warn!(raw = %raw, "auto_router: LLM returned unexpected mode token");
            }
            mode
        }
        Ok(Err(e)) => {
            warn!(error = %e, "auto_router: LLM classify call failed");
            None
        }
        Err(_) => {
            warn!("auto_router: LLM classify call timed out after {ROUTER_TIMEOUT_SECS}s");
            None
        }
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn heuristic_code() {
        // "implement" + "function that" = 2 → clear code win
        assert_eq!(
            classify_heuristic("implement a function that parses JSON and returns a struct"),
            Some(AgentMode::Code)
        );
        // "implement" + "unit test" = 2
        assert_eq!(
            classify_heuristic("implement the parser and add unit test coverage"),
            Some(AgentMode::Code)
        );
    }

    #[test]
    fn heuristic_debug() {
        // "why does" + "not working" = 2
        assert_eq!(
            classify_heuristic("why does this crash? the server is not working"),
            Some(AgentMode::Debug)
        );
        // "not working" + "exception" = 2
        assert_eq!(
            classify_heuristic("not working, fails with null pointer exception at runtime"),
            Some(AgentMode::Debug)
        );
    }

    #[test]
    fn heuristic_review() {
        // "review" + "feedback on" = 2
        assert_eq!(
            classify_heuristic("can you review this code and give feedback on it"),
            Some(AgentMode::Review)
        );
    }

    #[test]
    fn heuristic_ask() {
        // "what is" + "difference between" = 2
        assert_eq!(
            classify_heuristic("what is the difference between async and sync rust"),
            Some(AgentMode::Ask)
        );
    }

    #[test]
    fn heuristic_ambiguous_returns_none() {
        // Short or generic messages should not match strongly
        assert_eq!(classify_heuristic("hello"), None);
        assert_eq!(classify_heuristic("ok"), None);
    }
}

#[cfg(test)]
mod pt_br {
    use super::*;

    /// O classificador precisa acertar intencao escrita em portugues.
    ///
    /// Antes deste trabalho as listas eram so em ingles, entao `/mode auto`
    /// nunca disparava para quem escreve em portugues — e, com a `ToolPolicy`
    /// valendo (#988), "nunca dispara" quer dizer "nenhuma restricao", em
    /// silencio, justamente para o publico principal do projeto.
    #[test]
    fn intencao_em_portugues_e_classificada() {
        let casos = [
            ("escreve uma funcao que soma dois numeros", AgentMode::Code),
            ("implementar o parser de config", AgentMode::Code),
            (
                "por que o teste nao funciona? deu erro ao rodar",
                AgentMode::Debug,
            ),
            (
                "qual arquivo tem o handler de login? procura por ele",
                AgentMode::Search,
            ),
            (
                "como estruturar o modulo de auth? melhor forma de fazer",
                AgentMode::Architect,
            ),
            ("o que e um trait em rust? explica", AgentMode::Ask),
        ];
        for (frase, esperado) in casos {
            assert_eq!(
                classify_heuristic(frase),
                Some(esperado),
                "frase: {frase:?}"
            );
        }
    }

    /// E precisa continuar dizendo "nao sei" quando nao sabe.
    ///
    /// O criterio (dois acertos e folga de dois) deixa de fora frases curtas
    /// que um humano classificaria — `"revisa esse codigo pra mim"` casa so
    /// `revisa`, e `"onde esta a funcao que valida o token?"` empata entre
    /// busca e codigo. Isso e deliberado e a direcao segura: sob politica
    /// aplicada, classificar errado **bloqueia** uma ferramenta que o usuario
    /// podia usar, enquanto nao classificar so deixa tudo liberado, que e o
    /// comportamento padrao de sempre. Baixar o limiar troca um incomodo por
    /// um bloqueio indevido.
    #[test]
    fn frase_ambigua_ou_curta_continua_sem_classificacao() {
        for frase in [
            "revisa esse codigo pra mim",
            "onde esta a funcao que valida o token?",
            "oi",
            "obrigado",
        ] {
            assert_eq!(classify_heuristic(frase), None, "frase: {frase:?}");
        }
    }

    /// Acento nao pode derrubar o turno.
    ///
    /// O caminho de LLM cortava a mensagem com `&text[..text.len().min(400)]`,
    /// e `len()` conta **bytes**: numa mensagem em portugues com mais de 400
    /// bytes, o corte cai no meio de um `ç`/`ã`/`é` e o slice entra em panico.
    /// Este teste exercita o corte diretamente, com o limite caindo dentro de
    /// um caractere de dois bytes.
    #[test]
    fn corte_para_o_classificador_llm_nao_quebra_caractere() {
        let mut texto = "a".repeat(399);
        texto.push('ç');
        texto.push_str(" e mais um tanto de texto depois do corte");

        let cortado: &str = match texto.char_indices().nth(400) {
            Some((limite, _)) => &texto[..limite],
            None => &texto,
        };

        assert_eq!(cortado.chars().count(), 400);
        assert!(
            cortado.ends_with('ç'),
            "o caractere do limite ficou inteiro"
        );
    }
}
