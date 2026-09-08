use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Context, Result, bail};

const GITHUB_REPO: &str = "michelbr84/GarraRUST";
const RELEASES_API: &str = "https://api.github.com/repos/michelbr84/GarraRUST/releases/latest";
const UPDATE_CHECK_FILE: &str = "update-check.json";
const CHECK_TTL_SECS: u64 = 86400; // 24 hours

/// Cached update check result.
#[derive(serde::Serialize, serde::Deserialize)]
struct UpdateCheck {
    latest_version: String,
    release_notes: String,
    checked_at: u64,
}

/// GitHub release API response (only the fields we need).
#[derive(serde::Deserialize)]
struct GitHubRelease {
    tag_name: String,
    body: Option<String>,
    assets: Vec<GitHubAsset>,
}

#[derive(serde::Deserialize)]
struct GitHubAsset {
    name: String,
    browser_download_url: String,
}

fn current_version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

fn cache_path() -> PathBuf {
    garraia_config::ConfigLoader::default_config_dir().join(UPDATE_CHECK_FILE)
}

/// Return the asset name for the current platform.
fn platform_asset_name() -> Result<&'static str> {
    asset_name_for(std::env::consts::OS, std::env::consts::ARCH)
}

/// Map an (os, arch) pair onto the release asset name. These names are the
/// compatibility surface of every release (CLAUDE.md, regra 15): they never
/// change, and every entry needs a `<asset>.sha256` sibling published with it.
/// `garraia-windows-aarch64.exe` exists from v0.3.4 onward; a native ARM64
/// binary asking for it against an older release fails with "no asset for
/// this platform", which is accurate — those releases only served ARM64 via
/// the x86_64 binary under emulation. `garraia-android-aarch64` (bionic,
/// Termux) exists from v0.3.6 onward (ADR 0016).
fn asset_name_for(os: &str, arch: &str) -> Result<&'static str> {
    match (os, arch) {
        ("macos", "aarch64") => Ok("garraia-macos-aarch64"),
        ("macos", "x86_64") => Ok("garraia-macos-x86_64"),
        ("linux", "x86_64") => Ok("garraia-linux-x86_64"),
        ("linux", "aarch64") => Ok("garraia-linux-aarch64"),
        ("windows", "x86_64") => Ok("garraia-windows-x86_64.exe"),
        ("windows", "aarch64") => Ok("garraia-windows-aarch64.exe"),
        ("android", "aarch64") => Ok("garraia-android-aarch64"),
        (os, arch) => bail!("unsupported platform: {os}/{arch}"),
    }
}

/// Strip leading 'v' from a version tag.
fn strip_v(tag: &str) -> &str {
    tag.strip_prefix('v').unwrap_or(tag)
}

/// Fetch the latest release info from GitHub.
async fn fetch_latest_release(client: &reqwest::Client) -> Result<GitHubRelease> {
    let resp = client
        .get(RELEASES_API)
        .header("user-agent", format!("garraia/{}", current_version()))
        .header("accept", "application/vnd.github+json")
        .send()
        .await
        .context("failed to reach GitHub releases API")?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        bail!("GitHub API returned {status}: {body}");
    }

    resp.json::<GitHubRelease>()
        .await
        .context("failed to parse GitHub release response")
}

/// Run `garraia update`. Returns Ok(true) if an update was applied.
pub async fn run_update(yes: bool) -> Result<bool> {
    let client = reqwest::Client::new();
    let release = fetch_latest_release(&client).await?;

    let latest = strip_v(&release.tag_name);
    let current = current_version();

    if latest == current {
        println!("Already up to date (v{current}).");
        save_check_cache(latest, release.body.as_deref().unwrap_or(""));
        return Ok(false);
    }

    println!("Current version:   v{current}");
    println!("Latest version:    v{latest}");
    println!();

    // Show release notes (truncated)
    if let Some(notes) = &release.body {
        let preview: String = notes.lines().take(20).collect::<Vec<_>>().join("\n");
        println!("Release notes:");
        println!("{preview}");
        if notes.lines().count() > 20 {
            println!(
                "  ... (truncated, see https://github.com/{GITHUB_REPO}/releases/tag/{})",
                release.tag_name
            );
        }
        println!();
    }

    // Confirm unless --yes
    if !yes {
        let confirm = dialoguer::Confirm::new()
            .with_prompt(format!("Update to v{latest}?"))
            .default(true)
            .interact()
            .context("failed to read confirmation")?;

        if !confirm {
            println!("Update cancelled.");
            return Ok(false);
        }
    }

    // Find the right asset
    let asset_name = platform_asset_name()?;
    let checksum_name = format!("{asset_name}.sha256");

    let asset = release
        .assets
        .iter()
        .find(|a| a.name == asset_name)
        .context(format!(
            "release has no asset for this platform ({asset_name})"
        ))?;

    let checksum_asset = release
        .assets
        .iter()
        .find(|a| a.name == checksum_name)
        .context(format!(
            "release is missing checksum file ({checksum_name}). Update aborted for safety."
        ))?;

    // Download binary
    println!("Downloading {asset_name}...");
    let binary_bytes = client
        .get(&asset.browser_download_url)
        .header("user-agent", format!("garraia/{current}"))
        .send()
        .await
        .context("failed to download binary")?
        .bytes()
        .await
        .context("failed to read binary bytes")?;

    // Verify checksum
    print!("Verifying checksum...");
    let cs_text = client
        .get(&checksum_asset.browser_download_url)
        .header("user-agent", format!("garraia/{current}"))
        .send()
        .await
        .context("failed to download checksum")?
        .text()
        .await
        .context("failed to read checksum")?;

    let expected_hash = cs_text
        .split_whitespace()
        .next()
        .context("invalid checksum file format")?
        .to_lowercase();

    use std::fmt::Write;
    let digest = sha256_digest(&binary_bytes);
    let mut actual_hash = String::with_capacity(64);
    for byte in &digest {
        write!(&mut actual_hash, "{byte:02x}").unwrap();
    }

    if actual_hash != expected_hash {
        bail!(
            "checksum mismatch!\n  expected: {expected_hash}\n  got:      {actual_hash}\n\nDownload may be corrupted. Update aborted."
        );
    }
    println!(" ok");

    // Locate current binary
    let current_exe =
        std::env::current_exe().context("cannot determine current executable path")?;
    let backup_path = current_exe.with_extension("old");

    // Write new binary to temp file
    let temp_path = current_exe.with_extension("new");
    fs::write(&temp_path, &binary_bytes).context("failed to write new binary")?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&temp_path, fs::Permissions::from_mode(0o755))
            .context("failed to set executable permissions")?;

        // Backup current binary
        if current_exe.exists() {
            fs::copy(&current_exe, &backup_path).context("failed to backup current binary")?;
            println!("Backed up current binary to {}", backup_path.display());
        }

        // Atomic replace
        fs::rename(&temp_path, &current_exe).context("failed to replace binary")?;
    }

    #[cfg(windows)]
    {
        // Windows replacement strategy:
        // 1. Remove old backup if exists (rename target must not exist)
        if backup_path.exists() {
            let _ = fs::remove_file(&backup_path);
        }

        // 2. Rename current to backup (this works even if running)
        if current_exe.exists() {
            fs::rename(&current_exe, &backup_path)
                .context("failed to move current binary to backup")?;
            println!("Backed up current binary to {}", backup_path.display());
        }

        // 3. Rename new to current
        if let Err(e) = fs::rename(&temp_path, &current_exe) {
            // Rollback attempt if rename fails
            let _ = fs::rename(&backup_path, &current_exe);
            return Err(e).context("failed to replace binary");
        }
    }

    // Update cache
    save_check_cache(latest, release.body.as_deref().unwrap_or(""));

    println!();
    println!("Updated to v{latest}.");
    println!("Restart the daemon to apply: garraia restart");

    // #1030: o update so troca ESTE binario. Um `/usr/bin/garraia` de outra
    // instalacao segue no PATH, intocado e sem aviso — e e ele que scripts,
    // cron e terminais com PATH diferente passam a rodar.
    if let Some(report) = report_other_binaries(&current_exe, latest).await {
        println!();
        println!("{report}");
    }

    Ok(true)
}

/// Run `garraia update --check-binaries`: so a varredura do PATH, sem rede.
pub async fn run_check_binaries() -> Result<()> {
    let current_exe =
        std::env::current_exe().context("cannot determine current executable path")?;
    match report_other_binaries(&current_exe, current_version()).await {
        Some(report) => println!("{report}"),
        None => println!(
            "Nenhum outro binario {} no PATH alem de {}.",
            BINARY_NAMES.join("/"),
            current_exe.display()
        ),
    }
    Ok(())
}

/// Nomes com que o GarraIA aparece no PATH: o binario e o alias curto.
const BINARY_NAMES: &[&str] = &["garraia", "garra"];

/// Onde os pacotes de sistema instalam, mesmo que o PATH deste shell nao os
/// liste (o do cron e do systemd lista).
#[cfg(unix)]
const SYSTEM_BIN_DIRS: &[&str] = &["/usr/local/bin", "/usr/bin"];
#[cfg(not(unix))]
const SYSTEM_BIN_DIRS: &[&str] = &[];

/// Um binario do GarraIA no PATH que nao e o que acabou de ser atualizado.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OtherBinary {
    pub path: PathBuf,
    /// `true` quando o diretorio dele vem ANTES do diretorio do atualizado
    /// no PATH: e ele, nao o atualizado, que `garraia` resolve neste shell.
    pub precedes: bool,
}

/// Procura outros `garraia`/`garra` nos diretorios do PATH e nos de sistema,
/// excluindo o proprio `current` (por caminho canonico, entao um symlink
/// `garra -> garraia` ao lado do atualizado nao e "outro").
///
/// O PATH entra como lista em vez de ser lido aqui, para o teste montar o
/// cenario inteiro num diretorio temporario.
pub fn find_other_binaries(
    path_dirs: &[PathBuf],
    system_dirs: &[PathBuf],
    current: &Path,
) -> Vec<OtherBinary> {
    let canonical = |p: &Path| fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf());
    let current_canon = canonical(current);
    let current_dir = current_canon.parent().map(Path::to_path_buf);
    let current_dir_index = path_dirs
        .iter()
        .position(|d| current_dir.as_deref() == Some(canonical(d).as_path()));

    let names: Vec<String> = BINARY_NAMES
        .iter()
        .map(|n| format!("{n}{}", std::env::consts::EXE_SUFFIX))
        .collect();

    let mut seen: Vec<PathBuf> = vec![current_canon.clone()];
    let mut found = Vec::new();
    let candidates = path_dirs
        .iter()
        .enumerate()
        .map(|(i, d)| (d, Some(i)))
        .chain(system_dirs.iter().map(|d| (d, None)));
    for (dir, index) in candidates {
        for name in &names {
            let candidate = dir.join(name);
            if !candidate.is_file() {
                continue;
            }
            let canon = canonical(&candidate);
            if seen.contains(&canon) {
                continue;
            }
            seen.push(canon);
            let precedes = match (index, current_dir_index) {
                (Some(i), Some(cur)) => i < cur,
                // Fora do PATH deste shell, ou o atualizado fora do PATH:
                // nao ha "antes"; e so um outro binario.
                _ => false,
            };
            found.push(OtherBinary {
                path: candidate,
                precedes,
            });
        }
    }
    found
}

/// A versao que um binario achado no PATH diz ter. Roda `--version` com
/// timeout: e um executavel que o usuario ja tem no PATH com o nome do
/// GarraIA, e um binario quebrado nao pode travar o update.
async fn probe_version(path: &Path) -> Option<String> {
    let out = tokio::time::timeout(
        Duration::from_secs(3),
        tokio::process::Command::new(path).arg("--version").output(),
    )
    .await
    .ok()?
    .ok()?;
    parse_version_output(&String::from_utf8_lossy(&out.stdout))
}

/// `garraia 0.3.4` (ou `garraia v0.3.4`) → `0.3.4`. So a primeira linha.
fn parse_version_output(stdout: &str) -> Option<String> {
    let first = stdout.lines().find(|l| !l.trim().is_empty())?;
    let last = first.split_whitespace().last()?;
    let v = strip_v(last);
    (!v.is_empty() && v.chars().next().is_some_and(|c| c.is_ascii_digit())).then(|| v.to_string())
}

/// O aviso do #1030, pronto para imprimir. `None` quando nao ha o que dizer:
/// nenhum outro binario, ou so copias da mesma versao (inofensivas).
pub fn shadow_report(
    current: &Path,
    current_version: &str,
    others: &[(OtherBinary, Option<String>)],
) -> Option<String> {
    let mut lines = Vec::new();
    for (other, version) in others {
        if version.as_deref() == Some(current_version) {
            continue;
        }
        let ver = version.as_deref().unwrap_or("versao desconhecida");
        if other.precedes {
            lines.push(format!(
                "⚠ Binario antigo tem precedencia no PATH: {} ({ver}) vem ANTES de {} ({current_version}) — `garraia` continua rodando a versao antiga.",
                other.path.display(),
                current.display()
            ));
        } else {
            lines.push(format!(
                "⚠ Binario antigo detectado: {} ({ver}) sombreado por {} ({current_version}).",
                other.path.display(),
                current.display()
            ));
        }
        lines.push(format!("  Remova com: {}", remove_hint(&other.path)));
    }
    (!lines.is_empty()).then(|| lines.join("\n"))
}

fn remove_hint(path: &Path) -> String {
    if cfg!(windows) {
        return format!("Remove-Item \"{}\"", path.display());
    }
    let needs_root = path.starts_with("/usr") || path.starts_with("/opt");
    format!(
        "{}rm {}",
        if needs_root { "sudo " } else { "" },
        path.display()
    )
}

/// PATH real + diretorios de sistema, sondando a versao de cada achado.
async fn report_other_binaries(current: &Path, current_version: &str) -> Option<String> {
    let path_dirs: Vec<PathBuf> = std::env::var_os("PATH")
        .map(|p| std::env::split_paths(&p).collect())
        .unwrap_or_default();
    let system_dirs: Vec<PathBuf> = SYSTEM_BIN_DIRS.iter().map(PathBuf::from).collect();
    let others = find_other_binaries(&path_dirs, &system_dirs, current);
    if others.is_empty() {
        return None;
    }
    let mut probed = Vec::with_capacity(others.len());
    for other in others {
        let version = probe_version(&other.path).await;
        probed.push((other, version));
    }
    shadow_report(current, current_version, &probed)
}

/// Run `garraia rollback`.
pub fn run_rollback() -> Result<()> {
    let current_exe =
        std::env::current_exe().context("cannot determine current executable path")?;
    let backup_path = current_exe.with_extension("old");

    if !backup_path.exists() {
        bail!(
            "no backup found at {}. Nothing to roll back to.",
            backup_path.display()
        );
    }

    fs::rename(&backup_path, &current_exe).context("failed to restore backup")?;

    println!("Rolled back to previous version.");
    println!("Restart the daemon to apply: garraia restart");

    Ok(())
}

/// Check for updates (non-blocking, cached). Returns a message if an update
/// is available, or None.
pub fn check_for_update_notice() -> Option<String> {
    if std::env::var("GARRAIA_NO_UPDATE_CHECK").is_ok() {
        return None;
    }

    let cache = read_check_cache()?;
    let current = current_version();
    let latest = strip_v(&cache.latest_version);

    if latest != current {
        Some(format!(
            "Update available: v{current} -> v{latest}  —  run `garraia update`"
        ))
    } else {
        None
    }
}

/// Spawn a background version check that updates the cache file.
/// Does not block. Failures are silently ignored.
pub fn spawn_background_check() {
    if std::env::var("GARRAIA_NO_UPDATE_CHECK").is_ok() {
        return;
    }

    // Skip if cache is fresh
    if let Some(cache) = read_check_cache() {
        let now = epoch_secs();
        if now.saturating_sub(cache.checked_at) < CHECK_TTL_SECS {
            return;
        }
    }

    tokio::spawn(async {
        let client = reqwest::Client::new();
        if let Ok(release) = fetch_latest_release(&client).await {
            let version = strip_v(&release.tag_name);
            save_check_cache(version, release.body.as_deref().unwrap_or(""));
        }
    });
}

fn read_check_cache() -> Option<UpdateCheck> {
    let path = cache_path();
    let contents = fs::read_to_string(&path).ok()?;
    serde_json::from_str(&contents).ok()
}

fn save_check_cache(version: &str, notes: &str) {
    let check = UpdateCheck {
        latest_version: version.to_string(),
        release_notes: notes.to_string(),
        checked_at: epoch_secs(),
    };
    if let Ok(json) = serde_json::to_string_pretty(&check) {
        let _ = fs::write(cache_path(), json);
    }
}

fn epoch_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

/// Simple SHA-256 using ring.
fn sha256_digest(data: &[u8]) -> [u8; 32] {
    use ring::digest;
    let d = digest::digest(&digest::SHA256, data);
    let mut out = [0u8; 32];
    out.copy_from_slice(d.as_ref());
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn touch_exe(path: &Path) {
        fs::write(path, b"#!/bin/sh\nexit 0\n").expect("write fake binary");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(path, fs::Permissions::from_mode(0o755)).expect("chmod");
        }
    }

    /// #1030: o update trocava so o proprio binario e nada dizia sobre o
    /// `/usr/bin/garraia` de outra instalacao. A varredura acha os outros,
    /// pula o proprio (e symlinks para ele) e diz quem vem antes no PATH.
    #[test]
    fn find_other_binaries_skips_itself_and_orders_by_path() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let local = tmp.path().join("local-bin");
        let system = tmp.path().join("usr-bin");
        let other = tmp.path().join("other-bin");
        for d in [&local, &system, &other] {
            fs::create_dir_all(d).expect("mkdir");
        }
        let exe = format!("garraia{}", std::env::consts::EXE_SUFFIX);
        let current = local.join(&exe);
        touch_exe(&current);
        touch_exe(&system.join(&exe));
        touch_exe(&other.join(format!("garra{}", std::env::consts::EXE_SUFFIX)));
        #[cfg(unix)]
        std::os::unix::fs::symlink(&current, local.join("garra")).expect("symlink");

        // PATH: other-bin vem ANTES de local-bin; usr-bin so pela lista de sistema.
        let found = find_other_binaries(
            &[other.clone(), local.clone()],
            std::slice::from_ref(&system),
            &current,
        );

        let paths: Vec<&Path> = found.iter().map(|o| o.path.as_path()).collect();
        assert!(
            !paths
                .iter()
                .any(|p| *p == current || p.ends_with("local-bin/garra")),
            "o proprio (e o symlink para ele) nao sao 'outros': {paths:?}"
        );
        let garra = found
            .iter()
            .find(|o| o.path.starts_with(&other))
            .expect("garra em other-bin");
        assert!(garra.precedes, "other-bin vem antes de local-bin no PATH");
        let sys = found
            .iter()
            .find(|o| o.path.starts_with(&system))
            .expect("garraia em usr-bin");
        assert!(!sys.precedes, "fora do PATH deste shell nao ha 'antes'");
        assert_eq!(found.len(), 2, "{paths:?}");
    }

    #[test]
    fn parse_version_output_reads_clap_style_lines() {
        assert_eq!(
            parse_version_output("garraia 0.3.4\n").as_deref(),
            Some("0.3.4")
        );
        assert_eq!(
            parse_version_output("\ngarra v0.4.0").as_deref(),
            Some("0.4.0")
        );
        assert_eq!(parse_version_output("not a version"), None);
        assert_eq!(parse_version_output(""), None);
    }

    #[test]
    fn shadow_report_names_precedence_and_skips_same_version() {
        let current = Path::new("/home/u/.local/bin/garraia");
        let old = OtherBinary {
            path: PathBuf::from("/usr/bin/garraia"),
            precedes: false,
        };
        let first = OtherBinary {
            path: PathBuf::from("/opt/garra/garra"),
            precedes: true,
        };
        let same = OtherBinary {
            path: PathBuf::from("/tmp/dup/garraia"),
            precedes: true,
        };

        let report = shadow_report(
            current,
            "0.3.9",
            &[
                (old.clone(), Some("0.3.4".into())),
                (first.clone(), None),
                (same, Some("0.3.9".into())),
            ],
        )
        .expect("ha o que avisar");

        assert!(
            report.contains(
                "/usr/bin/garraia (0.3.4) sombreado por /home/u/.local/bin/garraia (0.3.9)"
            ),
            "{report}"
        );
        assert!(report.contains("sudo rm /usr/bin/garraia"), "{report}");
        assert!(
            report.contains("/opt/garra/garra (versao desconhecida) vem ANTES de"),
            "{report}"
        );
        assert!(
            !report.contains("/tmp/dup"),
            "mesma versao nao e problema: {report}"
        );

        // So copias da mesma versao: nada a dizer.
        assert!(shadow_report(current, "0.3.9", &[(old, Some("0.3.9".into()))]).is_none());
    }

    // Regra 15 do CLAUDE.md: estes nomes são superfície de compatibilidade.
    // Renomear qualquer um quebra `garra update` de toda instalação em campo.
    #[test]
    fn asset_names_are_the_published_compatibility_surface() {
        let expected = [
            (("linux", "x86_64"), "garraia-linux-x86_64"),
            (("linux", "aarch64"), "garraia-linux-aarch64"),
            (("macos", "x86_64"), "garraia-macos-x86_64"),
            (("macos", "aarch64"), "garraia-macos-aarch64"),
            (("windows", "x86_64"), "garraia-windows-x86_64.exe"),
            (("windows", "aarch64"), "garraia-windows-aarch64.exe"),
            // bionic/Termux desde a v0.3.6 (ADR 0016).
            (("android", "aarch64"), "garraia-android-aarch64"),
        ];
        for ((os, arch), name) in expected {
            assert_eq!(asset_name_for(os, arch).unwrap(), name, "{os}/{arch}");
        }
    }

    #[test]
    fn unsupported_platforms_fail_instead_of_guessing() {
        for (os, arch) in [
            ("freebsd", "x86_64"),
            ("linux", "riscv64"),
            ("windows", "x86"),
        ] {
            let err = asset_name_for(os, arch).unwrap_err();
            assert!(
                err.to_string().contains("unsupported platform"),
                "{os}/{arch}: {err}"
            );
        }
    }

    // O download só prossegue quando existe o irmão `<asset>.sha256` com esse
    // nome exato na release — o formato derivado aqui é o contrato que o
    // release.yml cumpre ao gerar um `.sha256` por asset.
    #[test]
    fn checksum_sibling_name_appends_sha256_to_the_full_asset_name() {
        let asset = asset_name_for("windows", "aarch64").unwrap();
        assert_eq!(
            format!("{asset}.sha256"),
            "garraia-windows-aarch64.exe.sha256"
        );
    }

    #[test]
    fn current_platform_resolves_to_a_known_asset() {
        // Em qualquer alvo em que a suíte compile e rode hoje, o updater deve
        // resolver um nome — se um dia um alvo novo de CI cair no bail!, este
        // teste aponta direto para o mapa em asset_name_for.
        let name = platform_asset_name().unwrap();
        assert!(name.starts_with("garraia-"));
    }
}
