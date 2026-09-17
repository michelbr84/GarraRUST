use std::collections::HashMap;
use std::path::{Path, PathBuf};

use garraia_common::{Error, Result};
use tracing::info;

use crate::model::{AppConfig, ChannelConfig, McpServerConfig};

pub struct ConfigLoader {
    config_dir: PathBuf,
}

impl ConfigLoader {
    pub fn new() -> Result<Self> {
        let config_dir = Self::default_config_dir();
        Ok(Self { config_dir })
    }

    pub fn default_config_dir() -> PathBuf {
        if let Ok(env_dir) = std::env::var("GARRAIA_CONFIG_DIR") {
            return PathBuf::from(env_dir);
        }

        let home_config = dirs::home_dir().map(|h| h.join(".garraia"));
        let xdg_config = dirs::config_dir().map(|c| c.join("garraia"));

        match (xdg_config, home_config) {
            (Some(xdg), Some(home)) => {
                // If XDG exists, prefer it.
                if xdg.exists() {
                    xdg
                }
                // If Home exists (and XDG doesn't), use Home (migration/legacy case).
                else if home.exists() {
                    home
                }
                // If neither exists, prefer XDG for new installs.
                else {
                    xdg
                }
            }
            (Some(xdg), None) => xdg,
            (None, Some(home)) => home,
            (None, None) => PathBuf::from(".garraia"),
        }
    }

    pub fn with_dir(config_dir: impl Into<PathBuf>) -> Self {
        Self {
            config_dir: config_dir.into(),
        }
    }

    pub fn config_dir(&self) -> &Path {
        &self.config_dir
    }

    pub fn load(&self) -> Result<AppConfig> {
        self.warn_if_legacy_dir_shadowed();
        let yaml_path = self.config_dir.join("config.yml");
        let toml_path = self.config_dir.join("config.toml");

        if yaml_path.exists() {
            info!("loading config from {}", yaml_path.display());
            let contents = std::fs::read_to_string(&yaml_path)?;
            // Um `config.yml` sem conteudo desserializa **com sucesso** para
            // `AppConfig::default()`, e esse e o pior resultado possivel: e a
            // forma exata que uma escrita truncada deixa o arquivo, e o que
            // vinha depois era um `save()` gravando os defaults por cima da
            // config do usuario — e todo gate que morava nela (chaves, canais,
            // credenciais) sumindo em silencio. Arquivo sem conteudo nao e "sem
            // config": e config ilegivel.
            //
            // Dois criterios, porque nenhum cobre o outro. `trim().is_empty()`
            // pega o arquivo em branco, inclusive quando ele tem um TAB, que o
            // YAML nem consegue tokenizar. E `Value::Null` pega o arquivo que
            // sobrou so com comentarios: ele tem bytes, passa pelo `trim`, cai
            // no mesmo `AppConfig::default()` e faria a mesma perda silenciosa
            // de gate. Erro de parse com conteudo de verdade cai fora daqui de
            // proposito: a mensagem do `from_str` abaixo diz linha e coluna, e
            // esta nao diria.
            let sem_conteudo = contents.trim().is_empty()
                || serde_yaml::from_str::<serde_yaml::Value>(&contents)
                    .map(|v| v.is_null())
                    .unwrap_or(false);
            if sem_conteudo {
                return Err(Error::Config(format!(
                    "{} esta vazio (ou so tem comentarios) — isso costuma ser uma \
                     escrita interrompida, nao uma config valida. Restaure o \
                     arquivo, ou apague-o para que os defaults valham.",
                    yaml_path.display()
                )));
            }
            serde_yaml::from_str(&contents)
                .map_err(|e| Error::Config(format!("failed to parse YAML config: {e}")))
        } else if toml_path.exists() {
            info!("loading config from {}", toml_path.display());
            let contents = std::fs::read_to_string(&toml_path)?;
            toml::from_str(&contents)
                .map_err(|e| Error::Config(format!("failed to parse TOML config: {e}")))
        } else {
            info!("no config file found, using defaults");
            Ok(AppConfig::default())
        }
    }

    /// Warn when config files exist in the legacy `~/.garraia` dir while the
    /// active dir is another one (normally `~/.config/garraia`, which
    /// `ensure_dirs` creates on every run). Historic docs pointed users at
    /// `~/.garraia`, so files edited there were silently ignored.
    fn warn_if_legacy_dir_shadowed(&self) {
        let Some(legacy) = dirs::home_dir().map(|h| h.join(".garraia")) else {
            return;
        };
        if legacy == self.config_dir {
            return;
        }
        let shadowed: Vec<&str> = ["config.yml", "config.toml", "mcp.json"]
            .into_iter()
            .filter(|f| legacy.join(f).exists())
            .collect();
        if !shadowed.is_empty() {
            tracing::warn!(
                "ignoring {:?} in legacy dir {} — the active config dir is {} \
                 (move the files there, or set GARRAIA_CONFIG_DIR={})",
                shadowed,
                legacy.display(),
                self.config_dir.display(),
                legacy.display()
            );
        }
    }

    /// Load MCP server configs from `<config_dir>/mcp.json` (Claude Desktop compatible format).
    /// Returns an empty map if the file does not exist.
    pub fn load_mcp_json(&self) -> HashMap<String, McpServerConfig> {
        let mcp_path = self.config_dir.join("mcp.json");
        if !mcp_path.exists() {
            return HashMap::new();
        }

        let contents = match std::fs::read_to_string(&mcp_path) {
            Ok(c) => c,
            Err(e) => {
                tracing::warn!("failed to read mcp.json: {e}");
                return HashMap::new();
            }
        };

        // Per-entry deserialization: one malformed server (e.g. a URL-only
        // entry written by the admin UI, whose schema differs) must not drop
        // every other server in the file.
        #[derive(serde::Deserialize)]
        struct McpJsonFile {
            #[serde(default, rename = "mcpServers")]
            mcp_servers: HashMap<String, serde_json::Value>,
        }

        let raw = match serde_json::from_str::<McpJsonFile>(&contents) {
            Ok(file) => file.mcp_servers,
            Err(e) => {
                tracing::warn!("failed to parse mcp.json: {e}");
                return HashMap::new();
            }
        };

        let total = raw.len();
        let mut servers = HashMap::new();
        for (name, value) in raw {
            match serde_json::from_value::<McpServerConfig>(value) {
                Ok(cfg) => {
                    servers.insert(name, cfg);
                }
                Err(e) => {
                    tracing::warn!("mcp.json entry '{name}' is invalid, skipping: {e}");
                }
            }
        }
        info!(
            "loaded {} of {} MCP server(s) from mcp.json",
            servers.len(),
            total
        );
        servers
    }

    /// Merge MCP configs from mcp.json and config.yml. Config.yml entries win on conflict.
    pub fn merged_mcp_config(&self, config: &AppConfig) -> HashMap<String, McpServerConfig> {
        let mut merged = self.load_mcp_json();
        // config.yml wins on conflicts
        for (name, server_config) in &config.mcp {
            merged.insert(name.clone(), server_config.clone());
        }
        merged
    }

    pub fn ensure_dirs(&self) -> Result<()> {
        let dirs = [
            self.config_dir.clone(),
            self.config_dir.join("sessions"),
            self.config_dir.join("credentials"),
            self.config_dir.join("plugins"),
            self.config_dir.join("skills"),
            self.config_dir.join("data"),
        ];

        for dir in &dirs {
            if !dir.exists() {
                std::fs::create_dir_all(dir)?;
            }
        }

        Ok(())
    }

    /// Save the given AppConfig to config.yml in the config directory.
    ///
    /// The file is written with mode `0600` on Unix — `llm.*.api_key` and
    /// `gateway.api_key` live in here, so a umask-default `0644` would leave
    /// provider credentials world-readable.
    pub fn save(&self, config: &AppConfig) -> Result<()> {
        let yaml_path = self.config_dir.join("config.yml");

        let contents = serde_yaml::to_string(config)
            .map_err(|e| Error::Config(format!("failed to serialize config: {e}")))?;

        write_atomic_secret(&yaml_path, contents.as_bytes())?;

        info!("saved updated config to {}", yaml_path.display());
        Ok(())
    }

    /// Liga ou desliga `channels.<key>.enabled`, sem tocar em mais nada.
    ///
    /// Devolve `true` quando o arquivo mudou; `false` quando ja estava assim
    /// (ou quando a secao nao existe e o pedido era `false` — nao se cria uma
    /// secao so para escrever "desligado").
    ///
    /// # Por que isto mora aqui
    ///
    /// Dois processos precisam desta escrita e precisam dela **igual**: a CLI
    /// (`garra whatsapp link` liga, `logout` desliga) e o gateway (que desliga
    /// sozinho quando o servidor mata a sessao — senao todo boot seguinte paga
    /// timeout e retry por uma credencial que nao existe mais). Duas copias
    /// desta funcao divergiriam no dia em que uma delas ganhasse um campo, e o
    /// sintoma seria o pior possivel: a CLI dizendo "desligado" e o gateway
    /// subindo o canal assim mesmo.
    pub fn set_channel_enabled(&self, key: &str, enabled: bool) -> Result<bool> {
        // `save` escreve `<config_dir>/config.yml`; numa maquina que nunca
        // rodou `garra init` o diretorio ainda nao existe.
        self.ensure_dirs()?;
        let mut config = self.load()?;
        match config.channels.get_mut(key) {
            Some(existing) => {
                if existing.enabled == Some(enabled) {
                    return Ok(false);
                }
                existing.enabled = Some(enabled);
            }
            None => {
                if !enabled {
                    return Ok(false);
                }
                config.channels.insert(
                    key.to_string(),
                    ChannelConfig {
                        channel_type: key.to_string(),
                        enabled: Some(true),
                        settings: Default::default(),
                    },
                );
            }
        }
        self.save(&config)?;
        Ok(true)
    }
}

/// Escreve `path` de forma atomica e ja apertada: tmp no **mesmo** diretorio,
/// nascido `0600`, `sync_all`, `rename`.
///
/// # Por que isto substituiu `fs::write` + `harden_secret_file`
///
/// `std::fs::write` e truncate-then-write: entre o truncate e o fim da escrita
/// o `config.yml` esta parcial no disco, e uma queda ali deixa o arquivo vazio
/// — que o `load` acima agora recusa em vez de tratar como "sem config". O
/// `chmod` vinha **depois** da escrita, entao havia ainda uma janela em que
/// `llm.*.api_key` e `gateway.api_key` estavam no disco com o modo do umask
/// (comumente `0644`).
///
/// A janela deixou de ser teorica nesta fatia: alem da CLI (`garra whatsapp
/// link` / `logout`), o gateway passou a chamar `set_channel_enabled` sozinho
/// quando o servidor invalida a sessao — dois processos, read-modify-write, sem
/// lock. O `rename` nao remove a corrida de leitura-modificacao-escrita (o
/// ultimo a escrever ainda vence), mas garante que nenhum leitor jamais veja um
/// arquivo pela metade, e que o arquivo nunca exista com modo frouxo.
///
/// # Por que o nome do temporario e aleatorio, e por que `create_new`
///
/// O nome era deterministico (`.config.yml.tmp`) e o arquivo era aberto com
/// `create(true).truncate(true)`, sem `O_EXCL`. Com **um** escritor isso e
/// inofensivo; com dois — e o paragrafo acima declara dois — os dois calculam
/// o mesmo caminho. A escrita do gateway abre o tmp e comeca a escrever; a da
/// CLI abre o MESMO tmp com `O_TRUNC` no meio; a primeira segue escrevendo do
/// offset antigo, e o que sobra e um arquivo com buraco de NULs e fragmentos
/// dos dois. Os dois `rename`: o `config.yml` resultante ou nao parseia (o
/// gateway nao sobe) ou parseia pela metade e **perde secoes** — exatamente a
/// perda silenciosa de gate que esta funcao existe para impedir.
///
/// E com nome previsivel o preexistente nao precisa ser um arquivo: um symlink
/// plantado no mesmo diretorio seria SEGUIDO por `create(true)`, e o alvo dele
/// e que receberia a config e o `chmod`. `create_new(true)` recusa abrir
/// qualquer coisa que ja exista — symlink inclusive — e o sufixo aleatorio faz
/// o caminho nao ser adivinhavel. As duas juntas, porque cada uma sozinha
/// ainda deixa metade do problema.
///
/// O padrao e o mesmo de `whatsapp_linked::session::write_atomic`, que guarda o
/// blob de sessao, ate no sufixo: sao dois call sites e um padrao, e a
/// alternativa era duas definicoes de "escrita segura" capazes de divergir.
fn write_atomic_secret(path: &Path, bytes: &[u8]) -> Result<()> {
    let dir = path
        .parent()
        .ok_or_else(|| Error::Config(format!("{} nao tem diretorio pai", path.display())))?;

    let nonce = u64::from_ne_bytes(garraia_security::random_bytes::<8>().map_err(|_| {
        Error::Config("RNG do sistema indisponivel para nomear o temporario da config".into())
    })?);
    let tmp = dir.join(format!(
        ".{}.{nonce:016x}.tmp",
        path.file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "config".into())
    ));

    // Qualquer falha daqui para baixo deixaria a config inteira num temporario
    // orfao — 0600, entao nao e exposicao, mas e uma segunda copia que ninguem
    // espera e que o proximo `save` nao reaproveita, porque o nome mudou. Por
    // isso o desfecho de erro apaga o `tmp` antes de propagar, e nao so no
    // braco do `rename`, que era o unico coberto antes.
    let escrito = escreve_tmp(&tmp, bytes)
        // Roda em TODA plataforma, e nao so fora de Unix como o comentario
        // anterior dizia. Em Unix o `mode(0o600)` do `open` ja garante que o
        // arquivo nunca existiu mais frouxo que isso — o `open(2)` aplica
        // `mode & ~umask`, e umask so tira bit. O que sobra para esta chamada
        // fazer la e o caso patologico do umask que tira tambem os bits do
        // dono: `0000` e restritivo demais, e o proprio processo nao reabriria
        // o arquivo.
        .and_then(|()| harden_secret_file(&tmp))
        .and_then(|()| {
            std::fs::rename(&tmp, path).map_err(|e| {
                Error::Config(format!(
                    "failed to replace {} atomically: {e}",
                    path.display()
                ))
            })
        });
    if let Err(e) = escrito {
        let _ = std::fs::remove_file(&tmp);
        return Err(e);
    }

    // Best-effort: sem isto o rename pode nao estar duravel depois de uma queda
    // de energia. Falha aqui nao invalida a escrita.
    if let Ok(d) = std::fs::File::open(dir) {
        let _ = d.sync_all();
    }
    Ok(())
}

/// O temporario do [`write_atomic_secret`]: criado do zero, 0600 desde o
/// `open`, escrito e sincronizado.
fn escreve_tmp(tmp: &Path, bytes: &[u8]) -> Result<()> {
    use std::io::Write;

    let mut opts = std::fs::OpenOptions::new();
    // `create_new`: se o caminho ja existe — arquivo ou symlink — isto falha
    // em vez de escrever por cima. Ver o docstring de [`write_atomic_secret`].
    opts.write(true).create_new(true);
    // Nasce 0600: apertar depois da escrita deixaria uma janela com o
    // segredo ja no disco sob o modo do umask.
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(0o600);
    }
    let mut f = opts
        .open(tmp)
        .map_err(|e| Error::Config(format!("failed to open {} for write: {e}", tmp.display())))?;
    f.write_all(bytes)
        .map_err(|e| Error::Config(format!("failed to write {}: {e}", tmp.display())))?;
    f.sync_all()
        .map_err(|e| Error::Config(format!("failed to fsync {}: {e}", tmp.display())))?;
    Ok(())
}

/// Restrict `path` to owner-only read/write (`0600`) on Unix.
///
/// Call this after writing any file that can hold a credential — `config.yml`
/// carries `llm.*.api_key` since the onboarding wizard stopped defaulting to
/// the credential vault, and `std::fs::write` alone leaves the file at whatever
/// the process umask allows (commonly `0644`).
///
/// No-op on non-Unix targets, where the parent directory ACL governs access.
///
/// A politica em si mora em [`garraia_common::fs_perms`] desde o
/// `whatsapp_linked`: `garraia-channels` precisa das mesmas regras (0600 em
/// arquivo, 0700 em diretorio) e nao depende desta crate. Esta funcao continua
/// existindo porque dezenas de call sites ja a usam e porque ela mapeia o erro
/// para [`Error::Config`]; o que ela nao faz mais e ter uma segunda definicao
/// de "modo seguro" capaz de divergir da primeira.
pub fn harden_secret_file(path: &Path) -> Result<()> {
    garraia_common::fs_perms::harden_secret_file(path).map_err(|e| {
        Error::Config(format!(
            "failed to restrict permissions on {}: {e}",
            path.display()
        ))
    })
}

#[cfg(test)]
mod tests {
    use super::ConfigLoader;
    use std::fs;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_dir(label: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock should be after unix epoch")
            .as_nanos();
        std::env::temp_dir().join(format!(
            "garraia-config-test-{}-{}-{}",
            label,
            std::process::id(),
            nanos
        ))
    }

    /// `config.yml` holds `llm.*.api_key` since the wizard stopped defaulting
    /// to the credential vault. A umask-default `0644` would make provider
    /// credentials world-readable, so `save` must clamp the mode to `0600`.
    #[cfg(unix)]
    #[test]
    fn save_writes_config_with_owner_only_permissions() {
        use std::os::unix::fs::PermissionsExt;

        let dir = temp_dir("save-perms");
        fs::create_dir_all(&dir).expect("failed to create temp dir");

        // Pre-create the file world-readable so we prove `save` tightens an
        // existing loose mode, not merely that a fresh file happens to be 0600.
        let path = dir.join("config.yml");
        fs::write(&path, "gateway:\n  host: \"127.0.0.1\"\n").expect("failed to seed config");
        fs::set_permissions(&path, fs::Permissions::from_mode(0o644))
            .expect("failed to loosen permissions");

        let loader = ConfigLoader::with_dir(&dir);
        loader
            .save(&crate::model::AppConfig::default())
            .expect("save should succeed");

        let mode = fs::metadata(&path)
            .expect("config should exist after save")
            .permissions()
            .mode();
        assert_eq!(
            mode & 0o777,
            0o600,
            "config.yml must be owner-only; got {:o}",
            mode & 0o777
        );

        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn load_returns_default_when_no_config_exists() {
        let dir = temp_dir("default");
        fs::create_dir_all(&dir).expect("failed to create temp dir");

        let loader = ConfigLoader::with_dir(&dir);
        let config = loader.load().expect("load should succeed");

        assert_eq!(config.gateway.host, "127.0.0.1");
        assert_eq!(config.gateway.port, 3888);
        assert!(config.channels.is_empty());

        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn load_prefers_yaml_over_toml_when_both_exist() {
        let dir = temp_dir("yaml-precedence");
        fs::create_dir_all(&dir).expect("failed to create temp dir");

        fs::write(
            dir.join("config.yml"),
            "gateway:\n  host: \"0.0.0.0\"\n  port: 4001\n",
        )
        .expect("failed to write yaml config");
        fs::write(
            dir.join("config.toml"),
            "[gateway]\nhost = \"127.0.0.2\"\nport = 4999\n",
        )
        .expect("failed to write toml config");

        let loader = ConfigLoader::with_dir(&dir);
        let config = loader.load().expect("load should succeed");

        assert_eq!(config.gateway.host, "0.0.0.0");
        assert_eq!(config.gateway.port, 4001);

        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn load_reads_toml_when_yaml_missing() {
        let dir = temp_dir("toml");
        fs::create_dir_all(&dir).expect("failed to create temp dir");

        fs::write(
            dir.join("config.toml"),
            "[gateway]\nhost = \"127.0.0.2\"\nport = 4002\n",
        )
        .expect("failed to write toml config");

        let loader = ConfigLoader::with_dir(&dir);
        let config = loader.load().expect("load should succeed");

        assert_eq!(config.gateway.host, "127.0.0.2");
        assert_eq!(config.gateway.port, 4002);

        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn load_mcp_json_returns_empty_when_file_missing() {
        let dir = temp_dir("mcp-missing");
        fs::create_dir_all(&dir).expect("failed to create temp dir");

        let loader = ConfigLoader::with_dir(&dir);
        let mcp = loader.load_mcp_json();
        assert!(mcp.is_empty());

        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn merged_mcp_config_prefers_config_yml_on_conflict() {
        let dir = temp_dir("mcp-merge");
        fs::create_dir_all(&dir).expect("failed to create temp dir");

        // Write an mcp.json with one server
        let mcp_json = r#"{
            "mcpServers": {
                "server1": {
                    "command": "from-json",
                    "transport": "stdio"
                },
                "server2": {
                    "command": "only-in-json",
                    "transport": "stdio"
                }
            }
        }"#;
        fs::write(dir.join("mcp.json"), mcp_json).expect("failed to write mcp.json");

        let loader = ConfigLoader::with_dir(&dir);

        // Build an AppConfig with server1 overridden via config.yml mcp section
        let mut config = crate::model::AppConfig::default();
        config.mcp.insert(
            "server1".to_string(),
            crate::model::McpServerConfig {
                command: "from-config-yml".to_string(),
                args: vec![],
                env: std::collections::HashMap::new(),
                transport: "stdio".to_string(),
                url: None,
                enabled: None,
                timeout: None,
                allowed_tools: vec![],
                memory_limit_mb: None,
                max_restarts: None,
                restart_delay_secs: None,
                inherit_env: false,
            },
        );

        let merged = loader.merged_mcp_config(&config);

        // server1 should come from config.yml (wins)
        assert_eq!(merged.get("server1").unwrap().command, "from-config-yml");
        // server2 should still come from mcp.json
        assert_eq!(merged.get("server2").unwrap().command, "only-in-json");

        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn load_mcp_json_skips_invalid_entries_and_keeps_valid_ones() {
        let dir = temp_dir("mcp-tolerant");
        fs::create_dir_all(&dir).expect("failed to create temp dir");

        // "broken" lacks the required `command` (e.g. a URL-only entry
        // written by the gateway admin UI). It must not drop "filesystem".
        let mcp_json = r#"{
            "mcpServers": {
                "filesystem": {
                    "command": "npx",
                    "args": ["-y", "@modelcontextprotocol/server-filesystem", "/root"]
                },
                "broken": {
                    "url": "http://localhost:9999/mcp"
                }
            }
        }"#;
        fs::write(dir.join("mcp.json"), mcp_json).expect("failed to write mcp.json");

        let loader = ConfigLoader::with_dir(&dir);
        let mcp = loader.load_mcp_json();

        assert_eq!(mcp.len(), 1);
        assert_eq!(mcp.get("filesystem").unwrap().command, "npx");
        assert!(!mcp.contains_key("broken"));

        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn load_mcp_json_malformed_top_level_returns_empty_without_panic() {
        let dir = temp_dir("mcp-malformed");
        fs::create_dir_all(&dir).expect("failed to create temp dir");
        fs::write(dir.join("mcp.json"), "{ not json").expect("failed to write mcp.json");

        let loader = ConfigLoader::with_dir(&dir);
        assert!(loader.load_mcp_json().is_empty());

        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn ensure_dirs_creates_expected_subdirectories() {
        let dir = temp_dir("ensure-dirs");
        let loader = ConfigLoader::with_dir(&dir);

        loader.ensure_dirs().expect("ensure_dirs should succeed");

        assert!(dir.exists());
        assert!(dir.join("sessions").exists());
        assert!(dir.join("credentials").exists());
        assert!(dir.join("plugins").exists());
        assert!(dir.join("skills").exists());
        assert!(dir.join("data").exists());

        let _ = fs::remove_dir_all(dir);
    }

    /// **A prova de que a escrita e por rename, e nao truncate-then-write.**
    ///
    /// `std::fs::write` reaproveita o arquivo existente: mesmo inode, conteudo
    /// substituido no lugar — e entre o truncate e o fim da escrita o
    /// `config.yml` esta parcial no disco. Um `rename` atomico **troca** o
    /// inode. E a unica diferenca observavel das duas implementacoes sem uma
    /// corrida, e ela morre no instante em que alguem voltar ao `fs::write`.
    #[cfg(unix)]
    #[test]
    fn save_substitui_o_arquivo_por_rename_em_vez_de_truncar() {
        use std::os::unix::fs::MetadataExt;

        let dir = temp_dir("save-atomica");
        fs::create_dir_all(&dir).expect("failed to create temp dir");
        let loader = ConfigLoader::with_dir(&dir);
        let config = loader.load().expect("defaults");

        loader.save(&config).expect("primeira escrita");
        let path = dir.join("config.yml");
        let antes = fs::metadata(&path).expect("metadata").ino();

        loader.save(&config).expect("segunda escrita");
        let depois = fs::metadata(&path).expect("metadata").ino();

        assert_ne!(
            antes, depois,
            "o destino tem de ser substituido por rename; mesmo inode significa \
             truncate-then-write, que deixa o arquivo parcial no disco durante a escrita"
        );
        assert!(
            fs::read_dir(&dir)
                .expect("lista")
                .filter_map(|e| e.ok())
                .all(|e| !e.file_name().to_string_lossy().ends_with(".tmp")),
            "nenhum tmp orfao pode sobrar — seria uma segunda copia da config inteira"
        );

        let _ = fs::remove_dir_all(dir);
    }

    /// **Config vazia nao e "sem config".**
    ///
    /// `serde_yaml` desserializa uma string em branco para `AppConfig::default()`
    /// **com sucesso** — e essa era a forma exata que uma escrita interrompida
    /// deixava o arquivo. O gate de credencial do gateway desaparecia em
    /// silencio, e o `save()` seguinte gravava os defaults por cima do que
    /// restava da config do usuario.
    #[test]
    fn load_recusa_config_yml_em_branco() {
        let dir = temp_dir("load-vazia");
        fs::create_dir_all(&dir).expect("failed to create temp dir");
        let path = dir.join("config.yml");

        // A premissa que torna o bug possivel — se ela cair, este teste perde o
        // sentido e quem mexer precisa saber disso.
        assert!(
            serde_yaml::from_str::<crate::model::AppConfig>("   \n").is_ok(),
            "premissa: YAML em branco desserializa com sucesso para os defaults"
        );

        // O ultimo caso e o que `trim().is_empty()` deixava passar: um arquivo
        // que sobrou so com comentarios tem bytes, cai no MESMO
        // `AppConfig::default()` e faz a MESMA perda silenciosa de gate. E
        // criterio de YAML, nao de espaco em branco, que fecha os dois.
        for conteudo in ["", "   ", "\n\n", "  \n\t\n", "# so um comentario\n"] {
            fs::write(&path, conteudo).expect("seed");
            let erro = ConfigLoader::with_dir(&dir)
                .load()
                .expect_err("config sem conteudo tem de ser recusada");
            assert!(
                format!("{erro}").contains("vazio"),
                "o erro tem de dizer o que fazer: {erro}"
            );
        }

        let _ = fs::remove_dir_all(dir);
    }

    /// **O temporario nao pode ter nome que outro escritor saiba adivinhar.**
    ///
    /// Esta fatia declara dois escritores do mesmo `config.yml` — a CLI
    /// (`garra whatsapp link`/`logout`) e o gateway (`desligar_na_config()` →
    /// `set_channel_enabled` → `save`). Com o nome deterministico
    /// (`.config.yml.tmp`) e `create(true).truncate(true)` os dois abriam o
    /// MESMO arquivo, e o resultado era um tmp com fragmentos dos dois — que
    /// os dois entao renomeavam por cima da config.
    ///
    /// O teste planta um arquivo exatamente no nome antigo e exige que ele
    /// sobreviva intacto: com a construcao anterior ele seria truncado no
    /// `open`, entao a mutacao "voltar ao nome fixo" sai vermelha aqui.
    #[test]
    fn o_temporario_nao_usa_o_nome_deterministico_de_antes() {
        let dir = temp_dir("save-tmp-unico");
        fs::create_dir_all(&dir).expect("failed to create temp dir");
        let loader = ConfigLoader::with_dir(&dir);
        let config = loader.load().expect("defaults");

        let antigo = dir.join(".config.yml.tmp");
        fs::write(&antigo, b"escrita de outro processo").expect("planta o tmp antigo");

        loader
            .save(&config)
            .expect("a escrita nao pode depender do tmp antigo");

        assert_eq!(
            fs::read(&antigo).expect("o tmp plantado tem de continuar la"),
            b"escrita de outro processo",
            "o nome do temporario nao pode ser adivinhavel: este arquivo e o que \
             o outro escritor estaria no meio de escrever"
        );
        assert!(
            dir.join("config.yml").is_file(),
            "e a config tem de ter sido gravada assim mesmo"
        );

        let _ = fs::remove_dir_all(dir);
    }

    /// **E nem symlink plantado no caminho do temporario e seguido.**
    ///
    /// Nome previsivel mais `create(true)` seguia um symlink: o alvo dele e que
    /// receberia a config inteira e o `chmod 0600`. `create_new` recusa abrir
    /// qualquer coisa que ja exista. Este teste cobre o caso que o anterior
    /// nao cobre — la o plantado e um arquivo comum, e truncar um arquivo
    /// comum estraga o do vizinho; aqui o plantado redireciona a escrita para
    /// fora do diretorio.
    #[cfg(unix)]
    #[test]
    fn o_temporario_nao_segue_symlink_plantado_no_nome_antigo() {
        let dir = temp_dir("save-tmp-symlink");
        fs::create_dir_all(&dir).expect("failed to create temp dir");
        let vitima = dir.join("vitima.txt");
        fs::write(&vitima, b"conteudo da vitima").expect("vitima");
        std::os::unix::fs::symlink(&vitima, dir.join(".config.yml.tmp")).expect("symlink");

        let loader = ConfigLoader::with_dir(&dir);
        let config = loader.load().expect("defaults");
        loader.save(&config).expect("salva");

        assert_eq!(
            fs::read(&vitima).expect("vitima"),
            b"conteudo da vitima",
            "a escrita nao pode ter atravessado o symlink"
        );

        let _ = fs::remove_dir_all(dir);
    }

    /// E uma config de verdade continua carregando — a recusa acima nao pode
    /// ter virado "recusa tudo".
    #[test]
    fn load_aceita_config_yml_com_conteudo() {
        let dir = temp_dir("load-ok");
        fs::create_dir_all(&dir).expect("failed to create temp dir");
        fs::write(dir.join("config.yml"), "gateway:\n  host: \"127.0.0.1\"\n").expect("seed");
        assert!(ConfigLoader::with_dir(&dir).load().is_ok());
        let _ = fs::remove_dir_all(dir);
    }
}
