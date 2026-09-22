use std::path::{Path, PathBuf};
use std::time::Duration;

use notify::{EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use tokio::sync::watch;
use tracing::{info, warn};

use crate::model::AppConfig;

const DEBOUNCE_MS: u64 = 500;

/// Watches a config file and broadcasts new `AppConfig` values via a
/// `tokio::sync::watch` channel whenever the file changes on disk.
pub struct ConfigWatcher {
    // Hold the watcher to keep it alive; dropping it stops watching.
    _watcher: RecommendedWatcher,
}

impl ConfigWatcher {
    /// Start watching `config_path`. Returns a receiver that yields the latest
    /// config whenever the file changes. The initial value is `initial_config`.
    pub fn start(
        config_path: PathBuf,
        initial_config: AppConfig,
    ) -> Result<(Self, watch::Receiver<AppConfig>), notify::Error> {
        let (tx, rx) = watch::channel(initial_config);

        // We watch the parent directory because editors often write to a temp
        // file and rename, which doesn't trigger events on the file itself.
        let watch_dir = config_path.parent().unwrap_or(Path::new(".")).to_path_buf();
        let target_filename = config_path.file_name().unwrap_or_default().to_os_string();

        let (notify_tx, mut notify_rx) = tokio::sync::mpsc::channel::<()>(8);

        let mut watcher =
            notify::recommended_watcher(move |event: notify::Result<notify::Event>| {
                if let Ok(event) = event {
                    let dominated =
                        matches!(event.kind, EventKind::Create(_) | EventKind::Modify(_));
                    if dominated {
                        // Only fire if this event touches our config file
                        let touches_config = event
                            .paths
                            .iter()
                            .any(|p| p.file_name().map(|f| f == target_filename).unwrap_or(false));
                        if touches_config {
                            let _ = notify_tx.try_send(());
                        }
                    }
                }
            })?;

        watcher.watch(&watch_dir, RecursiveMode::NonRecursive)?;

        // Debounced reload loop
        let cfg_path = config_path.clone();
        tokio::spawn(async move {
            loop {
                // Wait for any filesystem event
                if notify_rx.recv().await.is_none() {
                    break; // channel closed
                }
                // Debounce: drain further events within the window
                tokio::time::sleep(Duration::from_millis(DEBOUNCE_MS)).await;
                while notify_rx.try_recv().is_ok() {}

                // Re-read the config
                let recarregada = {
                    let anterior = tx.borrow();
                    recarregar(&cfg_path, &anterior)
                };
                match recarregada {
                    Ok(new_config) => {
                        info!("config reloaded from {}", cfg_path.display());
                        let _ = tx.send(new_config);
                    }
                    Err(e) => {
                        warn!("config reload failed (keeping previous config): {e}");
                    }
                }
            }
        });

        info!("watching config file: {}", config_path.display());
        Ok((Self { _watcher: watcher }, rx))
    }
}

/// Leva para a config recarregada o que veio da env no boot, e nao do arquivo.
///
/// #1261: `gateway.api_key_env` (`GARRAIA_GATEWAY_API_KEY`) e `serde(skip)`
/// e so o `ConfigLoader::load` a preenche. O reload desserializa o arquivo
/// cru, entao sem isto a primeira edicao do `config.yml` apagava a credencial
/// de env do `current_config()`: o cookie de sessao perdia o `Secure` e o
/// alerta do admin passava a dizer que nao ha chave, com o gate (montado no
/// boot) ainda ligado. A env do processo nao muda depois do boot, entao
/// copiar o valor anterior e o mesmo que reler a env — sem tocar no ambiente.
fn recarregar(path: &Path, anterior: &AppConfig) -> Result<AppConfig, String> {
    let mut nova = reload_config(path)?;
    nova.gateway.api_key_env = anterior.gateway.api_key_env.clone();
    Ok(nova)
}

fn reload_config(path: &Path) -> Result<AppConfig, String> {
    let contents = std::fs::read_to_string(path).map_err(|e| format!("read error: {e}"))?;

    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
    match ext {
        "yml" | "yaml" => {
            serde_yaml::from_str(&contents).map_err(|e| format!("YAML parse error: {e}"))
        }
        "toml" => toml::from_str(&contents).map_err(|e| format!("TOML parse error: {e}")),
        other => Err(format!("unsupported config extension: {other}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use secrecy::ExposeSecret;

    #[test]
    fn reload_preserva_a_credencial_de_env() {
        let mut anterior = AppConfig::default();
        anterior.gateway.api_key_env = crate::auth::gateway_api_key_de(Some("k-env-1261".into()));

        let dir = std::env::temp_dir().join(format!("garraia-watcher-1261-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("mkdir");
        let path = dir.join("config.yml");
        std::fs::write(&path, "gateway:\n  port: 3888\n").expect("write");

        let nova = recarregar(&path, &anterior).expect("reload");
        let _ = std::fs::remove_dir_all(&dir);

        assert_eq!(nova.gateway.port, 3888, "o resto vem do arquivo");
        assert_eq!(
            nova.gateway.api_key_env.as_ref().map(|s| s.expose_secret()),
            Some("k-env-1261"),
            "o reload apagou a credencial de env"
        );
        assert!(nova.gateway.api_key_configurada());
    }
}
