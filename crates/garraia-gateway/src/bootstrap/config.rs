//! Bootstrap configuration helpers.
//!
//! Slice 10.a of GAR-440 (Q10 of EPIC GAR-430 Quality Gates Phase 3.6)
//! extracted the path resolvers and the API-key precedence chain out of
//! `bootstrap.rs`. The precedence chain itself has since moved down into
//! [`garraia_config::provider_keys`], so that `garraia config check`, `/health`,
//! and the admin providers list can answer "does this provider have a usable
//! key?" the same way the boot path does — they previously disagreed, which is
//! how a provider could look configured on every surface and still be skipped
//! at startup.
//!
//! What remains here is the gateway-facing surface: the re-exports consumed via
//! `crate::bootstrap::{default_vault_path, resolve_api_key}` (used by
//! `admin::handlers`, `admin::mcp`, `router`, `state`) and the locked-vault
//! diagnostic.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use crate::state::SharedState;

/// Default vault path under the user's config directory.
///
/// Returns `Option` purely to preserve the signature its four call sites
/// (`router`, `state`, `admin::mcp` ×2) already branch on; the path is always
/// resolvable.
pub(crate) fn default_vault_path() -> Option<PathBuf> {
    Some(garraia_config::default_vault_path())
}

/// Os dois gates de canal — allowlist e pairing — vivem no [`AppState`] e
/// TODO canal compartilha as MESMAS instancias (#1189).
///
/// Cada bootstrap de canal montava um `Allowlist` e um `PairingManager`
/// proprios, e isso quebrava o pareamento em todos os 11 canais de uma vez:
///
/// - `/pair` gera o codigo em `state.pairing` (`commands.rs`), mas o handler
///   de mensagens chamava `claim()` na instancia local, cuja tabela esta
///   sempre vazia. O `claim()` devolvia `None`, o usuario caia em
///   `unauthorized` e a mensagem era descartada.
/// - Os dois `Allowlist` liam o MESMO arquivo e ambos gravam nele em
///   `add()`/`claim_owner()`, entao um sobrescrevia o owner ou o usuario
///   recem-pareado do outro — perda de escrita silenciosa em disco.
///
/// Voltar a construir essas instancias dentro de um bootstrap de canal
/// reintroduz o bug; o teste `nenhum_canal_constroi_gate_proprio` varre o
/// diretorio e falha se acontecer.
///
/// [`AppState`]: crate::state::AppState
pub(super) fn channel_gates(
    state: &SharedState,
) -> (
    Arc<Mutex<garraia_security::Allowlist>>,
    Arc<Mutex<garraia_security::PairingManager>>,
) {
    (Arc::clone(&state.allowlist), Arc::clone(&state.pairing))
}

/// Plan 0250 (GAR-771): emit one friendly, actionable warning when a credential
/// vault exists on disk but `GARRAIA_VAULT_PASSPHRASE` is not set — the exact
/// situation that silently disables providers/channels (the keys are encrypted
/// and we can't open the vault). Without this, the operator only sees a cryptic
/// "no API key" and has no idea their secrets are right there, locked.
///
/// This is the state the pre-v0.3.0 onboarding wizard produced by default: it
/// encrypted the key into the vault, then `garraia start` had no passphrase to
/// open it with. The wizard now writes to `config.yml` instead, so this warning
/// should only fire for operators who deliberately opted into the vault.
pub(crate) fn warn_if_vault_locked() {
    let Some(vault_path) = default_vault_path() else {
        return;
    };
    if !garraia_config::vault_present_but_locked(&vault_path) {
        return;
    }
    tracing::warn!(
        "🔒 Encontrei seu cofre de credenciais em {}, mas preciso da senha pra \
         abri-lo. Suas chaves estão guardadas e seguras — só defina a variável \
         GARRAIA_VAULT_PASSPHRASE (a mesma senha que você criou no wizard) e me \
         reinicie. Sem ela, eu subo, mas os provedores e canais ficam desligados.",
        vault_path.display()
    );
}

/// Resolve an API key using the priority chain vault -> config -> env var.
///
/// Delegates to [`garraia_config::resolve_api_key`]. Also used for channel
/// tokens (`TELEGRAM_BOT_TOKEN`, `SLACK_BOT_TOKEN`, `WHATSAPP_ACCESS_TOKEN`),
/// which follow the identical chain.
pub(crate) fn resolve_api_key(
    config_key: Option<&str>,
    vault_credential_key: &str,
    env_var: &str,
) -> Option<String> {
    garraia_config::resolve_api_key(config_key, vault_credential_key, env_var)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// #1189 — regressao. O `/pair` gera o codigo em `state.pairing`; se o
    /// gate entregue ao canal nao for a MESMA instancia, o `claim()` do
    /// handler de mensagens procura numa tabela vazia e todo mundo cai em
    /// `unauthorized`. Aqui a prova e de comportamento, nao de identidade:
    /// gera pelo `state` e resgata pelo handle do canal.
    #[test]
    fn gate_do_canal_ve_o_codigo_gerado_pelo_pair() {
        use garraia_agents::AgentRuntime;
        use garraia_channels::ChannelRegistry;
        use garraia_config::AppConfig;

        let state: SharedState = Arc::new(crate::state::AppState::new(
            AppConfig::default(),
            Arc::new(AgentRuntime::new()),
            ChannelRegistry::new(),
        ));

        let (allowlist, pairing) = channel_gates(&state);

        // Mesmas instancias, nao copias: uma allowlist paralela sobre o
        // mesmo arquivo perde escrita em disco no `add()`/`claim_owner()`.
        assert!(
            Arc::ptr_eq(&allowlist, &state.allowlist),
            "o canal recebeu um Allowlist diferente do AppState"
        );
        assert!(
            Arc::ptr_eq(&pairing, &state.pairing),
            "o canal recebeu um PairingManager diferente do AppState"
        );

        // `/pair` gera aqui...
        let code = state.pairing.lock().unwrap().generate("telegram");
        // ...e o handler de mensagens do canal resgata aqui.
        let claimed = pairing.lock().unwrap().claim(&code, "7978617919");
        assert_eq!(
            claimed.as_deref(),
            Some("telegram"),
            "codigo gerado pelo /pair precisa ser resgatavel pelo gate do canal"
        );
    }

    /// #1189 — invariante de fonte. O unico lugar que constroi os gates e o
    /// `AppState`; bootstrap de canal so pode pegar via [`channel_gates`].
    /// Sem isso o bug volta por copy-paste no proximo canal (foi assim que
    /// ele chegou a 11 arquivos de uma vez).
    #[test]
    fn nenhum_canal_constroi_gate_proprio() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/bootstrap");
        let mut violacoes = Vec::new();

        for entry in std::fs::read_dir(&dir).expect("bootstrap/ legivel") {
            let path = entry.expect("entrada legivel").path();
            if path.extension().and_then(|e| e.to_str()) != Some("rs") {
                continue;
            }
            let nome = path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or_default()
                .to_string();
            // `config.rs` e este proprio teste: os marcadores aparecem aqui
            // como literais, e nao como construcao de gate.
            if nome == "config.rs" {
                continue;
            }
            let fonte = std::fs::read_to_string(&path).expect("fonte legivel");
            for marcador in ["Allowlist::load_or_create(", "PairingManager::new("] {
                if fonte.contains(marcador) {
                    violacoes.push(format!("{nome} constroi `{marcador}`"));
                }
            }
        }

        assert!(
            violacoes.is_empty(),
            "gate de canal duplicado quebra o /pair (#1189) -- use \
             `channel_gates(state)`: {violacoes:?}"
        );
    }

    #[test]
    fn resolve_api_key_prefers_config_over_env() {
        // Config value should win when present
        let result = resolve_api_key(
            Some("from-config"),
            "NONEXISTENT_VAULT_KEY",
            "NONEXISTENT_ENV_VAR_12345",
        );
        assert_eq!(result, Some("from-config".to_string()));
    }

    #[test]
    fn resolve_api_key_falls_back_to_env() {
        // Set a unique env var for this test
        let var_name = "GARRAIA_TEST_API_KEY_BOOTSTRAP_72";
        // SAFETY: this test is single-threaded and uses a unique env var name.
        unsafe { std::env::set_var(var_name, "from-env") };
        let result = resolve_api_key(None, "NONEXISTENT_VAULT_KEY", var_name);
        assert_eq!(result, Some("from-env".to_string()));
        unsafe { std::env::remove_var(var_name) };
    }

    #[test]
    fn resolve_api_key_returns_none_when_all_missing() {
        let result = resolve_api_key(None, "NONEXISTENT_VAULT_KEY", "NONEXISTENT_ENV_VAR_99999");
        assert_eq!(result, None);
    }

    /// Guards the refactor that removed fifteen hardcoded `("X_API_KEY",
    /// "X_API_KEY")` pairs from `build_agent_runtime`: every provider string the
    /// boot loop matches on must still resolve to an env var through the shared
    /// table, and the keyless ones must still report `None`.
    #[test]
    fn provider_key_table_covers_every_arm_of_the_boot_loop() {
        use garraia_config::provider_key_env;

        for (provider, expected) in [
            ("anthropic", "ANTHROPIC_API_KEY"),
            ("openai", "OPENAI_API_KEY"),
            ("sansa", "SANSA_API_KEY"),
            ("deepseek", "DEEPSEEK_API_KEY"),
            ("mistral", "MISTRAL_API_KEY"),
            ("gemini", "GEMINI_API_KEY"),
            ("falcon", "FALCON_API_KEY"),
            ("jais", "JAIS_API_KEY"),
            ("qwen", "QWEN_API_KEY"),
            ("yi", "YI_API_KEY"),
            ("cohere", "COHERE_API_KEY"),
            ("minimax", "MINIMAX_API_KEY"),
            ("moonshot", "MOONSHOT_API_KEY"),
            ("openrouter", "OPENROUTER_API_KEY"),
        ] {
            assert_eq!(
                provider_key_env(provider),
                Some(expected),
                "boot loop matches on `{provider}` but the shared table disagrees"
            );
        }

        // Keyless arms: these must never be reported as missing a key.
        assert_eq!(provider_key_env("ollama"), None);
        assert_eq!(provider_key_env("llamacpp"), None);
        assert_eq!(provider_key_env("echo"), None);
    }
}
