//! `garraia doctor` — saúde da instalação numa passada.
//!
//! Fase 0 do Garra Mobile (ADR 0016): o onboarding no Termux é
//! `curl … install.sh | bash` → `garraia doctor` → `garraia chat`. O doctor
//! precisa então sobreviver a config inexistente **e** a config
//! não-parseável, reportando sysexits como `config check` — por isso é
//! interceptado em `main()` antes do `load()` global.
//!
//! Seções:
//!   1. Plataforma — versão, SO/arquitetura, detecção Termux, dir de config.
//!   2. Diretórios — `ensure_dirs` (cria o que falta, valida escrita).
//!   3. Config — reuso direto de [`garraia_config::run_check`].
//!   4. Providers — para cada entrada `llm:`: fonte da credencial
//!      (presence-only) e, para os daemons locais keyless (`ollama`,
//!      `llamacpp`), probe TCP com alvo vetado por `garraia_common::ssrf`
//!      ([`IpScope::AllowPrivate`] — link-local, CGNAT, multicast e
//!      unspecified continuam bloqueados; regra 14).
//!   5. Daemon — reconciliação pidfile + porta (mesma fonte do `status`),
//!      não-fatal quando parado.
//!
//! Exit codes (sysexits, padrão do `config check`):
//!   - `0` — OK (avisos permitidos fora de `--strict`).
//!   - `2` — erros de validação da config (ou avisos sob `--strict`).
//!   - `65` — `EX_DATAERR`, arquivo existe mas não parseia.
//!
//! Probes de provider/daemon **não** afetam o exit code — são diagnóstico,
//! não validação. Invariante de redaction mantida: credenciais aparecem só
//! como fonte (`config.yml`, `credential vault`, `environment variable`),
//! nunca como valor.

use std::net::{TcpStream, ToSocketAddrs};
use std::time::Duration;

use anyhow::Result;
use garraia_common::ssrf::{IpScope, SsrfCategory, UrlPolicy, vet_url};
use garraia_config::{
    AppConfig, ConfigCheck, ConfigLoader, provider_key_env, resolve_provider_key_source,
};
use serde::Serialize;

/// Budget de cada probe TCP. Mesmo valor que o boot loop usa para os
/// daemons locais — 2s é generoso para loopback/LAN.
const PROBE_TIMEOUT: Duration = Duration::from_secs(2);

/// Política do probe de daemon local: plaintext é a escolha legítima do
/// operador (llama-server/ollama servem HTTP cru na LAN), escopo
/// AllowPrivate porque o alvo **é** suposto ser local — o que o guard ainda
/// bloqueia (link-local = metadata de cloud, CGNAT, multicast, unspecified)
/// continua bloqueado.
fn daemon_probe_policy() -> UrlPolicy {
    UrlPolicy {
        allowed_schemes: &["http", "https"],
        host_allowlist: None,
        ip_scope: IpScope::AllowPrivate,
        timeout: PROBE_TIMEOUT,
        user_agent: concat!("garraia-doctor/", env!("CARGO_PKG_VERSION")),
    }
}

/// Resultado do probe de um daemon local.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub(crate) enum ProbeResult {
    /// Pelo menos um endereço vetado aceitou conexão.
    Reachable,
    /// Resolveu e vetou OK, mas nada aceitou conexão.
    Unreachable { reason: String },
    /// O guard SSRF recusou o alvo — a URL nunca foi tocada.
    Blocked { category: &'static str },
}

fn category_label(category: SsrfCategory) -> &'static str {
    match category {
        SsrfCategory::BadRequest => "bad_request",
        SsrfCategory::Forbidden => "forbidden",
        SsrfCategory::Upstream => "upstream",
    }
}

/// Vetar a URL e conectar TCP aos endereços aprovados, em ordem.
///
/// `vet_url` resolve o host uma vez e devolve os endereços pinados —
/// conectar neles fecha a janela TOCTOU do DNS. Probe é diagnóstico: nenhum
/// erro aqui é fatal, tudo vira [`ProbeResult`].
fn probe_local_daemon(base_url: &str, timeout: Duration) -> ProbeResult {
    let vetted = match vet_url(base_url, &daemon_probe_policy()) {
        Ok(v) => v,
        Err(rejection) => {
            return ProbeResult::Blocked {
                category: category_label(rejection.category()),
            };
        }
    };

    let mut last_err = "nenhum endereço após o veto".to_string();
    for addr in &vetted.addrs {
        match TcpStream::connect_timeout(addr, timeout) {
            Ok(_) => return ProbeResult::Reachable,
            Err(e) => last_err = e.to_string(),
        }
    }
    ProbeResult::Unreachable { reason: last_err }
}

/// Termux detectado? Mesma heurística do `install.sh` (branch Android):
/// `$TERMUX_VERSION` exportado por todo shell do Termux, ou `$PREFIX`
/// apontando para o sandbox `com.termux`.
fn detect_termux(termux_version: Option<&str>, prefix: Option<&str>) -> bool {
    termux_version.is_some_and(|v| !v.is_empty())
        || prefix.is_some_and(|p| p.contains("com.termux"))
}

fn termux_detected() -> bool {
    detect_termux(
        std::env::var("TERMUX_VERSION").ok().as_deref(),
        std::env::var("PREFIX").ok().as_deref(),
    )
}

/// Um item do bloco Termux: o que foi observado e, quando não-OK, o passo
/// seguinte. Espelha a forma `next_step` de `/api/diagnostics`.
#[derive(Debug, Serialize)]
pub(crate) struct TermuxItem {
    pub label: &'static str,
    pub ok: bool,
    pub detail: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_step: Option<&'static str>,
}

/// Bloco de diagnóstico só emitido dentro do Termux. Puramente informativo:
/// nada aqui altera o exit code, pela mesma razão que os probes de provider
/// não alteram — diagnóstico não é veredito.
#[derive(Debug, Serialize)]
pub(crate) struct TermuxCheck {
    pub items: Vec<TermuxItem>,
}

/// Entradas de ambiente do bloco Termux, agrupadas para manter
/// `collect_termux_check` puro (e portanto afirmável em qualquer runner).
pub(crate) struct TermuxEnv<'a> {
    pub prefix: Option<&'a str>,
    pub path: Option<&'a str>,
    pub ld_preload: Option<&'a str>,
    pub ssl_cert_file: Option<&'a str>,
    /// `true` quando a config tem um caminho que fala TLS com Postgres.
    pub uses_postgres: bool,
}

/// Caminho do shim termux-exec, relativo a `$PREFIX`.
const TERMUX_EXEC_LIB: &str = "lib/libtermux-exec.so";

/// Monta o bloco Termux. `exists` é parâmetro para o teste não depender do
/// filesystem do runner.
pub(crate) fn collect_termux_check<F>(env: &TermuxEnv<'_>, exists: F) -> TermuxCheck
where
    F: Fn(&std::path::Path) -> bool,
{
    let mut items = Vec::new();

    // Trust store. Está aqui porque a suposição contrária custou caro numa
    // issue: o `garra` fala HTTPS por reqwest com `rustls-tls`, cujos roots
    // são os do webpki COMPILADOS no binário — não há leitura de trust store
    // do sistema, e portanto `SSL_CERT_FILE` não afeta esse tráfego.
    items.push(TermuxItem {
        label: "Trust store TLS",
        ok: true,
        detail: "webpki-roots embutidos no binário (SSL_CERT_FILE não afeta HTTPS do garra)"
            .to_string(),
        next_step: None,
    });

    // A exceção: o driver de Postgres usa native-roots, que LÊ o trust store
    // do sistema. Só vira aviso quando há Postgres configurado.
    if env.uses_postgres {
        let set = env.ssl_cert_file.is_some_and(|v| !v.is_empty());
        items.push(TermuxItem {
            label: "SSL_CERT_FILE (Postgres)",
            ok: set,
            detail: if set {
                "definido — TLS de Postgres usa os CAs do Termux".to_string()
            } else {
                "não definido — só o TLS de Postgres depende disso".to_string()
            },
            next_step: (!set).then_some(
                "export SSL_CERT_FILE=$PREFIX/etc/tls/cert.pem (pkg install ca-certificates)",
            ),
        });
    }

    // termux-exec: sem ele, todo filho MCP que é script npm/pip morre no
    // shebang `/usr/bin/env`, e hosts MCP externos não conseguem nem exec.
    let shim = env
        .prefix
        .map(|p| std::path::Path::new(p).join(TERMUX_EXEC_LIB));
    let shim_ok = shim.as_deref().is_some_and(&exists);
    items.push(TermuxItem {
        label: "termux-exec",
        ok: shim_ok,
        detail: match (&shim, shim_ok) {
            (Some(p), true) => format!("{} presente", p.display()),
            (Some(p), false) => format!("{} ausente", p.display()),
            (None, _) => "$PREFIX não definido".to_string(),
        },
        next_step: (!shim_ok).then_some(
            "pkg install termux-exec (corrige shebangs de npm/pip e o exec de servidores MCP)",
        ),
    });

    // LD_PRELOAD é informativo: o gateway injeta o shim nos filhos MCP
    // sozinho, então a ausência aqui não é falha.
    let preload = env.ld_preload.filter(|v| !v.is_empty());
    items.push(TermuxItem {
        label: "LD_PRELOAD",
        ok: true,
        detail: match preload {
            Some(v) => format!("herdado: {v}"),
            None => "não definido — o gateway injeta o shim nos filhos MCP".to_string(),
        },
        next_step: None,
    });

    // Wrapper de MCP. Escrito pelo install.sh; um host externo aponta para
    // ele porque o exec com env filtrado falha sem o preload.
    let wrapper = env
        .prefix
        .map(|p| std::path::Path::new(p).join("bin/garra-mcp-server"));
    let wrapper_ok = wrapper.as_deref().is_some_and(&exists);
    items.push(TermuxItem {
        label: "Wrapper MCP",
        ok: wrapper_ok,
        detail: match (&wrapper, wrapper_ok) {
            (Some(p), true) => format!("{} instalado", p.display()),
            (Some(p), false) => format!("{} ausente", p.display()),
            (None, _) => "$PREFIX não definido".to_string(),
        },
        next_step: (!wrapper_ok)
            .then_some("reinstale via install.sh para obter $PREFIX/bin/garra-mcp-server"),
    });

    // $PREFIX/bin no PATH — onde o install.sh põe o binário.
    let bin_dir = env.prefix.map(|p| format!("{p}/bin"));
    let on_path = match (&bin_dir, env.path) {
        (Some(dir), Some(path)) => path.split(':').any(|e| e == dir),
        _ => false,
    };
    items.push(TermuxItem {
        label: "$PREFIX/bin no PATH",
        ok: on_path,
        detail: match &bin_dir {
            Some(dir) if on_path => format!("{dir} presente no PATH"),
            Some(dir) => format!("{dir} fora do PATH"),
            None => "$PREFIX não definido".to_string(),
        },
        next_step: (!on_path).then_some("export PATH=\"$PREFIX/bin:$PATH\" no seu ~/.profile"),
    });

    TermuxCheck { items }
}

/// Base URL com que um daemon local deve ser sondado, ou `None` para
/// providers que não são daemon local. Defaults espelham os arms de boot do
/// gateway (`garraia-gateway/src/bootstrap/mod.rs`).
fn resolve_daemon_base_url(kind: &str, base_url: Option<&str>) -> Option<String> {
    match kind {
        "ollama" => Some(base_url.unwrap_or("http://localhost:11434").to_string()),
        "llamacpp" => Some(base_url.unwrap_or("http://localhost:8080").to_string()),
        _ => None,
    }
}

/// Diagnóstico de um provider configurado.
#[derive(Debug, Serialize)]
pub(crate) struct ProviderCheck {
    /// Chave da entrada em `llm:`.
    pub name: String,
    /// Tipo declarado (`provider:`).
    pub kind: String,
    pub model: Option<String>,
    /// `true` para os daemons locais keyless (`ollama`, `llamacpp`).
    pub keyless: bool,
    /// Fonte da credencial, presence-only — nunca o valor. `None` quando
    /// keyless.
    pub credential_source: Option<String>,
    /// Probe de reachability — só para daemons locais keyless.
    pub probe: Option<ProbeResult>,
    /// URL efetivamente sondada (com o default aplicado).
    pub probe_url: Option<String>,
}

/// Diagnóstico do daemon do gateway (mesma fonte de verdade do `status`).
#[derive(Debug, Serialize)]
pub(crate) struct DaemonCheck {
    pub pid_file_present: bool,
    pub pid: Option<u32>,
    pub process_alive: bool,
    /// pidfile existe mas o PID não está vivo.
    pub stale_pid_file: bool,
    pub gateway_host: String,
    pub gateway_port: u16,
    pub port_listening: bool,
}

/// Relatório completo do doctor — é o payload `report` do `--json`.
#[derive(Debug, Serialize)]
pub(crate) struct DoctorReport {
    pub version: &'static str,
    pub os: &'static str,
    pub arch: &'static str,
    pub termux: bool,
    pub config_dir: String,
    pub dirs_ok: bool,
    /// Presente quando a config existe mas não parseia (exit 65).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub config_error: Option<String>,
    /// Presente quando a config carregou — é o `ConfigCheck` do `config check`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub config_check: Option<ConfigCheck>,
    pub providers: Vec<ProviderCheck>,
    pub daemon: DaemonCheck,
    /// Bloco Termux — só presente dentro do Termux (issues #909/#911/#913).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub termux_check: Option<TermuxCheck>,
}

/// Um provider é keyless-local (e portanto sondável) quando o tipo é um dos
/// daemons locais; o resto é classificado pela tabela de credenciais.
fn classify_provider_kind(kind: &str) -> KeylessOrCredential {
    match provider_key_env(kind) {
        Some(_) => KeylessOrCredential::NeedsCredential,
        None if kind == "ollama" || kind == "llamacpp" => KeylessOrCredential::KeylessLocal,
        // `echo` é dev-only e tipos desconhecidos caem aqui; o `config check`
        // já reporta o desconhecido como Error — o doctor só rotula.
        None => KeylessOrCredential::NeedsCredential,
    }
}

enum KeylessOrCredential {
    KeylessLocal,
    NeedsCredential,
}

/// Ordem determinística (chave ordenada) porque `config.llm` é map não-ordenado
/// e a saída humana/JSON do doctor é afirmável em teste.
fn collect_provider_checks(config: &AppConfig) -> Vec<ProviderCheck> {
    let mut names: Vec<&str> = config.llm.keys().map(|k| k.as_str()).collect();
    names.sort_unstable();

    names
        .into_iter()
        .map(|name| {
            let entry = &config.llm[name];
            let kind = entry.provider.as_str();
            let model = entry.model.clone().filter(|m| !m.is_empty());

            let (keyless, credential_source) = match classify_provider_kind(kind) {
                KeylessOrCredential::KeylessLocal => (true, None),
                KeylessOrCredential::NeedsCredential => (
                    false,
                    Some(
                        resolve_provider_key_source(kind, entry.api_key.as_deref())
                            .label()
                            .to_string(),
                    ),
                ),
            };

            let (probe, probe_url) = match resolve_daemon_base_url(kind, entry.base_url.as_deref())
            {
                Some(url) => (Some(probe_local_daemon(&url, PROBE_TIMEOUT)), Some(url)),
                None => (None, None),
            };

            ProviderCheck {
                name: name.to_string(),
                kind: kind.to_string(),
                model,
                keyless,
                credential_source,
                probe,
                probe_url,
            }
        })
        .collect()
}

/// Probe TCP curto para a porta do gateway. Host vem da config do operador
/// (mesmo nível de confiança do `garraia status`, que faz reqwest direto) —
/// falha aqui é diagnóstico, nunca pânico.
fn tcp_reachable(host: &str, port: u16, timeout: Duration) -> bool {
    match (host, port).to_socket_addrs() {
        Ok(mut addrs) => addrs.any(|addr| TcpStream::connect_timeout(&addr, timeout).is_ok()),
        Err(_) => false,
    }
}

fn check_daemon(gateway_host: &str, gateway_port: u16) -> DaemonCheck {
    let pid = crate::read_pid();
    let pid_file_present = pid.is_some();
    let process_alive = pid.map(crate::is_process_running).unwrap_or(false);

    DaemonCheck {
        pid_file_present,
        pid,
        process_alive,
        stale_pid_file: pid_file_present && !process_alive,
        gateway_host: gateway_host.to_string(),
        gateway_port,
        port_listening: tcp_reachable(gateway_host, gateway_port, PROBE_TIMEOUT),
    }
}

/// Exit code do doctor. Falha de parse domina (65); depois os findings da
/// config (2, ou sob `--strict`); probes e daemon são diagnóstico e não
/// afetam o código.
fn compute_doctor_exit_code(report: &DoctorReport, strict: bool) -> i32 {
    if report.config_error.is_some() {
        return 65;
    }
    if let Some(check) = &report.config_check
        && (check.has_errors() || (strict && check.has_warnings()))
    {
        return 2;
    }
    0
}

/// Ponto de entrada do `garraia doctor`. Retorna o exit code (sysexits).
pub fn run_doctor(json: bool, strict: bool) -> Result<i32> {
    let loader = ConfigLoader::new()?;
    let dirs_ok = loader.ensure_dirs().is_ok();

    let (config_check, config_error, providers, gateway) = match loader.load() {
        Ok(config) => {
            let check = garraia_config::run_check(&loader, &config);
            let providers = collect_provider_checks(&config);
            let gateway = (config.gateway.host.clone(), config.gateway.port);
            (Some(check), None, providers, gateway)
        }
        Err(e) => {
            // Mesma truncagem do `config check` (SEC-L-01): o erro nunca
            // imprime o arquivo inteiro.
            (
                None,
                Some(crate::config_cmd::truncate_error(format!("{e}"))),
                Vec::new(),
                (
                    AppConfig::default().gateway.host,
                    AppConfig::default().gateway.port,
                ),
            )
        }
    };

    // Ambiente do bloco Termux. Lido uma vez aqui para manter
    // `collect_termux_check` puro. `uses_postgres` é presence-only — o valor
    // da URL (que carrega credencial) nunca sai daqui.
    let prefix_env = std::env::var("PREFIX").ok();
    let path_env = std::env::var("PATH").ok();
    let ld_preload_env = std::env::var("LD_PRELOAD").ok();
    let ssl_cert_env = std::env::var("SSL_CERT_FILE").ok();
    let uses_postgres = [
        "GARRAIA_LOGIN_DATABASE_URL",
        "GARRAIA_SIGNUP_DATABASE_URL",
        "GARRAIA_APP_DATABASE_URL",
    ]
    .iter()
    .any(|k| std::env::var(k).is_ok_and(|v| !v.is_empty()));

    let report = DoctorReport {
        version: env!("CARGO_PKG_VERSION"),
        os: std::env::consts::OS,
        arch: std::env::consts::ARCH,
        termux: termux_detected(),
        config_dir: loader.config_dir().display().to_string(),
        dirs_ok,
        config_error,
        config_check,
        providers,
        daemon: check_daemon(&gateway.0, gateway.1),
        termux_check: termux_detected().then(|| {
            collect_termux_check(
                &TermuxEnv {
                    prefix: prefix_env.as_deref(),
                    path: path_env.as_deref(),
                    ld_preload: ld_preload_env.as_deref(),
                    ssl_cert_file: ssl_cert_env.as_deref(),
                    uses_postgres,
                },
                |path| path.exists(),
            )
        }),
    };

    let exit_code = compute_doctor_exit_code(&report, strict);

    if json {
        let payload = serde_json::json!({
            "ok": exit_code == 0,
            "exit_code": exit_code,
            "report": report,
        });
        println!("{}", serde_json::to_string_pretty(&payload)?);
    } else {
        print_human(&report, strict);
    }

    Ok(exit_code)
}

fn print_human(report: &DoctorReport, strict: bool) {
    println!("🩺 GarraIA doctor v{}", report.version);
    println!(
        "  Plataforma     : {}/{}{}",
        report.os,
        report.arch,
        if report.termux { " (Termux)" } else { "" }
    );
    println!("  Config         : {}", report.config_dir);
    println!();

    println!(
        "  [1/4] Diretórios ......... {}",
        ok_or(report.dirs_ok, "FALHA — sem escrita")
    );

    match (&report.config_error, &report.config_check) {
        (Some(err), _) => {
            println!("  [2/4] Configuração ....... ERRO (exit 65)");
            println!("        {err}");
            println!("        dica: o arquivo existe mas não parseia; corrija o YAML/TOML.");
        }
        (None, Some(check)) => {
            let errors = check
                .findings
                .iter()
                .filter(|f| f.severity == garraia_config::Severity::Error)
                .count();
            let warnings = check.findings.len() - errors;
            println!(
                "  [2/4] Configuração ....... {}",
                ok_or(errors == 0 && (warnings == 0 || !strict), "PROBLEMAS")
            );
            for finding in &check.findings {
                let label = match finding.severity {
                    garraia_config::Severity::Error => "erro",
                    garraia_config::Severity::Warning => "aviso",
                };
                println!("        [{label}] {}: {}", finding.field, finding.message);
            }
        }
        (None, None) => unreachable!("load falhou sem erro nem check"),
    }

    println!("  [3/4] Providers ({}) :", report.providers.len());
    if report.providers.is_empty() {
        println!("        (nenhum — rode `garraia init` ou `garraia config set-model`)");
    }
    for p in &report.providers {
        let mut line = format!("        • {} ({})", p.name, p.kind);
        if let Some(model) = &p.model {
            line.push_str(&format!(", modelo {model}"));
        }
        if p.keyless {
            line.push_str(" — keyless");
        }
        println!("{line}");
        if let Some(source) = &p.credential_source {
            println!("          credencial: {source}");
        }
        match (&p.probe, &p.probe_url) {
            (Some(ProbeResult::Reachable), Some(url)) => {
                println!("          probe {url} → acessível");
            }
            (Some(ProbeResult::Unreachable { reason }), Some(url)) => {
                println!("          probe {url} → inacessível ({reason})");
            }
            (Some(ProbeResult::Blocked { category }), Some(url)) => {
                println!("          probe {url} → bloqueado pelo guard SSRF ({category})");
            }
            _ => {}
        }
    }

    let daemon = &report.daemon;
    let daemon_status = if daemon.process_alive && daemon.port_listening {
        format!(
            "OK — PID {} vivo, porta {} respondendo",
            daemon.pid.unwrap_or_default(),
            daemon.gateway_port
        )
    } else if daemon.process_alive {
        format!(
            "parcial — PID {} vivo mas a porta {} não responde (iniciando? host/porta da config diferentes?)",
            daemon.pid.unwrap_or_default(),
            daemon.gateway_port
        )
    } else if daemon.stale_pid_file {
        format!(
            "parado — pidfile obsoleto (PID {} não está vivo)",
            daemon.pid.unwrap_or_default()
        )
    } else {
        "parado — sem pidfile (`garraia start` sobe o daemon)".to_string()
    };
    println!("  [4/4] Daemon ............. {daemon_status}");

    if let Some(termux) = &report.termux_check {
        println!();
        println!("  Termux ..................");
        for item in &termux.items {
            println!(
                "        {} {} — {}",
                if item.ok { "OK  " } else { "aviso" },
                item.label,
                item.detail
            );
            if let Some(step) = item.next_step {
                println!("          → {step}");
            }
        }
    }

    let code = compute_doctor_exit_code(report, strict);
    println!();
    println!(
        "  Resultado: {}",
        match code {
            0 => "OK".to_string(),
            65 => "config não-parseável (exit 65)".to_string(),
            _ => "PROBLEMAS (exit 2)".to_string(),
        }
    );
}

fn ok_or(ok: bool, fail_label: &str) -> String {
    if ok {
        "OK".to_string()
    } else {
        fail_label.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use garraia_config::LlmProviderConfig;
    use std::collections::HashMap;

    fn llm_entry(provider: &str, model: Option<&str>, base_url: Option<&str>) -> LlmProviderConfig {
        LlmProviderConfig {
            provider: provider.to_string(),
            model: model.map(String::from),
            api_key: None,
            base_url: base_url.map(String::from),
            extra: HashMap::new(),
        }
    }

    fn config_with(entries: &[(&str, LlmProviderConfig)]) -> AppConfig {
        let mut cfg = AppConfig::default();
        for (k, v) in entries {
            cfg.llm.insert((*k).to_string(), v.clone());
        }
        cfg
    }

    // ─── collect_termux_check ──────────────────────────────────────────────

    const TERMUX_PREFIX: &str = "/data/data/com.termux/files/usr";

    fn termux_env() -> TermuxEnv<'static> {
        TermuxEnv {
            prefix: Some(TERMUX_PREFIX),
            path: Some("/data/data/com.termux/files/usr/bin:/system/bin"),
            ld_preload: None,
            ssl_cert_file: None,
            uses_postgres: false,
        }
    }

    fn item<'a>(check: &'a TermuxCheck, label: &str) -> &'a TermuxItem {
        check
            .items
            .iter()
            .find(|i| i.label == label)
            .unwrap_or_else(|| panic!("item {label} ausente"))
    }

    /// O bloco existe para desfazer uma suposição que já custou caro numa
    /// issue: o HTTPS do `garra` não lê trust store do sistema.
    #[test]
    fn termux_check_states_the_trust_store_is_embedded() {
        let check = collect_termux_check(&termux_env(), |_| true);
        let trust = item(&check, "Trust store TLS");
        assert!(trust.ok);
        assert!(trust.detail.contains("webpki-roots"));
        assert!(trust.detail.contains("SSL_CERT_FILE"));
        assert!(trust.next_step.is_none());
    }

    /// `SSL_CERT_FILE` só é cobrado quando há Postgres: é o único caminho
    /// (sqlx, native-roots) que de fato lê o trust store do sistema.
    #[test]
    fn termux_check_only_asks_for_ssl_cert_file_when_postgres_is_configured() {
        let check = collect_termux_check(&termux_env(), |_| true);
        assert!(
            !check
                .items
                .iter()
                .any(|i| i.label.starts_with("SSL_CERT_FILE")),
            "sem Postgres o item não deve aparecer"
        );

        let env = TermuxEnv {
            uses_postgres: true,
            ..termux_env()
        };
        let check = collect_termux_check(&env, |_| true);
        let ssl = item(&check, "SSL_CERT_FILE (Postgres)");
        assert!(!ssl.ok);
        assert!(ssl.next_step.is_some_and(|s| s.contains("cert.pem")));

        let env = TermuxEnv {
            uses_postgres: true,
            ssl_cert_file: Some("/etc/tls/cert.pem"),
            ..termux_env()
        };
        let check = collect_termux_check(&env, |_| true);
        assert!(item(&check, "SSL_CERT_FILE (Postgres)").ok);
    }

    /// termux-exec ausente é acionável — é o que quebra shebangs de npm/pip
    /// e o exec de servidores MCP.
    #[test]
    fn termux_check_flags_a_missing_termux_exec_shim() {
        let check = collect_termux_check(&termux_env(), |_| false);
        let shim = item(&check, "termux-exec");
        assert!(!shim.ok);
        assert!(shim.detail.contains("libtermux-exec.so"));
        assert!(
            shim.next_step
                .is_some_and(|s| s.contains("pkg install termux-exec"))
        );

        let check = collect_termux_check(&termux_env(), |_| true);
        assert!(item(&check, "termux-exec").ok);
    }

    /// `LD_PRELOAD` ausente NÃO é falha: o gateway injeta o shim nos filhos
    /// MCP por conta própria.
    #[test]
    fn termux_check_reports_ld_preload_without_calling_it_a_failure() {
        let check = collect_termux_check(&termux_env(), |_| true);
        let preload = item(&check, "LD_PRELOAD");
        assert!(preload.ok);
        assert!(preload.next_step.is_none());

        let env = TermuxEnv {
            ld_preload: Some("/lib/x.so"),
            ..termux_env()
        };
        let check = collect_termux_check(&env, |_| true);
        assert!(item(&check, "LD_PRELOAD").detail.contains("/lib/x.so"));
    }

    #[test]
    fn termux_check_flags_a_missing_mcp_wrapper() {
        let wrapper = format!("{TERMUX_PREFIX}/bin/garra-mcp-server");
        let check = collect_termux_check(&termux_env(), |p| p.display().to_string() == wrapper);
        assert!(item(&check, "Wrapper MCP").ok);

        let check = collect_termux_check(&termux_env(), |_| false);
        let w = item(&check, "Wrapper MCP");
        assert!(!w.ok);
        assert!(w.next_step.is_some_and(|s| s.contains("install.sh")));
    }

    /// Match exato de componente do PATH: um prefixo como
    /// `/data/data/com.termux/files/usr/bin-old` não pode contar como acerto.
    #[test]
    fn termux_check_matches_path_entries_exactly() {
        let check = collect_termux_check(&termux_env(), |_| true);
        assert!(item(&check, "$PREFIX/bin no PATH").ok);

        for path in [
            "/system/bin:/usr/bin",
            "/data/data/com.termux/files/usr/bin-old",
            "/prefix/data/data/com.termux/files/usr/bin",
        ] {
            let env = TermuxEnv {
                path: Some(path),
                ..termux_env()
            };
            let check = collect_termux_check(&env, |_| true);
            let p = item(&check, "$PREFIX/bin no PATH");
            assert!(!p.ok, "PATH {path:?} não deveria contar como acerto");
            assert!(p.next_step.is_some());
        }
    }

    /// Sem `$PREFIX` nada explode — o doctor precisa sobreviver a ambiente
    /// degradado, como já sobrevive a config ausente.
    #[test]
    fn termux_check_survives_a_missing_prefix() {
        let env = TermuxEnv {
            prefix: None,
            path: None,
            ..termux_env()
        };
        let check = collect_termux_check(&env, |_| true);
        assert_eq!(check.items.len(), 5);
        for label in ["termux-exec", "Wrapper MCP", "$PREFIX/bin no PATH"] {
            assert!(!item(&check, label).ok);
        }
    }

    // ─── detect_termux ─────────────────────────────────────────────────────

    #[test]
    fn termux_detection_matches_the_installer_heuristic() {
        assert!(detect_termux(Some("0.119.0"), None));
        assert!(detect_termux(None, Some("/data/data/com.termux/files/usr")));
        assert!(detect_termux(Some("0.119.0"), Some("/usr")));
        // String vazia não conta como presente (paridade com install.sh).
        assert!(!detect_termux(Some(""), Some("/usr")));
        assert!(!detect_termux(None, None));
        assert!(!detect_termux(None, Some("/home/me/.local")));
    }

    // ─── resolve_daemon_base_url ───────────────────────────────────────────

    #[test]
    fn daemon_base_url_defaults_mirror_the_gateway_boot_arms() {
        assert_eq!(
            resolve_daemon_base_url("ollama", None).as_deref(),
            Some("http://localhost:11434")
        );
        assert_eq!(
            resolve_daemon_base_url("llamacpp", None).as_deref(),
            Some("http://localhost:8080")
        );
        assert_eq!(
            resolve_daemon_base_url("llamacpp", Some("http://pc:9090")).as_deref(),
            Some("http://pc:9090")
        );
        assert_eq!(
            resolve_daemon_base_url("openrouter", Some("http://x")),
            None
        );
    }

    // ─── probe_local_daemon ────────────────────────────────────────────────

    #[test]
    fn probe_blocks_non_http_schemes_without_touching_the_url() {
        let probe = probe_local_daemon("ftp://127.0.0.1:21", PROBE_TIMEOUT);
        assert_eq!(
            probe,
            ProbeResult::Blocked {
                category: "bad_request"
            }
        );
    }

    #[test]
    fn probe_blocks_link_local_even_under_allow_private() {
        // O AllowPrivate libera loopback/LAN, mas link-local (metadata de
        // cloud) continua bloqueado — regra 14.
        let probe = probe_local_daemon("http://169.254.169.254:80", PROBE_TIMEOUT);
        assert_eq!(
            probe,
            ProbeResult::Blocked {
                category: "forbidden"
            }
        );
    }

    #[test]
    fn probe_reports_unreachable_on_closed_loopback_port() {
        // Porta 1 do loopback é fechada em qualquer runner de CI.
        let probe = probe_local_daemon("http://127.0.0.1:1", PROBE_TIMEOUT);
        assert!(matches!(probe, ProbeResult::Unreachable { .. }));
    }

    // ─── collect_provider_checks ───────────────────────────────────────────

    #[test]
    fn provider_checks_are_key_sorted_and_classify_keyless_locals() {
        let cfg = config_with(&[
            (
                "main",
                llm_entry("openrouter", Some("openrouter/auto"), None),
            ),
            ("zeta", llm_entry("unknown-kind", None, None)),
            ("local", llm_entry("ollama", Some("qwen3.8:latest"), None)),
            (
                "llama",
                llm_entry("llamacpp", None, Some("http://127.0.0.1:1")),
            ),
        ]);

        let checks = collect_provider_checks(&cfg);
        let names: Vec<&str> = checks.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(names, vec!["llama", "local", "main", "zeta"]);

        let llama = &checks[0];
        assert!(llama.keyless);
        assert!(llama.credential_source.is_none());
        assert_eq!(llama.probe_url.as_deref(), Some("http://127.0.0.1:1"));
        // Porta 1 fechada → inacessível, e o doctor não morre nisso.
        assert!(matches!(llama.probe, Some(ProbeResult::Unreachable { .. })));

        let ollama = &checks[1];
        assert!(ollama.keyless);
        assert_eq!(ollama.probe_url.as_deref(), Some("http://localhost:11434"));

        let openrouter = &checks[2];
        assert!(!openrouter.keyless);
        assert!(openrouter.credential_source.is_some());
        assert!(openrouter.probe.is_none());
        assert!(openrouter.probe_url.is_none());

        let unknown = &checks[3];
        assert!(!unknown.keyless);
        assert!(unknown.probe.is_none());
    }

    // ─── exit codes ────────────────────────────────────────────────────────

    #[test]
    fn doctor_exit_code_parse_failure_dominates_as_ex_dataerr() {
        let report = DoctorReport {
            version: "test",
            os: "linux",
            arch: "x86_64",
            termux: false,
            config_dir: "/tmp".into(),
            dirs_ok: true,
            config_error: Some("yaml inválido".into()),
            config_check: None,
            providers: vec![],
            daemon: DaemonCheck {
                pid_file_present: false,
                pid: None,
                process_alive: false,
                stale_pid_file: false,
                gateway_host: "127.0.0.1".into(),
                gateway_port: 3888,
                port_listening: false,
            },
            termux_check: None,
        };
        assert_eq!(compute_doctor_exit_code(&report, false), 65);
        assert_eq!(compute_doctor_exit_code(&report, true), 65);
    }

    #[test]
    fn tcp_reachable_false_on_closed_port_and_bad_host() {
        assert!(!tcp_reachable("127.0.0.1", 1, PROBE_TIMEOUT));
        assert!(!tcp_reachable(
            "host.que.nao.existe.invalid",
            80,
            PROBE_TIMEOUT
        ));
    }

    #[test]
    fn category_label_covers_every_ssrf_category() {
        assert_eq!(category_label(SsrfCategory::BadRequest), "bad_request");
        assert_eq!(category_label(SsrfCategory::Forbidden), "forbidden");
        assert_eq!(category_label(SsrfCategory::Upstream), "upstream");
    }
}
