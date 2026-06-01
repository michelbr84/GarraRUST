//! Garra's default persona — the warm, human voice it speaks with when the
//! operator has not supplied a custom `system_prompt`.
//!
//! Plan 0250 (GAR-771). Before this module, an unset `system_prompt` meant the
//! agent fell back to the base model's neutral tone (`unwrap_or_default()` →
//! empty system message). That made "Garra" feel like a generic LLM rather than
//! a personal assistant with an identity.
//!
//! Design (plan 0250 §2):
//! - First person, warm, concise, PT-BR first with an EN fallback.
//! - Helpful and reassuring, never sycophantic.
//! - Sparing emoji.
//!
//! The persona is **always overridable**: any non-empty `system_prompt` from
//! config or a per-request override wins. Choosing `PersonaMode::Neutral`
//! restores the pre-0250 behavior (no default system prompt at all).

/// Which voice the agent uses when no explicit `system_prompt` is set.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PersonaMode {
    /// Warm "Garra" persona (plan 0250 default).
    #[default]
    Friendly,
    /// No default system prompt — pre-0250 behavior. The base model's own tone.
    Neutral,
}

/// Language used to pick the default persona copy.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Lang {
    /// Brazilian Portuguese (primary audience).
    #[default]
    Pt,
    /// English fallback.
    En,
}

impl Lang {
    /// Best-effort parse from a BCP-47-ish tag or short code. Anything that is
    /// not clearly English falls back to PT-BR (the primary audience).
    pub fn from_code(code: &str) -> Self {
        let c = code.trim().to_ascii_lowercase();
        if c.starts_with("en") {
            Lang::En
        } else {
            Lang::Pt
        }
    }
}

/// The warm Garra persona in Brazilian Portuguese.
pub const DEFAULT_PERSONA_PT: &str = "\
Você é o Garra — um assistente pessoal de IA, leal e prestativo, que conversa \
em português do Brasil de forma natural e calorosa.

Como você se comporta:
- Fala na primeira pessoa (\"eu\", \"vou te ajudar\") e com naturalidade, como \
um colega de confiança — não como um robô.
- É caloroso, mas direto e conciso: acolhe sem enrolar.
- Quando algo dá errado, explica de forma tranquila o que aconteceu e qual é o \
próximo passo, sem despejar mensagens técnicas cruas.
- Usa emojis com parcimônia (no máximo um, quando agrega) — 👋 ao cumprimentar, \
🐾 num toque amigável. Nunca poluir.
- Não bajula (\"que ótima pergunta!\") nem inventa: é gentil, honesto e \
competente. Se não sabe, diz que não sabe e sugere um caminho.
- Respeita o usuário e a privacidade dele.

Seu nome é Garra. Você existe para tornar a vida da pessoa (ou da família/equipe) \
mais fácil.";

/// The warm Garra persona in English.
pub const DEFAULT_PERSONA_EN: &str = "\
You are Garra — a loyal, helpful personal AI assistant who speaks naturally and \
warmly.

How you behave:
- Speak in the first person (\"I\", \"I'll help you with that\") and \
conversationally, like a trusted colleague — not like a robot.
- Be warm but direct and concise: welcoming without padding.
- When something goes wrong, calmly explain what happened and the next step, \
instead of dumping raw technical errors.
- Use emoji sparingly (at most one, when it adds something) — 👋 to greet, 🐾 \
for a friendly touch. Never clutter.
- Don't flatter (\"what a great question!\") or make things up: be kind, honest, \
and competent. If you don't know, say so and suggest a path forward.
- Respect the user and their privacy.

Your name is Garra. You exist to make the person's (or family's/team's) life \
easier.";

/// Return the default persona system prompt for the given language.
pub fn default_persona(lang: Lang) -> &'static str {
    match lang {
        Lang::Pt => DEFAULT_PERSONA_PT,
        Lang::En => DEFAULT_PERSONA_EN,
    }
}

/// Resolve the effective base system prompt.
///
/// Priority (plan 0250 §5 — operator override always wins):
/// 1. A non-empty explicit prompt (from config or a per-request override).
/// 2. Otherwise, the default persona for `lang` — unless `mode` is
///    [`PersonaMode::Neutral`], in which case there is no default (pre-0250
///    behavior, returns `None`).
pub fn resolve_system_prompt(
    explicit: Option<&str>,
    mode: PersonaMode,
    lang: Lang,
) -> Option<String> {
    if let Some(p) = explicit
        && !p.trim().is_empty()
    {
        return Some(p.to_string());
    }
    match mode {
        PersonaMode::Friendly => Some(default_persona(lang).to_string()),
        PersonaMode::Neutral => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lang_parsing_defaults_to_pt() {
        assert_eq!(Lang::from_code("pt-BR"), Lang::Pt);
        assert_eq!(Lang::from_code("pt"), Lang::Pt);
        assert_eq!(Lang::from_code("en-US"), Lang::En);
        assert_eq!(Lang::from_code("EN"), Lang::En);
        // Unknown → PT (primary audience).
        assert_eq!(Lang::from_code("fr"), Lang::Pt);
        assert_eq!(Lang::from_code(""), Lang::Pt);
    }

    #[test]
    fn persona_text_mentions_garra_and_is_nonempty() {
        assert!(DEFAULT_PERSONA_PT.contains("Garra"));
        assert!(DEFAULT_PERSONA_EN.contains("Garra"));
        assert!(default_persona(Lang::Pt).len() > 100);
        assert!(default_persona(Lang::En).len() > 100);
    }

    #[test]
    fn explicit_prompt_always_wins() {
        let got = resolve_system_prompt(Some("custom voice"), PersonaMode::Friendly, Lang::Pt);
        assert_eq!(got.as_deref(), Some("custom voice"));
        // Even in neutral mode an explicit prompt is honored.
        let got = resolve_system_prompt(Some("custom voice"), PersonaMode::Neutral, Lang::Pt);
        assert_eq!(got.as_deref(), Some("custom voice"));
    }

    #[test]
    fn empty_explicit_falls_back_to_persona() {
        // None → persona in Friendly mode.
        let got = resolve_system_prompt(None, PersonaMode::Friendly, Lang::Pt);
        assert_eq!(got.as_deref(), Some(DEFAULT_PERSONA_PT));
        // Whitespace-only counts as empty.
        let got = resolve_system_prompt(Some("   "), PersonaMode::Friendly, Lang::En);
        assert_eq!(got.as_deref(), Some(DEFAULT_PERSONA_EN));
    }

    #[test]
    fn neutral_mode_reproduces_pre_0250_behavior() {
        // No explicit prompt + neutral → no default (old behavior).
        let got = resolve_system_prompt(None, PersonaMode::Neutral, Lang::Pt);
        assert_eq!(got, None);
        let got = resolve_system_prompt(Some(""), PersonaMode::Neutral, Lang::En);
        assert_eq!(got, None);
    }
}
