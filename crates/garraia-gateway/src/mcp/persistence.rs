//! Persistence for MCP server configuration.
//!
//! [`McpPersistenceService`] is the single place where `mcp.json` is read and
//! written. It uses the gateway's [`McpConfig`] format (compatible with Claude
//! Desktop) and bridges it to the in-memory [`McpRuntimeRegistry`].
//!
//! # File location
//!
//! By default the file lives at `<config_dir>/mcp.json` where `config_dir` is
//! resolved by [`garraia_config::ConfigLoader::default_config_dir`]
//! (usually `~/.garraia/` or `$XDG_CONFIG_HOME/garraia/`).

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use tracing::{debug, info, warn};

use super::{McpConfig, McpRuntimeRegistry, is_sensitive_key};

/// Sentinel prefix for vault-referenced env values in `mcp.json`.
const VAULT_REF_PREFIX: &str = "vault:";

/// Returns the vault key used to store `env_key` for `server_name`.
fn vault_key(server_name: &str, env_key: &str) -> String {
    format!("mcp.{server_name}.{env_key}")
}

/// Resolve `vault:` refs em um mapa `env`, reescrevendo cada referência pelo
/// valor do cofre. Valores sem o prefixo ficam como estão.
///
/// Devolve as referências (`env_key`, `ref`) que **não** resolveram; a
/// política de cada caminho é do chamador:
///
/// - **Boot** (`build_mcp_tools`, #1237): fechado — qualquer ref não
///   resolvida impede o servidor de subir. O caminho de registry pode
///   tolerar o literal porque a admin API mostra a config para depurar; o
///   boot não pode: o literal iria direto para o processo filho como valor
///   da variável, e um retry do `pending` (#1242) entregaria o literal de
///   novo — por isso a porta fecha antes de qualquer spawn.
/// - **Registry** (admin API, GAR-291): tolerante — avisa e deixa o literal.
pub(crate) fn resolver_env_com_vault(
    env: &mut HashMap<String, String>,
    vault_path: &Path,
) -> Vec<(String, String)> {
    let mut nao_resolvidos = Vec::new();
    for (env_key, env_val) in env.iter_mut() {
        if let Some(vk) = env_val.strip_prefix(VAULT_REF_PREFIX) {
            match garraia_security::try_vault_get(vault_path, vk) {
                Some(resolved) => *env_val = resolved,
                None => nao_resolvidos.push((env_key.clone(), vk.to_string())),
            }
        }
    }
    nao_resolvidos
}

/// Loads and saves `mcp.json`, and builds [`McpRuntimeRegistry`] from it.
///
/// When a `vault_path` is configured (via [`with_vault`](Self::with_vault)),
/// sensitive env vars (API keys, tokens, etc.) are stored encrypted in the
/// vault and replaced by `vault:<key>` references in `mcp.json`. On load,
/// vault references are resolved back to their plaintext values for use at
/// runtime. If the vault is unavailable, plaintext values are used as-is.
#[derive(Clone)]
pub struct McpPersistenceService {
    path: PathBuf,
    /// Path to the AES-256-GCM credential vault (see `garraia-security`).
    /// When `None`, env vars are saved as plaintext (with a warning).
    vault_path: Option<PathBuf>,
}

impl McpPersistenceService {
    /// Create a service that reads/writes the file at `path`.
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self {
            path: path.into(),
            vault_path: None,
        }
    }

    /// Create a service pointing to the default `mcp.json` location
    /// (`<garraia_config_dir>/mcp.json`).
    pub fn with_default_path() -> Self {
        Self::new(garraia_config::ConfigLoader::default_config_dir().join("mcp.json"))
    }

    /// Attach an encrypted vault for sensitive env var storage (GAR-291).
    ///
    /// When set, [`load_registry`](Self::load_registry) resolves `vault:` refs
    /// from the vault and [`save_from_registry`](Self::save_from_registry)
    /// encrypts sensitive values into the vault.
    pub fn with_vault(mut self, vault_path: impl Into<PathBuf>) -> Self {
        self.vault_path = Some(vault_path.into());
        self
    }

    /// The path this service manages.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Env var that opts a boot out of first-run MCP provisioning.
    ///
    /// Set it to `1` (or any non-empty value) when the gateway must come up
    /// without touching the network. See
    /// [`provision_filesystem_if_missing`](Self::provision_filesystem_if_missing).
    pub const DISABLE_AUTOPROVISION_ENV: &'static str = "GARRAIA_DISABLE_MCP_AUTOPROVISION";

    /// Seed `mcp.json` with a Filesystem MCP entry when the file does not exist yet.
    ///
    /// This is a first-run convenience: new installations get local filesystem
    /// access immediately without requiring manual admin-UI configuration.
    /// Existing installations (file already present) are **never** modified.
    ///
    /// The allowed root is the user's home directory (`$HOME` / `%USERPROFILE%`),
    /// falling back to the parent of `~/.garraia/` if the env var is absent.
    ///
    /// # Por que existe um opt-out
    ///
    /// A entrada provisionada roda `npx -y @modelcontextprotocol/server-filesystem`,
    /// e o `-y` **baixa o pacote do npm na primeira execução**. Como o boot do
    /// gateway spawna os servidores MCP logo em seguida (`build_mcp_tools`), num
    /// ambiente de cache frio esse download entra no caminho crítico do start.
    ///
    /// Em CI isso já custou uma `main` vermelha: o primeiro
    /// `start_test_gateway()` de `projects_test.rs` estourou os 30 s do laço de
    /// espera enquanto o npm baixava, e o teste seguinte bateu numa porta
    /// fechada com um `ConnectionRefused` que não dizia nada sobre a causa. Os
    /// jobs `E2E Tests` e `Playwright` caíram no mesmo passo "Start gateway".
    ///
    /// Pior: um teste que faz `config.mcp.clear()` para se isolar de MCP tinha
    /// esse isolamento desfeito aqui, porque a provisão grava no config dir
    /// temporário do próprio teste.
    ///
    /// Por isso [`DISABLE_AUTOPROVISION_ENV`](Self::DISABLE_AUTOPROVISION_ENV):
    /// quem sobe o gateway sabendo que não vai exercitar MCP declara isso e o
    /// boot deixa de depender da rede. Não muda nada para o usuário final — o
    /// default segue provisionando.
    pub fn provision_filesystem_if_missing(&self) {
        if std::env::var_os(Self::DISABLE_AUTOPROVISION_ENV)
            .is_some_and(|v| !v.is_empty() && v != "0")
        {
            debug!(
                "mcp: auto-provisionamento desligado por {}",
                Self::DISABLE_AUTOPROVISION_ENV
            );
            return;
        }

        if self.path.exists() {
            return;
        }

        // Resolve the user's home directory in a cross-platform way.
        let home_dir = std::env::var("HOME")
            .or_else(|_| std::env::var("USERPROFILE"))
            .map(PathBuf::from)
            .unwrap_or_else(|_| {
                // Fallback: parent of ~/.garraia/ → ~
                self.path
                    .parent()
                    .and_then(|p| p.parent())
                    .map(|p| p.to_path_buf())
                    .unwrap_or_else(|| PathBuf::from("."))
            });

        let mut config = McpConfig::default();
        config.mcp_servers.insert(
            "filesystem".to_string(),
            super::McpServerConfig {
                command: Some("npx".to_string()),
                args: vec![
                    "-y".to_string(),
                    "@modelcontextprotocol/server-filesystem".to_string(),
                    home_dir.to_string_lossy().into_owned(),
                ],
                env: Default::default(),
                url: None,
                transport: None,
                timeout_secs: 30,
                memory_limit_mb: None,
                max_restarts: None,
                restart_delay_secs: None,
                allowed_tools: Vec::new(),
                inherit_env: false,
                enabled: None,
            },
        );

        match self.save(&config) {
            Ok(()) => info!(
                "mcp: provisioned default mcp.json with filesystem MCP at {}",
                home_dir.display()
            ),
            Err(e) => warn!("mcp: failed to provision default mcp.json: {e}"),
        }
    }

    /// Load `mcp.json` from disk.
    ///
    /// Returns an empty [`McpConfig`] when the file does not exist yet.
    pub fn load(&self) -> anyhow::Result<McpConfig> {
        let config = McpConfig::load(&self.path)?;
        info!(
            "mcp persistence: loaded {} server(s) from {}",
            config.mcp_servers.len(),
            self.path.display()
        );
        Ok(config)
    }

    /// Save a [`McpConfig`] snapshot to `mcp.json` (pretty-printed JSON).
    pub fn save(&self, config: &McpConfig) -> anyhow::Result<()> {
        config.save(&self.path)?;
        info!(
            "mcp persistence: saved {} server(s) to {}",
            config.mcp_servers.len(),
            self.path.display()
        );
        Ok(())
    }

    /// Load the file and build a registry with all servers in [`McpStatus::Stopped`].
    ///
    /// Call [`McpRuntimeRegistry::sync_from_manager`] after the agent-layer
    /// connections are established to update the live statuses.
    ///
    /// **GAR-291**: If a vault is configured, `vault:` references in env values
    /// are resolved to their plaintext secrets before the registry is built.
    /// Servers loaded THROUGH THIS PATH are never exposed to unresolved
    /// `vault:` strings at runtime.
    ///
    /// The qualifier is load-bearing (#1237): this is the registry path
    /// (admin API / marketplace). The gateway ALSO reads `mcp.json` at boot
    /// through `ConfigLoader::merged_mcp_config`, which does no resolution at
    /// all — a `vault:` reference on that path reaches the child process as
    /// the literal string. Do not read this sentence as "the product resolves
    /// `vault:` everywhere"; it does not, and #1237 is about closing the gap.
    pub fn load_registry(&self) -> McpRuntimeRegistry {
        match self.load() {
            Ok(mut config) => {
                self.resolve_vault_refs(&mut config);
                McpRuntimeRegistry::new(&config)
            }
            Err(e) => {
                warn!(
                    "mcp persistence: failed to load mcp.json ({}), starting with empty registry: {e}",
                    self.path.display()
                );
                McpRuntimeRegistry::new(&McpConfig::default())
            }
        }
    }

    /// Snapshot the registry's current config and write it to `mcp.json`.
    ///
    /// **GAR-291**: If a vault is configured and `GARRAIA_VAULT_PASSPHRASE` is
    /// set, sensitive env values are moved to the vault and replaced by
    /// `vault:<key>` references. If the vault is unavailable, plaintext is
    /// written with a warning (graceful degradation — never blocks operation).
    pub async fn save_from_registry(&self, registry: &McpRuntimeRegistry) -> anyhow::Result<()> {
        let mut config = registry.config_snapshot().await;
        self.encrypt_to_vault(&mut config);
        self.save(&config)
    }

    /// Remove all vault credentials whose key starts with `mcp.<server_name>.`.
    ///
    /// Called by the DELETE endpoint (GAR-286) so that removing a server also
    /// cleans up its encrypted credentials. No-op if no vault is configured or
    /// `GARRAIA_VAULT_PASSPHRASE` is not set.
    pub fn delete_server_vault_entries(&self, server_name: &str) {
        if let Some(vault_path) = &self.vault_path {
            let prefix = format!("mcp.{server_name}.");
            let removed = garraia_security::try_vault_delete_prefix(vault_path, &prefix);
            if removed > 0 {
                info!(server = %server_name, "mcp: removed {removed} vault credential(s)");
            }
        }
    }

    // ── Vault helpers (GAR-291) ───────────────────────────────────────────────

    /// Resolve `vault:` references in env values using the configured vault.
    ///
    /// Values that are already plaintext (no prefix) are left untouched.
    /// Unresolvable `vault:` refs emit a warning and remain as-is so the
    /// server config is still visible for debugging — this is the registry
    /// path, tolerant by design; the boot path is fail-closed (see
    /// [`resolver_env_com_vault`] and #1237).
    fn resolve_vault_refs(&self, config: &mut McpConfig) {
        let vault_path = match &self.vault_path {
            Some(p) => p.as_path(),
            None => return, // no vault configured — nothing to resolve
        };

        for (server_name, server_cfg) in config.mcp_servers.iter_mut() {
            for (env_key, vk) in resolver_env_com_vault(&mut server_cfg.env, vault_path) {
                warn!(
                    server = %server_name,
                    env_key = %env_key,
                    vault_ref = %vk,
                    "mcp: vault ref unresolvable — vault missing or GARRAIA_VAULT_PASSPHRASE not set"
                );
            }
        }
    }

    /// Move sensitive plaintext env values into the vault and replace them
    /// with `vault:` references in `config`.
    ///
    /// No-op if no vault is configured or `GARRAIA_VAULT_PASSPHRASE` is absent.
    fn encrypt_to_vault(&self, config: &mut McpConfig) {
        let vault_path = match &self.vault_path {
            Some(p) => p.as_path(),
            None => {
                // Check if any server has sensitive keys and warn if so.
                let has_secrets = config
                    .mcp_servers
                    .values()
                    .any(|s| s.env.keys().any(|k| is_sensitive_key(k)));
                if has_secrets {
                    warn!(
                        "mcp: vault not configured — saving sensitive env vars as plaintext; set GARRAIA_VAULT_PASSPHRASE to enable encryption"
                    );
                }
                return;
            }
        };

        if garraia_security::vault_passphrase_from_env().is_none() {
            let has_secrets = config
                .mcp_servers
                .values()
                .any(|s| s.env.keys().any(|k| is_sensitive_key(k)));
            if has_secrets {
                warn!(
                    "mcp: GARRAIA_VAULT_PASSPHRASE not set — sensitive env vars saved as plaintext"
                );
            }
            return;
        }

        for (server_name, server_cfg) in config.mcp_servers.iter_mut() {
            for (env_key, env_val) in server_cfg.env.iter_mut() {
                // Skip values already stored as vault references.
                if env_val.starts_with(VAULT_REF_PREFIX) {
                    continue;
                }
                if !is_sensitive_key(env_key) {
                    continue;
                }
                let vk = vault_key(server_name, env_key);
                if garraia_security::try_vault_set(vault_path, &vk, env_val) {
                    info!(
                        server = %server_name,
                        env_key = %env_key,
                        "mcp: stored credential in vault"
                    );
                    *env_val = format!("{VAULT_REF_PREFIX}{vk}");
                } else {
                    warn!(
                        server = %server_name,
                        env_key = %env_key,
                        "mcp: failed to store credential in vault — saving as plaintext"
                    );
                }
            }
        }
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mcp::{McpServerConfig, McpStatus};

    /// #1237: valores plaintext passam inteiros; toda `vault:` de um mapa
    /// com cofre INEXISTENTE volta como não resolvida — sem ler passphrase
    /// (o vault nem existe), então o teste não depende de ambiente.
    #[test]
    fn resolver_env_com_vault_sem_cofre_deixa_plaintext_e_lista_refs() {
        let mut env = HashMap::from([
            ("PLAIN".to_string(), "valor-cru".to_string()),
            ("REF_A".to_string(), "vault:mcp.ok.A".to_string()),
            ("REF_B".to_string(), "vault:mcp.ok.B".to_string()),
            ("JA_RESOLVIDO".to_string(), "token-ja-literal".to_string()),
        ]);
        let dir = tempfile::tempdir().expect("tempdir");
        let nao_resolvidos =
            resolver_env_com_vault(&mut env, dir.path().join("vault.json").as_path());
        assert_eq!(env["PLAIN"], "valor-cru", "plaintext não pode ser tocado");
        assert_eq!(
            env["JA_RESOLVIDO"], "token-ja-literal",
            "valor sem prefixo não pode ser tocado"
        );
        assert!(
            nao_resolvidos.contains(&("REF_A".to_string(), "mcp.ok.A".to_string())),
            "ref sem cofre precisa voltar como não resolvida: {nao_resolvidos:?}"
        );
        assert!(
            nao_resolvidos.contains(&("REF_B".to_string(), "mcp.ok.B".to_string())),
            "ref sem cofre precisa voltar como não resolvida: {nao_resolvidos:?}"
        );
        assert_eq!(nao_resolvidos.len(), 2);
    }

    fn temp_mcp_json(content: &str) -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("mcp.json");
        std::fs::write(&path, content).expect("write");
        (dir, path)
    }

    fn make_config(names: &[&str]) -> McpConfig {
        let mcp_servers = names
            .iter()
            .map(|&n| {
                (
                    n.to_string(),
                    McpServerConfig {
                        command: Some("npx".into()),
                        args: vec![n.into()],
                        ..Default::default()
                    },
                )
            })
            .collect();
        McpConfig { mcp_servers }
    }

    #[test]
    fn load_parses_valid_mcp_json() {
        let json = r#"{"mcpServers":{"gradio":{"command":"npx","args":["mcp-remote"]}}}"#;
        let (_dir, path) = temp_mcp_json(json);
        let svc = McpPersistenceService::new(&path);
        let cfg = svc.load().expect("load");
        assert_eq!(cfg.mcp_servers.len(), 1);
        assert!(cfg.mcp_servers.contains_key("gradio"));
    }

    #[test]
    fn load_returns_empty_when_missing() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("nonexistent.json");
        let svc = McpPersistenceService::new(&path);
        let cfg = svc.load().expect("load returns default");
        assert!(cfg.mcp_servers.is_empty());
    }

    #[test]
    fn save_and_reload_roundtrip() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("mcp.json");
        let svc = McpPersistenceService::new(&path);

        let original = make_config(&["alpha", "beta"]);
        svc.save(&original).expect("save");

        let loaded = svc.load().expect("reload");
        assert_eq!(loaded.mcp_servers.len(), 2);
        assert!(loaded.mcp_servers.contains_key("alpha"));
        assert!(loaded.mcp_servers.contains_key("beta"));
    }

    #[test]
    fn save_creates_parent_dirs() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("nested").join("dir").join("mcp.json");
        let svc = McpPersistenceService::new(&path);
        let cfg = make_config(&["server1"]);
        svc.save(&cfg).expect("save with nested dirs");
        assert!(path.exists());
    }

    #[test]
    fn load_registry_builds_all_stopped() {
        let json = r#"{"mcpServers":{"s1":{"command":"cmd1"},"s2":{"command":"cmd2"}}}"#;
        let (_dir, path) = temp_mcp_json(json);
        let svc = McpPersistenceService::new(&path);
        let reg = svc.load_registry();

        // We can't await in sync tests easily, so use block_on
        let rt = tokio::runtime::Runtime::new().unwrap();
        let servers = rt.block_on(reg.list());
        assert_eq!(servers.len(), 2);
        for s in &servers {
            assert_eq!(s.status, McpStatus::Stopped);
        }
    }

    #[tokio::test]
    async fn save_from_registry_persists_config() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("mcp.json");
        let svc = McpPersistenceService::new(&path);

        let cfg = make_config(&["my-server"]);
        let reg = McpRuntimeRegistry::new(&cfg);
        // Change live status — should NOT appear in saved file (only config)
        reg.set_status("my-server", McpStatus::Running, 5).await;

        svc.save_from_registry(&reg).await.expect("save");

        // Reload and check
        let loaded = svc.load().expect("reload");
        assert_eq!(loaded.mcp_servers.len(), 1);
        assert!(loaded.mcp_servers.contains_key("my-server"));
    }

    /// Issue #1273 — a sonda do corpo da issue, como teste de regressão: um
    /// `add_server` + `save_from_registry` não pode apagar o que os OUTROS
    /// servidores declaram no disco.
    ///
    /// O fixture é o arquivo exatamente como o operador (ou o wizard) o
    /// escreve: chaves snake_case, no schema `garraia_config` que o loader
    /// de boot lê — allowlist GAR-190, válvula `inherit_env` (#1075),
    /// chave de boot `enabled` e tuning. Antes de #1273 o tipo do registry
    /// não carregava nada disso, então o snapshot que `save_from_registry`
    /// serializa derrubava os campos de TODOS os servidores do arquivo —
    /// `enabled: false` religando o servidor no próximo boot incluído.
    ///
    /// À prova de mutação: remova qualquer campo asserido de
    /// `McpServerConfig` e o load deixa de capturá-lo, o arquivo reescrito
    /// fica sem ele e a asserção falla.
    #[tokio::test]
    async fn save_from_registry_preserves_declared_fields_of_other_servers() {
        let fixture = serde_json::json!({
            "mcpServers": {
                "keeper": {
                    "command": "python3",
                    "args": ["-m", "keeper"],
                    "transport": "stdio",
                    "timeout": 10,
                    "allowed_tools": ["read_file", "write_file"],
                    "inherit_env": true,
                    "enabled": false,
                    "memory_limit_mb": 512,
                    "max_restarts": 3,
                    "restart_delay_secs": 2
                }
            }
        });
        let (_dir, path) =
            temp_mcp_json(&serde_json::to_string_pretty(&fixture).expect("serialize fixture"));
        let svc = McpPersistenceService::new(&path);

        let reg = svc.load_registry();
        // O registry capturou os campos declarados (tolerância de alias).
        {
            let keeper = reg.get("keeper").await.expect("keeper loaded");
            assert_eq!(keeper.config.allowed_tools.len(), 2);
            assert!(keeper.config.inherit_env);
            assert_eq!(keeper.config.enabled, Some(false));
            assert_eq!(keeper.config.memory_limit_mb, Some(512));
        }

        // A escrita destrutiva que antes os apagava.
        reg.add_server(
            "novo",
            McpServerConfig {
                command: Some("echo".into()),
                ..Default::default()
            },
        )
        .await;
        svc.save_from_registry(&reg).await.expect("save");

        // Lê o ARQUIVO cru — a sonda exata do corpo da issue.
        let raw: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&path).expect("read back"))
                .expect("parse back");
        let keeper = &raw["mcpServers"]["keeper"];
        assert_eq!(
            keeper["allowed_tools"],
            serde_json::json!(["read_file", "write_file"]),
            "a allowlist declarada no disco deve sobreviver a uma escrita de admin dos OUTROS servidores"
        );
        assert_eq!(keeper["inherit_env"], serde_json::json!(true));
        assert_eq!(keeper["enabled"], serde_json::json!(false));
        // O tuning preserva o valor: o load lê a grafia snake_case via alias
        // e a gravação usa a grafia canônica do writer (camelCase).
        assert_eq!(keeper["memoryLimitMb"], serde_json::json!(512));
        assert_eq!(keeper["maxRestarts"], serde_json::json!(3));
        assert_eq!(keeper["timeoutSecs"], serde_json::json!(10));
        // E o recém-chegado está lá, allow-all (sem chave de allowlist).
        assert!(raw["mcpServers"]["novo"].is_object());
        assert!(raw["mcpServers"]["novo"]["allowed_tools"].is_null());
    }

    /// O default segue provisionando — o opt-out não pode mudar o que o
    /// usuário final vê num primeiro boot.
    #[test]
    #[serial_test::serial]
    fn provision_writes_the_filesystem_entry_by_default() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("mcp.json");
        // SAFETY: teste serializado; ninguém mais lê esta var em paralelo.
        unsafe { std::env::remove_var(McpPersistenceService::DISABLE_AUTOPROVISION_ENV) };

        McpPersistenceService::new(&path).provision_filesystem_if_missing();

        let loaded = McpPersistenceService::new(&path).load().expect("load");
        let fs = loaded
            .mcp_servers
            .get("filesystem")
            .expect("entrada filesystem provisionada");
        assert_eq!(fs.command.as_deref(), Some("npx"));
    }

    /// Com o opt-out ligado o boot não grava nada — e portanto não spawna
    /// `npx -y`, que baixa do npm na primeira execução. É o que mantém o start
    /// do gateway fora da rede em CI (ver o doc de
    /// `provision_filesystem_if_missing`).
    #[test]
    #[serial_test::serial]
    fn provision_is_skipped_when_the_opt_out_is_set() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("mcp.json");
        // SAFETY: teste serializado; ninguém mais lê esta var em paralelo.
        unsafe { std::env::set_var(McpPersistenceService::DISABLE_AUTOPROVISION_ENV, "1") };

        McpPersistenceService::new(&path).provision_filesystem_if_missing();

        assert!(
            !path.exists(),
            "opt-out ligado deve deixar o mcp.json inexistente"
        );

        // SAFETY: idem.
        unsafe { std::env::remove_var(McpPersistenceService::DISABLE_AUTOPROVISION_ENV) };
    }

    /// `0` e vazio são "desligado" — senão um `export VAR=` de shell viraria
    /// opt-out acidental.
    #[test]
    #[serial_test::serial]
    fn opt_out_treats_zero_and_empty_as_disabled() {
        for value in ["0", ""] {
            let dir = tempfile::tempdir().expect("tempdir");
            let path = dir.path().join("mcp.json");
            // SAFETY: teste serializado.
            unsafe { std::env::set_var(McpPersistenceService::DISABLE_AUTOPROVISION_ENV, value) };

            McpPersistenceService::new(&path).provision_filesystem_if_missing();

            assert!(
                path.exists(),
                "valor {value:?} nao deve contar como opt-out"
            );
        }
        // SAFETY: idem.
        unsafe { std::env::remove_var(McpPersistenceService::DISABLE_AUTOPROVISION_ENV) };
    }
}
