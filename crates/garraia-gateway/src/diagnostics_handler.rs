//! Diagnostics endpoint (plan 0122 / PR-9).
//!
//! Surfaces a single read-only `GET /api/diagnostics` endpoint that runs
//! the per-subsystem health checks the Web Console renders in the
//! Diagnostics page. Each check has the same shape so the UI can render
//! a uniform checklist.
//!
//! Secret-free: when reporting on a secret (e.g. JWT_SECRET) we only
//! emit `configured: true|false`, never the value.
//!
//! Honest statuses (#1437): "the operator never set this up" is a different
//! fact from "this is set up and broken", and the report says which. See
//! [`CheckStatus`] for the taxonomy and [`status_agregado`] for why the
//! neutral states never colour the aggregate.
//!
//! The two voice checks (#1098) are the only ones that touch the network, and
//! only when voice mode is on: with TTS/STT down the gateway used to log a
//! warning nobody read, so `POST /api/tts` answered with a silent text
//! fallback and the operator never learned the server was gone. Surfacing the
//! same fact as an `error` row here is what makes it visible in the console.
//!
//! Probe semantics: any HTTP response proves the endpoint is alive and
//! answering, so 4xx counts as `Reachable` (a `GET` against a health route
//! may simply not exist). Only 5xx means "server is up but the service is
//! broken" → `Unhealthy(status)`. Details never echo the configured URL
//! verbatim — see [`endpoint_publico`].

use std::sync::LazyLock;
use std::time::{Duration, Instant, SystemTime};

use axum::Json;
use axum::extract::State;
use garraia_common::ssrf::{self, IpScope, UrlPolicy};
use serde::Serialize;

use crate::state::SharedState;

/// Severity of one diagnostic row.
///
/// #1437: the four original variants collapsed two very different facts into
/// the same colour. A subsystem the operator never set up (no Telegram token,
/// no TLS certificate, no JWT secret on a local single-user gateway) looked
/// exactly like one that IS set up and is failing — a fresh install lit up
/// yellow for things nobody asked for, and the rows that matter drowned in it.
///
/// The taxonomy, and the line between the three neutral states:
///
/// | Variant | Means | Blocks the aggregate? |
/// |---|---|---|
/// | `Ok` | working | no |
/// | `Warning` | working, with a caveat worth reading | **yes** (`warning`) |
/// | `Error` | configured and **broken** — needs attention | **yes** (`error`) |
/// | `Disabled` | there is a switch for it and it is off (voice mode) | no |
/// | `NotConfigured` | optional subsystem this install never set up | no |
/// | `Skipped` | the check does not apply: nothing was declared to inspect | no |
///
/// `Disabled` vs `NotConfigured` vs `Skipped` is a distinction about the
/// operator's intent, not about severity: all three are neutral and none of
/// them ever pushes the report's aggregate status off `ok` (see
/// [`status_agregado`]). What they buy is an honest console: "you turned this
/// off", "you never set this up" and "there is nothing here to look at" read
/// differently to the person staring at the page.
///
/// **Additive by contract.** The four original variants keep their exact
/// serialized names (`ok` / `warning` / `error` / `skipped`) — `snake_case`
/// and `lowercase` agree on every single-word variant, so switching the
/// rename rule only adds `disabled` and `not_configured` to the vocabulary.
/// Any consumer must therefore treat an unknown status as neutral rather than
/// as an error (the Web Console does: unknown falls back to the `skipped`
/// rendering).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
enum CheckStatus {
    /// All good.
    Ok,
    /// Functional but with a caveat (Ollama optional, etc.).
    Warning,
    /// Broken — needs the user's attention.
    Error,
    /// Not applicable: nothing was declared for this check to inspect.
    Skipped,
    /// Deliberately off: there is a switch for this subsystem and the
    /// operator left it off. Not a defect (#1437).
    Disabled,
    /// Optional subsystem that this install never configured. Not a defect,
    /// and never the same thing as a configured subsystem that fails (#1437).
    NotConfigured,
}

/// The report's aggregate: `error` > `warning` > `ok`.
///
/// The three neutral states (`skipped`, `disabled`, `not_configured`) never
/// contribute — a local single-user install with no Postgres, no object
/// storage, no voice and no channel tokens must aggregate to `ok`, because
/// none of those absences is a defect. The match is exhaustive on purpose:
/// a variant added later has to say out loud which side it is on.
fn status_agregado(checks: &[DiagnosticCheck]) -> &'static str {
    let mut pior = "ok";
    for c in checks {
        match c.status {
            CheckStatus::Error => return "error",
            CheckStatus::Warning => pior = "warning",
            CheckStatus::Ok
            | CheckStatus::Skipped
            | CheckStatus::Disabled
            | CheckStatus::NotConfigured => {}
        }
    }
    pior
}

#[derive(Debug, Clone, Serialize)]
struct DiagnosticCheck {
    /// Stable id ("gateway.responds", "secrets.jwt", ...).
    id: &'static str,
    /// Human label rendered in the UI.
    label: &'static str,
    status: CheckStatus,
    /// Short evidence string. Never contains secret values.
    detail: String,
    /// Suggested next step when status != Ok. `None` when not applicable.
    ///
    /// `String` e nao `&'static str` desde a #1238: o passo do
    /// `whatsapp.linked` precisa citar o diretorio real da ponte, e um
    /// "rode `npm ci`" sem dizer onde manda a pessoa procurar.
    next_step: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct DiagnosticsReport {
    /// Aggregate worst-case status across all checks.
    status: &'static str,
    /// Process version + build info.
    version: &'static str,
    /// Seconds since boot.
    uptime_secs: u64,
    /// Wall-clock timestamp at report generation (server's clock, UTC).
    generated_at: String,
    /// Each per-subsystem check.
    checks: Vec<DiagnosticCheck>,
}

fn now_iso8601() -> String {
    let now = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    // Simple ISO-8601 formatter (UTC). Pulling chrono would be one more
    // dep; this is good enough for a diagnostic timestamp.
    let secs = now % 60;
    let mins = (now / 60) % 60;
    let hours = (now / 3600) % 24;
    let days = now / 86400;
    // Days since 1970-01-01.
    let (year, month, day) = days_to_ymd(days as i64);
    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z",
        year, month, day, hours, mins, secs
    )
}

/// Convert "days since 1970-01-01" to (year, month, day). No external dep.
fn days_to_ymd(days: i64) -> (i32, u32, u32) {
    // Algorithm from Howard Hinnant's date library, public domain.
    let z = days + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = (z - era * 146097) as u64; // [0, 146096]
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365; // [0, 399]
    let y = (yoe as i64) + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // [0, 365]
    let mp = (5 * doy + 2) / 153; // [0, 11]
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = (if mp < 10 { mp + 3 } else { mp - 9 }) as u32;
    let year = (y + if m <= 2 { 1 } else { 0 }) as i32;
    (year, m, d)
}

/// Next step offered when the TTS server is unreachable. Mirrors the command
/// in `docs/voice.md` so the console points at the same thing the doc does.
const TTS_NEXT_STEP: &str = "Start the TTS server: from a chatterbox checkout, run \
     `GRADIO_SERVER_NAME=127.0.0.1 GRADIO_SERVER_PORT=7860 python multilingual_app.py` \
     (docs/voice.md).";

/// Same, for the STT server.
const STT_NEXT_STEP: &str = "Start the STT server: from a whisper.cpp checkout, run \
     `./build/bin/whisper-server --host 127.0.0.1 --port 9090 -m models/ggml-base.bin` \
     (docs/voice.md).";

/// Next step for a 5xx: the process is up, so restarting it is not the
/// instruction — reading its logs is. The start command above is for
/// `Unreachable` only; docs/voice.md and the changelog promise this split.
const TTS_LOGS_STEP: &str =
    "The TTS process answered 5xx — it is up but broken; inspect its logs (docs/voice.md).";

/// Same, for the STT server.
const STT_LOGS_STEP: &str =
    "The STT process answered 5xx — it is up but broken; inspect its logs (docs/voice.md).";

/// Budget for one voice probe. Two of them run per report, and only when
/// voice mode is on — the Diagnostics page must not hang on a dead port.
const VOICE_PROBE_TIMEOUT: Duration = Duration::from_millis(1500);

/// Voice servers are local by design (`docs/voice.md`), so this call site
/// opts into [`IpScope::AllowPrivate`] — the same reason `Ollama` does. The
/// URL comes from config, but it still goes through `vet_url`: scheme,
/// host and blocked ranges (link-local metadata, CGNAT) are checked, and
/// `pinned_client` pins the resolved address so it cannot be swapped
/// between the check and the connect.
fn voice_policy() -> UrlPolicy {
    UrlPolicy::http_public(VOICE_PROBE_TIMEOUT, "garraia-gateway/diagnostics")
        .with_ip_scope(IpScope::AllowPrivate)
}

/// Outcome of reaching (or not) one configured voice endpoint. Kept as an
/// enum rather than a `Result` so [`voice_check`] stays a pure function —
/// that is what lets the mapping be tested without a network.
#[derive(Debug, Clone, PartialEq, Eq)]
enum VoiceProbe {
    /// Voice mode is off in this process: nothing to reach, nothing wrong.
    Disabled,
    /// Something answered with a non-5xx status (4xx included: a `GET` on a
    /// health route may not exist — the server being up is what counts).
    Reachable,
    /// The server answered with a 5xx: up, but the service is broken.
    Unhealthy(u16),
    /// Vetted and dialled, but no answer. Carries a short, stable reason.
    Unreachable(&'static str),
    /// Not a URL this gateway may call: bad scheme, no host, blocked range.
    Invalid(String),
}

/// Motivo legível para um veto do guard SSRF, montado campo a campo.
///
/// `/api/diagnostics` é auth-free sem a chave do gateway: o `detail` de um
/// endpoint inválido não pode depender do `Display` de `SsrfRejection`
/// acontecer de não ecoar a URL crua — uma variante nova amanhã embutindo a
/// URL abriria o vazamento sem testar ninguém. Aqui só entram campos que
/// estruturalmente não carregam userinfo: scheme, host e IP vêm dos
/// componentes homônimos da URL parseada, e o texto interno de
/// `InvalidUrl`/`ResolveFailed`/`ClientBuild`/`Transport` não é repassado
/// (não é nosso, e o `url` pode embutir trechos da entrada).
fn motivo_do_veto(e: &ssrf::SsrfRejection) -> String {
    use ssrf::SsrfRejection as R;
    match e {
        R::InvalidUrl(_) => "URL invalida".to_string(),
        R::SchemeNotAllowed { scheme, .. } => format!("esquema '{scheme}' nao permitido"),
        R::MissingHost => "URL sem host".to_string(),
        R::HostNotAllowed(host) => format!("host '{host}' fora da allowlist"),
        R::ResolveFailed(_) => "falha ao resolver o host".to_string(),
        R::BlockedAddress { host, ip } => {
            format!("host '{host}' resolve para endereco bloqueado ({ip})")
        }
        R::ClientBuild(_) => "nao foi possivel construir o cliente HTTP".to_string(),
        R::BodyTooLarge { .. } | R::Transport(_) => "requisicao falhou".to_string(),
    }
}

async fn probe_voice_endpoint(endpoint: &str) -> VoiceProbe {
    let policy = voice_policy();
    let vetted = match ssrf::vet_url(endpoint, &policy) {
        Ok(v) => v,
        Err(e) => return VoiceProbe::Invalid(motivo_do_veto(&e)),
    };
    let client = match ssrf::pinned_client(&vetted, &policy) {
        Ok(c) => c,
        Err(e) => return VoiceProbe::Invalid(motivo_do_veto(&e)),
    };
    match client.get(vetted.url.clone()).send().await {
        Ok(resp) => {
            let status = resp.status();
            if status.is_server_error() {
                VoiceProbe::Unhealthy(status.as_u16())
            } else {
                VoiceProbe::Reachable
            }
        }
        Err(e) => VoiceProbe::Unreachable(if e.is_connect() {
            "nothing listening (connection refused)"
        } else if e.is_timeout() {
            "no answer within 1.5s"
        } else {
            "request failed"
        }),
    }
}

/// Strips an endpoint down to `scheme://host[:port]` for display.
///
/// `/api/diagnostics` is auth-free when the gateway key is absent, so echoing
/// the raw configured URL would leak anything embedded in it — userinfo
/// (`http://user:pass@host/`), paths, query strings, fragments — into the
/// response body. `url::Url::parse` happily accepts userinfo and
/// `host_str()` ignores it, so the redaction has to happen here, before any
/// `format!` that lands in the report. The module promises "Secret-free";
/// this keeps that promise for the owner's own config.
fn endpoint_publico(endpoint: &str) -> String {
    let Ok(u) = url::Url::parse(endpoint) else {
        return "<endpoint invalido>".to_string();
    };
    let Some(host) = u.host_str() else {
        return "<endpoint invalido>".to_string();
    };
    if host.is_empty() || !matches!(u.scheme(), "http" | "https") {
        return "<endpoint invalido>".to_string();
    }
    match u.port() {
        Some(p) => format!("{}://{}:{}", u.scheme(), host, p),
        None => format!("{}://{}", u.scheme(), host),
    }
}

/// Build the diagnostic row for one voice endpoint.
///
/// `Error` (not `Warning`) when a configured server is down: a warning is
/// exactly what #1098 reports as too easy to miss. The endpoint is echoed
/// only through [`endpoint_publico`] — never verbatim — because this report
/// is auth-free without the gateway key.
///
/// `next_step` (the start command) is for `Unreachable` only; `unhealthy_step`
/// is what a 5xx shows — the process is up, so the instruction is to read
/// its logs, not to start it again. docs/voice.md documents both.
fn voice_check(
    id: &'static str,
    label: &'static str,
    endpoint: &str,
    next_step: &'static str,
    unhealthy_step: &'static str,
    probe: VoiceProbe,
) -> DiagnosticCheck {
    let ep = endpoint_publico(endpoint);
    let (status, detail, next_step) = match probe {
        // #1437: `disabled`, nao `skipped`. Ha um interruptor para o modo voz
        // e o operador o deixou desligado — dizer isso e mais honesto do que
        // "nao se aplica", e continua neutro no agregado.
        VoiceProbe::Disabled => (
            CheckStatus::Disabled,
            "voice mode not enabled (start the gateway with --with-voice)".to_string(),
            None,
        ),
        VoiceProbe::Reachable => (CheckStatus::Ok, format!("reachable at {ep}"), None),
        VoiceProbe::Unhealthy(code) => (
            CheckStatus::Error,
            format!("{ep} respondeu HTTP {code} — servidor de pe, servico quebrado"),
            Some(unhealthy_step.to_string()),
        ),
        VoiceProbe::Unreachable(reason) => (
            CheckStatus::Error,
            format!("{ep} unreachable: {reason}"),
            Some(next_step.to_string()),
        ),
        VoiceProbe::Invalid(reason) => (
            CheckStatus::Error,
            format!("{ep}: {reason}"),
            Some(
                "Set the voice endpoint to an http(s) URL on a host this gateway may reach."
                    .to_string(),
            ),
        ),
    };
    DiagnosticCheck {
        id,
        label,
        status,
        detail,
        next_step,
    }
}

/// Which TTS endpoint is actually in play — `hibiki_endpoint` only when the
/// configured provider is `hibiki`, otherwise the shared default.
fn active_tts_endpoint(config: &garraia_config::VoiceConfig) -> &str {
    if config.tts_provider.eq_ignore_ascii_case("hibiki") {
        &config.hibiki_endpoint
    } else {
        &config.tts_endpoint
    }
}

/// How long a pair of probe results is reused for the same endpoints.
/// The Diagnostics page may be polled repeatedly; without this every poll
/// dials both voice servers, which a hostile client can turn into a probe
/// amplifier against local addresses.
const VOICE_PROBE_CACHE_TTL: Duration = Duration::from_secs(5);

struct VoiceProbes {
    gravado_em: Instant,
    tts_endpoint: String,
    stt_endpoint: String,
    tts: VoiceProbe,
    stt: VoiceProbe,
}

// `tokio::sync::Mutex::new` não é `const` neste toolchain, então o cache
// nasce preguiçoso: `LazyLock` resolve na primeira sonda.
static VOICE_PROBE_CACHE: LazyLock<tokio::sync::Mutex<Option<VoiceProbes>>> =
    LazyLock::new(|| tokio::sync::Mutex::new(None));

/// Probe both voice servers, in parallel, with a short cache keyed by the
/// endpoints actually configured. Both probes share the 1.5 s budget each
/// (they dial different ports concurrently), and a repeated report within
/// the TTL reuses the previous result instead of re-dialling.
async fn sondas_de_voz(cfg: &garraia_config::VoiceConfig) -> (VoiceProbe, VoiceProbe) {
    let tts_endpoint = active_tts_endpoint(cfg).to_string();
    let stt_endpoint = cfg.stt_endpoint.clone();
    // O lock fica preso atravessando a sonda inteira (tokio Mutex e feito
    // para isso): quem chega junto serializa e encontra o cache ja fresco.
    // Sem isso, a janela entre o check e o preenchimento deixava N chamadas
    // concorrentes dispararem N pares de dials — exatamente o amplificador
    // que o TTL existe para impedir, so que pela porta dos fundos.
    let mut guard = VOICE_PROBE_CACHE.lock().await;
    if let Some(c) = guard.as_ref()
        && c.gravado_em.elapsed() < VOICE_PROBE_CACHE_TTL
        && c.tts_endpoint == tts_endpoint
        && c.stt_endpoint == stt_endpoint
    {
        return (c.tts.clone(), c.stt.clone());
    }
    let (tts, stt) = tokio::join!(
        probe_voice_endpoint(&tts_endpoint),
        probe_voice_endpoint(&stt_endpoint),
    );
    *guard = Some(VoiceProbes {
        gravado_em: Instant::now(),
        tts_endpoint,
        stt_endpoint,
        tts: tts.clone(),
        stt: stt.clone(),
    });
    (tts, stt)
}

/// A linha `whatsapp.linked` do relatorio.
///
/// Recebe o veredito ja classificado (e nao o `AppConfig`) para poder ser
/// exercitada sem montar `AppState`: os cinco estados cabem em cinco asserts.
///
/// Mapeamento de severidade, e o porque de cada um:
///
/// | Veredito | Status | Por que |
/// |---|---|---|
/// | `NotLinked` | `not_configured` | canal opcional que ninguem ligou — nao e defeito (#1437) |
/// | `MissingDependencies` | `error` | o operador ligou e o canal nao funciona |
/// | `BridgeDown` | `error` | idem: ha sessao e nao ha canal |
/// | `Connected` | `ok` | |
/// | `Linked` | `ok` | ha sessao e a ponte nao esta sendo supervisionada aqui |
///
/// Secret-free: nenhum caminho de sessao, nenhum JID, nenhum telefone. O
/// diretorio da **ponte** aparece porque e onde o `npm ci` precisa rodar — ele
/// nao guarda credencial, so `bridge.mjs` e `node_modules`.
fn whatsapp_linked_check(
    (saude, bridge_dir): (
        garraia_channels::whatsapp_linked::health::LinkHealth,
        std::path::PathBuf,
    ),
) -> DiagnosticCheck {
    use garraia_channels::whatsapp_linked::health::LinkHealth;

    let (status, detail) = match saude {
        LinkHealth::NotLinked => (
            CheckStatus::NotConfigured,
            "nenhum aparelho vinculado (canal opcional)".to_string(),
        ),
        LinkHealth::MissingDependencies => (
            CheckStatus::Error,
            format!(
                "ha sessao vinculada, mas a ponte esta sem dependencias em {}",
                bridge_dir.display()
            ),
        ),
        LinkHealth::BridgeDown => (
            CheckStatus::Error,
            "ha sessao vinculada e a ponte nao esta conectada".to_string(),
        ),
        LinkHealth::Connected => (CheckStatus::Ok, "conectado".to_string()),
        LinkHealth::Linked => (
            CheckStatus::Ok,
            "sessao vinculada (a ponte nao e supervisionada por este processo)".to_string(),
        ),
    };

    DiagnosticCheck {
        id: "whatsapp.linked",
        label: "WhatsApp (dispositivo vinculado)",
        status,
        detail,
        next_step: saude.next_step(&bridge_dir, &garraia_common::executavel::nome()),
    }
}

/// #1345: um vinculo saudavel com o portao vazio vira `Warning`.
///
/// O canal esta ligado, a ponte conecta, e toda mensagem e descartada em
/// silencio porque ninguem esta em `allow` nem em `owners`. `Ok` ali era a
/// mentira que deixava o operador esperando uma resposta que nunca vinha.
/// So rebaixa `Ok`: `Skipped` (nao vinculado) e `Error` (ponte quebrada) ja
/// tem o proximo passo certo, e o do portao so importa depois deles.
///
/// Mais dois casos que o `Ok` escondia:
///
/// - **Canal desligado na config viva com a ponte conectada.** O supervisor
///   do boot segue de pe e o turno recusa todo mundo, codigo de pareamento
///   incluso (`enabled` e relido a quente; a ponte so desce no restart).
/// - **Recusas de remetente `@lid` sem numero** desde o boot: um numero no
///   `allow` nao casa com um LID, e sem isto o operador so via "autorizado"
///   e silencio. Vai no detalhe, sem mudar o status — um estranho com LID
///   tambem e recusado, e isso nao e defeito.
///
/// Contagem, nunca identidade: a rota e auth-free.
fn whatsapp_linked_portao_vazio(
    mut check: DiagnosticCheck,
    saude: garraia_channels::whatsapp_linked::health::LinkHealth,
    settings: &crate::bootstrap::WhatsAppLinkedSettings,
    recusas_lid: u64,
    a_quente: bool,
) -> DiagnosticCheck {
    use garraia_channels::whatsapp_linked::health::LinkHealth;

    let bin = garraia_common::executavel::nome();
    if matches!(check.status, CheckStatus::Ok)
        && !settings.enabled
        && saude == LinkHealth::Connected
    {
        check.status = CheckStatus::Warning;
        check.detail = format!(
            "{} — mas o canal esta desligado na config viva (`channels.whatsapp_linked.enabled` \
             nao e `true`): toda mensagem e recusada, e a ponte segue conectada ate reiniciar",
            check.detail
        );
        check.next_step = Some(format!(
            "religue `channels.whatsapp_linked.enabled: true` no config.yml, ou rode `{bin} \
             restart` para o gateway descer a ponte"
        ));
        return check;
    }
    if matches!(check.status, CheckStatus::Ok) && recusas_lid > 0 {
        check.detail = format!(
            "{} — {recusas_lid} mensagem(ns) de remetente @lid sem numero recusada(s) desde o \
             boot: um numero no `allow` nao casa com LID (`{bin} whatsapp status` mostra o final)",
            check.detail
        );
    }
    if matches!(check.status, CheckStatus::Ok) && settings.enabled && settings.autorizados() == 0 {
        check.status = CheckStatus::Warning;
        check.detail = format!(
            "{} — mas nenhum numero esta autorizado (`allow` e `owners` vazios): toda \
             mensagem e descartada em silencio",
            check.detail
        );
        // "Sem reiniciar" so com o `ConfigWatcher` ligado: sem ele o turno
        // usa a lista do boot ate o proximo restart.
        check.next_step = Some(if a_quente {
            format!("rode `{bin} whatsapp allow <numero>` (com codigo do pais; vale sem reiniciar)")
        } else {
            format!(
                "rode `{bin} whatsapp allow <numero>` (com codigo do pais) e depois `{bin} \
                 restart`: este gateway nao vigia o config.yml"
            )
        });
    }
    check
}

// ─── ADR 0024 (#1329): perfil de execucao e raiz do MCP filesystem ──────────

/// O que o `execution.profile` reporta sobre o canal `whatsapp_linked`: o
/// piso do DONO nesse perfil (`default_mode` explicito, senao o default do
/// perfil — `search` em `standard`, `code` em `isolated-pod`, a mesma regra
/// que `LinkedSettings::modo_padrao_efetivo` aplica no turno) e a CONTAGEM de
/// `owners`. Nunca as identidades — a rota e auth-free. Puro.
fn piso_e_donos_do_whatsapp(
    config: &garraia_config::AppConfig,
    perfil: garraia_config::ExecutionProfile,
) -> (String, usize) {
    let settings = crate::bootstrap::whatsapp_linked_settings(config);
    (settings.modo_padrao_efetivo(perfil), settings.owners.len())
}

/// Um caminho como o console o mostra: relativo a `<data_dir>` quando esta
/// dentro dele. A rota e auth-free, e as raizes de politica (o workspace
/// default, `agent.file_roots`) nao precisam expor o caminho absoluto do
/// host para o operador entender a linha (F-1 da auditoria da #1329). Uma
/// raiz **fora** do `data_dir` sai como esta — e o que o operador precisa
/// ver para consertar.
fn exibir_raiz(raiz: &std::path::Path, data_dir: &std::path::Path) -> String {
    match raiz.strip_prefix(data_dir) {
        Ok(rel) if rel.as_os_str().is_empty() => "<data_dir>".to_string(),
        Ok(rel) => format!("<data_dir>/{}", rel.display()),
        Err(_) => raiz.display().to_string(),
    }
}

fn lista_de_caminhos(raizes: &[std::path::PathBuf], data_dir: &std::path::Path) -> String {
    if raizes.is_empty() {
        return "(nenhuma)".to_string();
    }
    raizes
        .iter()
        .map(|r| exibir_raiz(r, data_dir))
        .collect::<Vec<_>>()
        .join(", ")
}

/// A linha `execution.profile`. `standard` e `ok`; `isolated-pod` e SEMPRE
/// `warning`, porque o risco numero um do ADR 0024 e o operador ligar o
/// perfil fora de um pod — e o console e o lugar onde ele ve isso sem ler o
/// log de boot. O detalhe diz o que foi liberado (piso do WhatsApp, quantos
/// donos, quais raizes) e o passo diz como reverter. Puro.
fn execution_profile_check(
    politica: &crate::bootstrap::PoliticaDeExecucao,
    piso_whatsapp: &str,
    donos: usize,
    raizes_mcp: &[std::path::PathBuf],
    data_dir: &std::path::Path,
) -> DiagnosticCheck {
    let (status, detail, next_step) = if politica.is_isolated_pod() {
        (
            CheckStatus::Warning,
            format!(
                "isolated-pod (fonte: {}; piso do WhatsApp pessoal: {piso_whatsapp}; \
                 owners com perfil completo: {donos}; raiz do MCP filesystem: {})",
                politica.origem,
                lista_de_caminhos(raizes_mcp, data_dir)
            ),
            Some(
                "confirme que este processo roda num pod/container isolado; para reverter: \
                 execution.profile = standard (ou remova GARRAIA_EXECUTION_PROFILE)"
                    .to_string(),
            ),
        )
    } else {
        (
            CheckStatus::Ok,
            format!("standard (seguro por padrao; fonte: {})", politica.origem),
            None,
        )
    };
    DiagnosticCheck {
        id: "execution.profile",
        label: "Perfil de execucao",
        status,
        detail,
        next_step,
    }
}

/// #1272: a linha `tools.bash`. `ok` quando o `bash` esta registrado (num
/// sandbox docker/podman, ou no host de um `isolated-pod` explicito);
/// `warning` com o passo acionavel quando ele ficou de fora em `standard`.
/// O detalhe nunca carrega valor de config (imagem, host, caminho). Pura.
fn tools_bash_check(exposicao: &crate::bootstrap::ExposicaoDoBash) -> DiagnosticCheck {
    let (status, next_step) = if exposicao.registra_bash() {
        (CheckStatus::Ok, None)
    } else {
        (
            CheckStatus::Warning,
            Some(crate::bootstrap::COMO_LIGAR_O_BASH.to_string()),
        )
    };
    DiagnosticCheck {
        id: "tools.bash",
        label: "Tool bash",
        status,
        detail: exposicao.descricao(),
        next_step,
    }
}

/// `raiz` esta dentro de `permitida`? Cada lado vai para a sua forma
/// comparavel ([`forma_comparavel`]) e a resposta e um `starts_with` por
/// componente. Um lado que nao tem forma comparavel (um `..` que escapa da
/// raiz do filesystem) esta **fora** — nunca "dentro" por acidente.
fn dentro_de(raiz: &std::path::Path, permitida: &std::path::Path) -> bool {
    match (forma_comparavel(raiz), forma_comparavel(permitida)) {
        (Some(a), Some(b)) => a.starts_with(&b),
        _ => false,
    }
}

/// A forma em que dois caminhos podem ser comparados por prefixo.
///
/// Primeiro o lexico: `.` some, `..` consome o componente anterior, e um
/// `..` que passaria da raiz e `None` (review C4 da #1329 — antes o
/// fallback era um `Path::starts_with` cru, e `/srv/x/../../etc` "comecava
/// com" `/srv/x` quando `/srv/x` ainda nao existia). Depois o canonico: o
/// maior prefixo que existe e canonicalizado (um symlink `~/ws -> /` nao
/// pode passar por lexico) e o resto e reanexado, para um diretorio ainda
/// nao criado dentro de um `permitida` que existe continuar comparavel.
fn forma_comparavel(p: &std::path::Path) -> Option<std::path::PathBuf> {
    let normal = normalizar_lexico(p)?;
    let mut prefixo = normal.as_path();
    let mut resto: Vec<&std::ffi::OsStr> = Vec::new();
    loop {
        if let Ok(canonico) = prefixo.canonicalize() {
            let mut out = canonico;
            for comp in resto.iter().rev() {
                out.push(comp);
            }
            return Some(out);
        }
        let Some(nome) = prefixo.file_name() else {
            // Nada do caminho existe (ou e relativo): fica o lexico.
            return Some(normal.clone());
        };
        resto.push(nome);
        prefixo = prefixo.parent()?;
    }
}

/// Normalizacao lexica: sem `.`; `..` consome o componente anterior; `None`
/// quando um `..` tenta subir alem do que ha (escapa da raiz, ou de um
/// caminho relativo sem ancestral).
fn normalizar_lexico(p: &std::path::Path) -> Option<std::path::PathBuf> {
    use std::path::Component;
    let mut out = std::path::PathBuf::new();
    for c in p.components() {
        match c {
            Component::CurDir => {}
            Component::ParentDir => {
                if !out.pop() {
                    return None;
                }
            }
            outro => out.push(outro.as_os_str()),
        }
    }
    Some(out)
}

/// A linha `files.workspace` (#1378): qual e o workspace efetivo das file
/// tools nativas e **por que** ele e esse.
///
/// As tres fontes mapeiam direto para os tres estados que o operador precisa
/// distinguir, na linha do que a #1445 fez com o resto do relatorio:
///
/// - `Declaradas` → `ok`. O operador escolheu, e a escolha vale.
/// - `WorkspacePadrao` → `ok`. Nada foi declarado e o Garra usa o proprio
///   workspace. Nao e aviso: e o default seguro, e chamar de aviso ensinaria o
///   operador a ignorar avisos. Aqui a linha mostra o diretorio **pai** com o
///   `<sessao>` explicito no fim, porque desde a #1449 a raiz efetiva de uma
///   chamada e `<data_dir>/workspace/<sessao>` e nao o pai: dizer so o pai
///   faria o console prometer mais alcance do que o turno tem. O identificador
///   da sessao nunca sai daqui — a rota e auth-free, e o nome do subdiretorio
///   e derivado do `session_id`, que pode ser PII.
/// - `SomenteSessao` → `warning`. E o defeito da #1378 ainda de pe: sessao sem
///   `working_dir` (toda sessao do WhatsApp recem-vinculada) nao le nem
///   escreve nada.
///
/// As raizes saem relativas a `<data_dir>` quando estao dentro dele — a rota e
/// auth-free e nao precisa publicar o caminho absoluto do host (F-1 da
/// auditoria da #1329). Pura: as raizes chegam ja resolvidas.
fn files_workspace_check(
    fonte: crate::bootstrap::FonteDasRaizesDasFileTools,
    raizes: &[std::path::PathBuf],
    workspace_por_sessao: Option<&std::path::Path>,
    data_dir: &std::path::Path,
) -> DiagnosticCheck {
    use crate::bootstrap::FonteDasRaizesDasFileTools as Fonte;
    let (status, detail, next_step) = match fonte {
        Fonte::Declaradas => (
            CheckStatus::Ok,
            format!(
                "{} (fonte: agent.file_roots / GARRAIA_FILE_ROOTS)",
                lista_de_caminhos(raizes, data_dir)
            ),
            None,
        ),
        Fonte::WorkspacePadrao => (
            CheckStatus::Ok,
            format!(
                "{}/<sessao> (fonte: workspace padrao — nada declarado em agent.file_roots; \
                 um subdiretorio por sessao)",
                workspace_por_sessao
                    .map(|raiz| exibir_raiz(raiz, data_dir))
                    .unwrap_or_else(|| "(nenhuma)".to_string())
            ),
            None,
        ),
        Fonte::SomenteSessao => (
            CheckStatus::Warning,
            "sem raiz efetiva: o workspace padrao nao resolveu e nada foi declarado. \
             file_read, file_write e list_dir negam tudo numa sessao sem working_dir"
                .to_string(),
            Some(
                "confira se o <data_dir> existe e e gravavel, ou declare uma raiz em \
                 agent.file_roots (ou na env GARRAIA_FILE_ROOTS)"
                    .to_string(),
            ),
        ),
    };
    DiagnosticCheck {
        id: "files.workspace",
        label: "Workspace das file tools",
        status,
        detail,
        next_step,
    }
}

const MCP_ROOT_NEXT_STEP: &str = "edite a entrada `filesystem` (em mcp.json, ou em `mcp:` do \
                                  config.yml, que vence o mcp.json; ou \
                                  GARRAIA_DISABLE_MCP_AUTOPROVISION=1 + remova o servidor) para \
                                  apontar para uma raiz dentro de agent.file_roots ou de \
                                  <data_dir>/workspace; ou declare execution.profile = \
                                  isolated-pod se este processo roda num pod";

/// A linha `mcp.filesystem_root`: a raiz que o servidor `filesystem`
/// efetivo declara (`mcp:` do config.yml vence o mcp.json, como no boot),
/// contra as raizes **declaradas** do perfil. Sem entrada => `skipped`.
/// Em `isolated-pod` qualquer raiz pod-local e `ok` por declaracao (o pod e
/// a fronteira). Em `standard` toda raiz precisa estar dentro de uma das
/// `permitidas` — `agent.file_roots` ou `<data_dir>/workspace`, o mesmo
/// conjunto que o autoprovisionamento escreve; a env `GARRAIA_FILE_ROOTS` e
/// o `working_dir` da sessao, que alargam o jail das file tools nativas,
/// NAO entram aqui de proposito (ver `bootstrap::raizes_do_mcp_filesystem`).
/// A primeira raiz fora vira `warning` nomeando-a, como esta — e a entrada
/// legada com `$HOME` que instalacoes anteriores a #1329 ainda carregam.
/// As raizes permitidas saem relativas a `<data_dir>` quando estao dentro
/// dele. Puro.
fn mcp_filesystem_root_check(
    perfil_isolado: bool,
    persistidas: Option<&[std::path::PathBuf]>,
    permitidas: &[std::path::PathBuf],
    data_dir: &std::path::Path,
) -> DiagnosticCheck {
    let (status, detail, next_step) = match persistidas {
        None => (
            CheckStatus::Skipped,
            "nenhum servidor `filesystem` em mcp.json nem em `mcp:` do config.yml".to_string(),
            None,
        ),
        Some(raizes) if perfil_isolado => (
            CheckStatus::Ok,
            format!(
                "{} (isolated-pod: o pod e a fronteira)",
                lista_de_caminhos(raizes, data_dir)
            ),
            None,
        ),
        Some([]) => (
            CheckStatus::Warning,
            "o servidor `filesystem` nao declara nenhuma raiz".to_string(),
            Some(MCP_ROOT_NEXT_STEP.to_string()),
        ),
        Some(raizes) => {
            let fora = raizes
                .iter()
                .find(|r| !permitidas.iter().any(|p| dentro_de(r, p)));
            match fora {
                Some(raiz) => (
                    CheckStatus::Warning,
                    format!(
                        "{} esta fora das raizes declaradas (agent.file_roots / \
                         <data_dir>/workspace): {}",
                        raiz.display(),
                        lista_de_caminhos(permitidas, data_dir)
                    ),
                    Some(MCP_ROOT_NEXT_STEP.to_string()),
                ),
                None => (
                    CheckStatus::Ok,
                    format!(
                        "{} (dentro das raizes declaradas)",
                        lista_de_caminhos(raizes, data_dir)
                    ),
                    None,
                ),
            }
        }
    };
    DiagnosticCheck {
        id: "mcp.filesystem_root",
        label: "MCP filesystem (raiz)",
        status,
        detail,
        next_step,
    }
}

/// #1346: o proximo passo para UM servidor MCP que falhou, pela causa
/// classificada. `bin` e o nome do executavel instalado (`garraia`/`garra`).
fn mcp_server_next_step(
    name: &str,
    cause: Option<&garraia_agents::McpFailureCause>,
    bin: &str,
) -> String {
    use garraia_agents::McpFailureCause;
    let restart = format!(
        "e reinicie o servidor com POST /admin/api/mcp/{name}/restart (ou reinicie o `{bin}`)"
    );
    match cause {
        // `dir` so chega aqui depois de o manager ter reconstruido e
        // validado a entrada dentro do cache do npm (revisao MCP-4): nunca e
        // o caminho que o processo filho imprimiu.
        Some(McpFailureCause::NpxCacheCorrupt { dir: Some(dir) }) => format!(
            "{name}: o cache do npx em {} esta incompleto/corrompido. Apague esse diretorio \
             (ou rode `npm cache verify`) {restart}.",
            dir.display()
        ),
        // Sem `dir`: integridade (EINTEGRITY) ou uma entrada que o gateway nao
        // conseguiu confirmar dentro do cache — nao se repete caminho nenhum.
        Some(McpFailureCause::NpxCacheCorrupt { dir: None }) => {
            format!(
                "{name}: o cache do npx esta incompleto ou falhou a verificacao de integridade. \
                 Rode `npm cache verify` (ou apague a entrada `_npx/<hash>` do pacote dentro \
                 do seu cache do npm) {restart}."
            )
        }
        Some(McpFailureCause::DiskFull) => {
            format!("{name}: disco cheio (ENOSPC). Libere espaco em disco {restart}.")
        }
        _ => format!(
            "{name}: veja `last_error` em GET /api/mcp/health e o stderr do processo \
             (RUST_LOG=garraia_agents=debug); corrija a causa {restart}."
        ),
    }
}

/// #1346: a linha `mcp.servers` — todo servidor MCP que o manager conhece,
/// inclusive os que falharam no boot e nunca entraram em `connections`.
/// `None` (sem manager) ou lista vazia => `skipped`; algum `failed` (restarts
/// esgotados) => `error` com um passo por servidor; algum ainda tentando =>
/// `warning`; todos conectados => `ok`. Puro.
fn mcp_servers_check(
    statuses: Option<&[garraia_agents::McpServerStatus]>,
    bin: &str,
) -> DiagnosticCheck {
    use garraia_agents::McpServerState;
    let (status, detail, next_step) = match statuses {
        None | Some([]) => (
            CheckStatus::Skipped,
            "nenhum servidor MCP configurado".to_string(),
            None,
        ),
        Some(list) => {
            let describe = |s: &garraia_agents::McpServerStatus| match s.state {
                McpServerState::Connected => format!("{} ok ({} tools)", s.name, s.tool_count),
                other => format!(
                    "{} {} ({}/{} tentativas, causa: {})",
                    s.name,
                    other.as_str(),
                    s.attempts,
                    s.max_restarts,
                    s.cause
                        .as_ref()
                        .map(|c| c.as_str())
                        .unwrap_or("desconhecida")
                ),
            };
            let detail = list.iter().map(describe).collect::<Vec<_>>().join("; ");
            let failed: Vec<_> = list
                .iter()
                .filter(|s| s.state == McpServerState::Failed)
                .collect();
            let not_ok: Vec<_> = list
                .iter()
                .filter(|s| s.state != McpServerState::Connected)
                .collect();
            if !failed.is_empty() {
                let steps = failed
                    .iter()
                    .map(|s| mcp_server_next_step(&s.name, s.cause.as_ref(), bin))
                    .collect::<Vec<_>>()
                    .join(" ");
                (CheckStatus::Error, detail, Some(steps))
            } else if !not_ok.is_empty() {
                let steps = not_ok
                    .iter()
                    .map(|s| mcp_server_next_step(&s.name, s.cause.as_ref(), bin))
                    .collect::<Vec<_>>()
                    .join(" ");
                (CheckStatus::Warning, detail, Some(steps))
            } else {
                (CheckStatus::Ok, detail, None)
            }
        }
    };
    DiagnosticCheck {
        id: "mcp.servers",
        label: "MCP servers",
        status,
        detail,
        next_step,
    }
}

/// #1346: a linha `mcp.filesystem_pinned` — a entrada `filesystem` efetiva
/// roda o `server-filesystem` com versao fixada? Sem versao, cada cache frio
/// do npx baixa o build mais novo do registry. O Garra nunca reescreve um
/// mcp.json existente, entao o aviso diz exatamente o que colar. Puro.
fn mcp_filesystem_pinned_check(
    versao: &crate::mcp::persistence::VersaoDoFilesystem,
    bin: &str,
) -> DiagnosticCheck {
    use crate::mcp::persistence::VersaoDoFilesystem;
    let (status, detail, next_step) = match versao {
        VersaoDoFilesystem::Ausente => (
            CheckStatus::Skipped,
            "nenhum servidor `filesystem` em mcp.json nem em `mcp:` do config.yml".to_string(),
            None,
        ),
        VersaoDoFilesystem::ForaDoNpx => (
            CheckStatus::Ok,
            "o `filesystem` nao roda via npx; versao e do operador".to_string(),
            None,
        ),
        VersaoDoFilesystem::Fixada(v) => (
            CheckStatus::Ok,
            format!("@modelcontextprotocol/server-filesystem@{v}"),
            None,
        ),
        VersaoDoFilesystem::SemVersao { args_sugeridos } => (
            CheckStatus::Warning,
            "o `filesystem` roda `npx -y @modelcontextprotocol/server-filesystem` sem versao: \
             cada cache frio baixa o que for mais novo no registry"
                .to_string(),
            Some(format!(
                "Troque os `args` do `filesystem` (mcp.json, ou `mcp:` do config.yml) por {} \
                 e reinicie o `{bin}`. O Garra nunca reescreve um mcp.json existente.",
                serde_json::Value::from(args_sugeridos.clone())
            )),
        ),
    };
    DiagnosticCheck {
        id: "mcp.filesystem_pinned",
        label: "MCP filesystem (versao)",
        status,
        detail,
        next_step,
    }
}

/// GET /api/diagnostics — full diagnostic report.
pub async fn diagnostics_handler(State(state): State<SharedState>) -> Json<DiagnosticsReport> {
    let mut checks: Vec<DiagnosticCheck> = Vec::new();

    // 1. Gateway responds — we are responding right now, so this is OK.
    checks.push(DiagnosticCheck {
        id: "gateway.responds",
        label: "Gateway responds",
        status: CheckStatus::Ok,
        detail: format!(
            "Live at {}:{}",
            state.config.gateway.host, state.config.gateway.port
        ),
        next_step: None,
    });

    // 2. Listener port valid range.
    let port = state.config.gateway.port;
    checks.push(DiagnosticCheck {
        id: "gateway.port",
        label: "Listener port valid",
        status: if (1..=65535).contains(&port) {
            CheckStatus::Ok
        } else {
            CheckStatus::Error
        },
        detail: format!("port={}", port),
        next_step: if (1..=65535).contains(&port) {
            None
        } else {
            // #1261: `gateway.port` do arquivo esta deprecado e nao e lido.
            Some(format!(
                "Start with `{} start --port <1..=65535>` or set the PORT env var.",
                garraia_common::executavel::nome()
            ))
        },
    });

    // 3. Config dir exists.
    let cfg_dir = garraia_config::ConfigLoader::default_config_dir();
    let cfg_exists = cfg_dir.exists();
    checks.push(DiagnosticCheck {
        id: "config.dir",
        label: "Config directory",
        status: if cfg_exists {
            CheckStatus::Ok
        } else {
            CheckStatus::Warning
        },
        detail: format!("{}", cfg_dir.display()),
        next_step: if cfg_exists {
            None
        } else {
            Some("Run `garraia init` to scaffold ~/.garraia.".to_string())
        },
    });

    // 3b. ADR 0024 (#1329): perfil de execucao e raiz do MCP `filesystem`.
    // A politica e pura (config ja carregada); as raizes persistidas vem do
    // registry em memoria, que e o mcp.json carregado no boot mais o que a
    // admin API gravou desde entao — sem I/O de disco por request.
    let politica = crate::bootstrap::politica_de_execucao(&state.config);
    let raizes_mcp = crate::bootstrap::raizes_do_mcp_filesystem(&state.config);
    // O `data_dir` precisa vir CANONICO para a relativizacao funcionar: as
    // raizes que o `FileJail` devolve ja passaram por `canonicalize`, e o valor
    // cru da config pode ser relativo ou conter symlink (`/var` -> `/private/var`
    // no macOS, `$TMPDIR` na suite). Comparar cru contra canonico faz o
    // `strip_prefix` de `exibir_raiz` errar em silencio, e o fallback imprime o
    // caminho ABSOLUTO do host numa rota auth-free — o oposto do F-1 da #1329.
    // Se o diretorio ainda nao existe, `canonicalize` falha e sobra o valor cru;
    // ali nenhuma raiz resolve, entao nao ha caminho para vazar.
    let data_dir = state.config.resolved_data_dir();
    let data_dir = data_dir.canonicalize().unwrap_or(data_dir);
    let (piso_whatsapp, donos) = piso_e_donos_do_whatsapp(&state.config, politica.perfil);
    checks.push(execution_profile_check(
        &politica,
        &piso_whatsapp,
        donos,
        raizes_mcp.caminhos(),
        &data_dir,
    ));
    // A entrada efetiva: `mcp:` do config.yml vence o mcp.json, como em
    // `ConfigLoader::merged_mcp_config` (o que o boot spawna).
    let persistidas = crate::mcp::persistence::raizes_do_filesystem_efetivo(
        &state.config.mcp,
        &state.mcp_registry.config_snapshot().await,
    );
    checks.push(mcp_filesystem_root_check(
        politica.is_isolated_pod(),
        persistidas.as_deref(),
        raizes_mcp.caminhos(),
        &data_dir,
    ));
    // #1378: o workspace efetivo das file tools NATIVAS, que e outra coisa da
    // raiz do servidor MCP acima. Passa pela mesma funcao que o boot usa para
    // montar o jail, entao a linha nunca descreve um jail que o turno nao tem.
    let raizes_file_tools = crate::bootstrap::raizes_das_file_tools(&state.config);
    checks.push(files_workspace_check(
        raizes_file_tools.fonte,
        raizes_file_tools.jail.roots(),
        // #1449: no workspace padrao o jail nao tem raiz fixa — a raiz da
        // chamada e o subdiretorio da sessao. O que a linha mostra e o PAI.
        raizes_file_tools
            .workspace_por_sessao
            .as_ref()
            .map(|w| w.raiz()),
        &data_dir,
    ));
    // #1346: servidores MCP que falharam (inclusive no boot) e a versao do
    // `filesystem`.
    let bin = garraia_common::executavel::nome();
    let mcp_statuses = match &state.mcp_manager_arc {
        Some(mgr) => Some(mgr.server_statuses().await),
        None => None,
    };
    checks.push(mcp_servers_check(mcp_statuses.as_deref(), &bin));
    checks.push(mcp_filesystem_pinned_check(
        &crate::mcp::persistence::versao_do_filesystem_efetivo(
            &state.config.mcp,
            &state.mcp_registry.config_snapshot().await,
        ),
        &bin,
    ));

    // #1272: a tool `bash` existe neste gateway? Mesma decisao do boot.
    checks.push(tools_bash_check(&crate::bootstrap::exposicao_do_bash(
        politica.perfil,
        &crate::bootstrap::sandbox_policy_from(&state.config.agent.sandbox),
    )));

    // 4. .env presence (best-effort — env vars are loaded by the host shell,
    // but a `.env` file in CWD is the most common dev setup).
    //
    // #1437: `not_configured`, nao `warning`. Um gateway que recebe as envs do
    // shell pai (ou que nao precisa de nenhuma) esta certo sem `.env` — o
    // amarelo ali era ruido permanente numa instalacao local recem-feita. O
    // `next_step` fica: ele diz como LIGAR, nao como consertar.
    let dotenv = std::path::Path::new(".env").exists();
    checks.push(DiagnosticCheck {
        id: "env.dotenv",
        label: ".env file in CWD",
        status: if dotenv {
            CheckStatus::Ok
        } else {
            CheckStatus::NotConfigured
        },
        detail: if dotenv {
            ".env loaded".to_string()
        } else {
            "no .env in CWD (env vars must come from the parent shell)".to_string()
        },
        next_step: if dotenv {
            None
        } else {
            Some("Copy .env.example to .env and fill in the values you need.".to_string())
        },
    });

    // 5. Default provider active.
    let default_provider = state.agents.default_provider_id();
    checks.push(DiagnosticCheck {
        id: "provider.default",
        label: "Default LLM provider",
        status: if default_provider.is_some() {
            CheckStatus::Ok
        } else {
            CheckStatus::Error
        },
        detail: default_provider
            .clone()
            .unwrap_or_else(|| "none registered".to_string()),
        next_step: if default_provider.is_some() {
            None
        } else {
            Some(
                "Register at least one LLM provider via /api/providers POST or seed an API key in .env."
                    .to_string(),
            )
        },
    });

    // 6. Telegram configured (env var presence — never the value).
    let tg_configured =
        std::env::var("TELOXIDE_TOKEN").is_ok() || std::env::var("TELEGRAM_BOT_TOKEN").is_ok();
    checks.push(DiagnosticCheck {
        id: "channel.telegram",
        label: "Telegram channel",
        // #1437: um canal opcional sem token nao e "nao se aplica" — e um
        // subsistema real que esta instalacao nunca configurou.
        status: if tg_configured {
            CheckStatus::Ok
        } else {
            CheckStatus::NotConfigured
        },
        detail: if tg_configured {
            "TELOXIDE_TOKEN configured".to_string()
        } else {
            "optional channel — set TELOXIDE_TOKEN to enable".to_string()
        },
        next_step: None,
    });

    // 7. Discord configured.
    let dc_configured = std::env::var("DISCORD_TOKEN").is_ok();
    checks.push(DiagnosticCheck {
        id: "channel.discord",
        label: "Discord channel",
        status: if dc_configured {
            CheckStatus::Ok
        } else {
            CheckStatus::NotConfigured
        },
        detail: if dc_configured {
            "DISCORD_TOKEN configured".to_string()
        } else {
            "optional channel — set DISCORD_TOKEN to enable".to_string()
        },
        next_step: None,
    });

    // 8. JWT secret configured (presence only). Issue #824: the canonical
    // all-caps vault passphrase is the last fallback in AuthConfig::from_env,
    // so its presence also unlocks the auth flow.
    let jwt_configured = std::env::var("GARRAIA_JWT_SECRET").is_ok()
        || std::env::var("GarraIA_VAULT_PASSPHRASE").is_ok()
        || std::env::var("GARRAIA_VAULT_PASSPHRASE").is_ok();
    checks.push(DiagnosticCheck {
        id: "secrets.jwt",
        label: "JWT signing secret",
        // #1437: o proprio `next_step` abaixo diz que este secret e OPCIONAL
        // num gateway local single-user — e ainda assim a linha saia amarela
        // em toda instalacao que nunca quis auth. `not_configured` e o fato:
        // ninguem configurou. Quem LIGA auth descobre o problema pelos 503 do
        // `/v1/auth/*`, que continuam sendo o comportamento fail-closed.
        status: if jwt_configured {
            CheckStatus::Ok
        } else {
            CheckStatus::NotConfigured
        },
        detail: if jwt_configured {
            "configured (value masked)".to_string()
        } else {
            "missing — /v1/auth/* and /auth/* will return 503".to_string()
        },
        next_step: if jwt_configured {
            None
        } else {
            Some(
                "Optional on a local single-user gateway. To enable auth you need all \
                 four auth env vars plus Postgres, not this one alone; start with \
                 export GARRAIA_JWT_SECRET=$(openssl rand -hex 32). See docs/auth-config.md."
                    .to_string(),
            )
        },
    });

    // 9. Gateway exposed on 0.0.0.0 — security warning.
    let bind = state.config.gateway.host.as_str();
    let exposed = bind == "0.0.0.0" || bind == "::";
    checks.push(DiagnosticCheck {
        id: "security.bind",
        label: "Listener binding",
        status: if exposed {
            CheckStatus::Warning
        } else {
            CheckStatus::Ok
        },
        detail: format!("host={}", bind),
        next_step: if exposed {
            Some(
                "Binding to all interfaces — make sure a firewall protects the port or switch to 127.0.0.1."
                    .to_string(),
            )
        } else {
            None
        },
    });

    // 10. TLS — informational.
    let tls_on = state.config.gateway.tls_cert_path.is_some();
    checks.push(DiagnosticCheck {
        id: "security.tls",
        label: "TLS",
        // #1437: sem certificado configurado a linha e `not_configured` — o
        // gateway nao "pulou" a verificacao, simplesmente nao ha TLS montado.
        status: if tls_on {
            CheckStatus::Ok
        } else {
            CheckStatus::NotConfigured
        },
        detail: if tls_on {
            "TLS enabled".to_string()
        } else {
            "plain HTTP — fine for localhost, enable TLS for prod".to_string()
        },
        next_step: None,
    });

    // 11. Active channels.
    let channels: Vec<String> = state
        .channels
        .read()
        .await
        .list()
        .into_iter()
        .map(|s| s.to_string())
        .collect();
    checks.push(DiagnosticCheck {
        id: "runtime.channels",
        label: "Active channels",
        status: if channels.is_empty() {
            CheckStatus::Warning
        } else {
            CheckStatus::Ok
        },
        detail: if channels.is_empty() {
            "none".to_string()
        } else {
            channels.join(", ")
        },
        next_step: if channels.is_empty() {
            Some(
                "At least 'web' is expected. Check the bootstrap log for channel registration errors."
                    .to_string(),
            )
        } else {
            None
        },
    });

    // 12. Active sessions count.
    checks.push(DiagnosticCheck {
        id: "runtime.sessions",
        label: "Active sessions",
        status: CheckStatus::Ok,
        detail: format!("{} in-memory", state.sessions.len()),
        next_step: None,
    });

    // 13. TTS server reachable (#1098). Skipped when voice mode is off —
    //     there is nothing to reach, and nothing wrong, in that case.
    let voice_on = state.config.voice.enabled;
    let (tts_probe, stt_probe) = if voice_on {
        sondas_de_voz(&state.config.voice).await
    } else {
        (VoiceProbe::Disabled, VoiceProbe::Disabled)
    };
    let tts_endpoint = active_tts_endpoint(&state.config.voice);
    checks.push(voice_check(
        "voice.tts",
        "TTS server",
        tts_endpoint,
        TTS_NEXT_STEP,
        TTS_LOGS_STEP,
        tts_probe,
    ));

    // 14. WhatsApp vinculado (#1238, fatia D).
    //
    //     Le a MESMA fonte que o `garra whatsapp status` e que a linha
    //     `whatsapp_linked` do `/api/channels`: `whatsapp_linked::health::
    //     classify`. Duas fontes divergentes sobre o mesmo canal e o defeito
    //     que a #1079 ja custou uma vez.
    let (saude_wa, bridge_dir_wa) =
        crate::bootstrap::whatsapp_linked_health(&state.config, &state.whatsapp_linked);
    checks.push(whatsapp_linked_portao_vazio(
        whatsapp_linked_check((saude_wa, bridge_dir_wa)),
        saude_wa,
        // #1345: a config VIVA, a mesma que o turno le para admitir.
        &crate::bootstrap::whatsapp_linked_settings(&state.current_config()),
        state.whatsapp_linked.recusas_lid(),
        state.has_config_watcher(),
    ));

    // 15. STT server reachable (#1098).
    let stt_endpoint = state.config.voice.stt_endpoint.clone();
    checks.push(voice_check(
        "voice.stt",
        "STT server",
        &stt_endpoint,
        STT_NEXT_STEP,
        STT_LOGS_STEP,
        stt_probe,
    ));

    // Aggregate status: error > warning > ok. `skipped`, `disabled` e
    // `not_configured` sao neutros (#1437) — ver `status_agregado`.
    let status = status_agregado(&checks);

    Json(DiagnosticsReport {
        status,
        version: env!("CARGO_PKG_VERSION"),
        uptime_secs: state.boot_time.elapsed().as_secs(),
        generated_at: now_iso8601(),
        checks,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const ENDPOINT: &str = "http://127.0.0.1:7860";

    // ─── #1272: tools.bash ─────────────────────────────────────────────────

    #[test]
    fn tools_bash_desligado_e_warning_com_passo() {
        use crate::bootstrap::{ExposicaoDoBash, MotivoDoBashDesligado};
        let c = tools_bash_check(&ExposicaoDoBash::Desligado {
            motivo: MotivoDoBashDesligado::SandboxDesligado,
        });
        assert_eq!(c.id, "tools.bash");
        assert!(matches!(c.status, CheckStatus::Warning));
        let passo = c.next_step.expect("desligado precisa de passo");
        assert!(passo.contains("agent.sandbox"), "{passo}");
        assert!(passo.contains("execution.profile"), "{passo}");
        assert!(c.detail.contains("DESLIGADO"), "{}", c.detail);
    }

    #[test]
    fn tools_bash_sandbox_e_pod_sao_ok() {
        use crate::bootstrap::ExposicaoDoBash;
        for e in [
            ExposicaoDoBash::HostDoPod,
            ExposicaoDoBash::Sandbox {
                backend: garraia_agents::SandboxBackend::Podman,
            },
        ] {
            let c = tools_bash_check(&e);
            assert!(matches!(c.status, CheckStatus::Ok), "{e:?}");
            assert!(c.next_step.is_none());
        }
    }

    // ─── #1238: WhatsApp vinculado ────────────────────────────────────────

    use garraia_channels::whatsapp_linked::health::LinkHealth;

    fn wa(saude: LinkHealth) -> DiagnosticCheck {
        whatsapp_linked_check((
            saude,
            std::path::PathBuf::from("/home/ana/.garraia/data/whatsapp/bridge"),
        ))
    }

    /// Canal que ninguem ligou nao e defeito — e `not_configured` (#1437; era
    /// `skipped`). Mas o proximo passo existe: e como a pessoa liga.
    #[test]
    fn nao_vinculado_e_nao_configurado_com_o_comando_de_link() {
        let c = wa(LinkHealth::NotLinked);
        assert_eq!(c.status, CheckStatus::NotConfigured);
        // No harness o executavel e `garraia_gateway-<hash>`, que cai no
        // nome canonico: o passo cita `garraia`, nunca o alias fixo (#1329).
        assert_eq!(
            c.next_step.as_deref(),
            Some("rode `garraia whatsapp link`"),
            "sem sessao o passo e vincular"
        );
    }

    /// O modo de falha que a issue nomeia: ponte sem `node_modules`. O passo
    /// tem de citar o diretorio real — `npm ci` sem `cd` nao ajuda ninguem.
    #[test]
    fn sem_dependencias_e_error_com_o_npm_ci_no_diretorio_certo() {
        let c = wa(LinkHealth::MissingDependencies);
        assert!(matches!(c.status, CheckStatus::Error));
        let passo = c.next_step.expect("passo acionavel");
        assert!(passo.contains("npm ci"), "{passo}");
        assert!(
            passo.contains("/home/ana/.garraia/data/whatsapp/bridge"),
            "{passo}"
        );
    }

    /// Sessao vinculada e ponte caida e **defeito**, nao "opcional": alguem
    /// ligou o canal e ele nao esta funcionando.
    #[test]
    fn ponte_caida_com_sessao_e_error() {
        let c = wa(LinkHealth::BridgeDown);
        assert!(matches!(c.status, CheckStatus::Error));
        assert!(c.next_step.is_some(), "todo erro precisa de proximo passo");
    }

    #[test]
    fn conectado_e_ok_sem_proximo_passo() {
        for saude in [LinkHealth::Connected, LinkHealth::Linked] {
            let c = wa(saude);
            assert!(matches!(c.status, CheckStatus::Ok), "{saude:?}");
            assert!(c.next_step.is_none(), "{saude:?} nao tem o que consertar");
        }
    }

    /// #1345: vinculo saudavel com `allow` e `owners` vazios e `Warning`, com
    /// o comando que resolve — e sem numero nenhum no corpo auth-free.
    #[test]
    fn vinculo_saudavel_com_portao_vazio_e_warning_com_o_allow() {
        let ligado_vazio = crate::bootstrap::WhatsAppLinkedSettings {
            enabled: true,
            ..Default::default()
        };
        for saude in [LinkHealth::Connected, LinkHealth::Linked] {
            let c = acesso(saude, &ligado_vazio, 0);
            assert!(matches!(c.status, CheckStatus::Warning), "{saude:?}");
            let passo = c.next_step.as_deref().unwrap_or_default();
            assert!(passo.contains("whatsapp allow <numero>"), "{passo}");
            assert!(
                !c.detail.chars().any(|ch| ch.is_ascii_digit()),
                "{}",
                c.detail
            );
        }

        // Com alguem autorizado, segue `Ok` sem passo.
        let com_um = crate::bootstrap::WhatsAppLinkedSettings {
            enabled: true,
            allow: vec!["5511900000001".into()],
            ..Default::default()
        };
        let c = acesso(LinkHealth::Connected, &com_um, 0);
        assert!(matches!(c.status, CheckStatus::Ok));
        assert!(c.next_step.is_none());
        let json = serde_json::to_string(&c).expect("serializa");
        assert!(!json.contains("5511900000001"), "{json}");

        // Nao vinculado e ponte quebrada ficam com o veredito proprio.
        let c = acesso(LinkHealth::NotLinked, &ligado_vazio, 0);
        assert_eq!(c.status, CheckStatus::NotConfigured);
        let c = acesso(LinkHealth::BridgeDown, &ligado_vazio, 0);
        assert!(matches!(c.status, CheckStatus::Error));
        assert!(
            !c.next_step.unwrap_or_default().contains("allow"),
            "consertar a ponte vem antes"
        );

        // Canal desligado sem ponte deste processo: o portao vazio nao e o
        // problema.
        let c = acesso(
            LinkHealth::Linked,
            &crate::bootstrap::WhatsAppLinkedSettings::default(),
            0,
        );
        assert!(matches!(c.status, CheckStatus::Ok));
    }

    fn acesso(
        saude: LinkHealth,
        settings: &crate::bootstrap::WhatsAppLinkedSettings,
        recusas_lid: u64,
    ) -> DiagnosticCheck {
        whatsapp_linked_portao_vazio(wa(saude), saude, settings, recusas_lid, true)
    }

    /// Sem `ConfigWatcher` o `allow` nao recarrega: o passo manda reiniciar
    /// em vez de prometer "sem reiniciar" (review WHATSAPP-3/12).
    #[test]
    fn portao_vazio_sem_watcher_manda_reiniciar() {
        let ligado_vazio = crate::bootstrap::WhatsAppLinkedSettings {
            enabled: true,
            ..Default::default()
        };
        let c = whatsapp_linked_portao_vazio(
            wa(LinkHealth::Connected),
            LinkHealth::Connected,
            &ligado_vazio,
            0,
            false,
        );
        let passo = c.next_step.as_deref().unwrap_or_default();
        assert!(!passo.contains("sem reiniciar"), "{passo}");
        assert!(passo.contains("restart"), "{passo}");
        let c = acesso(LinkHealth::Connected, &ligado_vazio, 0);
        assert!(
            c.next_step
                .as_deref()
                .unwrap_or_default()
                .contains("sem reiniciar"),
            "{c:?}"
        );
    }

    /// #1345 (review WHATSAPP-10/14): a ponte do boot segue conectada, a config
    /// viva desligou o canal, e o turno recusa todo mundo. `Ok` "conectado"
    /// ali mentia.
    #[test]
    fn ponte_conectada_com_canal_desligado_na_config_viva_e_warning() {
        let desligado_com_gente = crate::bootstrap::WhatsAppLinkedSettings {
            enabled: false,
            allow: vec!["5511900000001".into()],
            ..Default::default()
        };
        let c = acesso(LinkHealth::Connected, &desligado_com_gente, 0);
        assert!(matches!(c.status, CheckStatus::Warning), "{c:?}");
        assert!(
            c.detail.contains("desligado na config viva"),
            "{}",
            c.detail
        );
        let passo = c.next_step.as_deref().unwrap_or_default();
        assert!(
            passo.contains("enabled: true") && passo.contains("restart"),
            "{passo}"
        );
        assert!(
            !serde_json::to_string(&c)
                .expect("json")
                .contains("5511900000001")
        );

        // Sem supervisor neste processo (`Linked`), desligado e so desligado.
        let c = acesso(LinkHealth::Linked, &desligado_com_gente, 0);
        assert!(matches!(c.status, CheckStatus::Ok), "{c:?}");
    }

    /// #1345: recusas de `@lid` sem numero aparecem no detalhe, como contagem,
    /// sem mudar o status nem o passo.
    #[test]
    fn recusas_de_lid_sem_numero_aparecem_no_detalhe_como_contagem() {
        let com_um = crate::bootstrap::WhatsAppLinkedSettings {
            enabled: true,
            allow: vec!["5511900000001".into()],
            ..Default::default()
        };
        let c = acesso(LinkHealth::Connected, &com_um, 3);
        assert!(matches!(c.status, CheckStatus::Ok), "{c:?}");
        assert!(
            c.detail.contains("3 mensagem(ns) de remetente @lid"),
            "{}",
            c.detail
        );
        assert!(c.detail.contains("whatsapp status"), "{}", c.detail);
        assert!(c.next_step.is_none());

        let c = acesso(LinkHealth::Connected, &com_um, 0);
        assert!(!c.detail.contains("@lid"), "{}", c.detail);
    }

    /// **A fiacao.** Os testes acima exercitam `whatsapp_linked_check`
    /// diretamente; sem este, apagar o `checks.push(...)` do handler deixaria
    /// todos eles verdes e o `/api/diagnostics` sem a linha — o padrao de
    /// defeito que este repositorio ja viu cinco vezes.
    #[tokio::test]
    #[serial_test::serial]
    async fn o_relatorio_de_verdade_inclui_a_linha_do_whatsapp() {
        let dir = tempfile::tempdir().expect("tempdir");
        let config = garraia_config::AppConfig {
            data_dir: Some(dir.path().to_path_buf()),
            ..Default::default()
        };
        let state: SharedState = std::sync::Arc::new(estado_no_config_dir(config, dir.path()));

        let Json(report) = diagnostics_handler(State(state)).await;
        let linha = report
            .checks
            .iter()
            .find(|c| c.id == "whatsapp.linked")
            .expect("o relatorio precisa carregar a linha `whatsapp.linked`");

        assert_eq!(
            linha.status,
            CheckStatus::NotConfigured,
            "sem sessao a linha e `not_configured` (#1437): {:?}",
            linha.status
        );
        assert_eq!(
            linha.next_step.as_deref(),
            Some("rode `garraia whatsapp link`")
        );
    }

    /// #1345, a fiacao: sem o `whatsapp_linked_portao_vazio` no handler, o
    /// teste de unidade acima continuaria verde e o relatorio diria `ok` para
    /// um canal que descarta toda mensagem.
    #[tokio::test]
    #[serial_test::serial]
    async fn o_relatorio_de_verdade_avisa_o_portao_vazio() {
        use garraia_channels::whatsapp_linked::{SessionBlob, SessionKey};

        let dir = tempfile::tempdir().expect("tempdir");
        let mut config = garraia_config::AppConfig {
            data_dir: Some(dir.path().to_path_buf()),
            ..Default::default()
        };
        config.channels.insert(
            "whatsapp_linked".into(),
            garraia_config::ChannelConfig {
                channel_type: "whatsapp_linked".into(),
                enabled: Some(true),
                settings: Default::default(),
            },
        );
        let paths = crate::bootstrap::LinkedPaths::from_config(&config).expect("paths");
        let key = SessionKey::resolve(paths.store.dir(), None).expect("chave");
        paths
            .store
            .save(&SessionBlob::new("eyJhIjoxfQ=="), &key)
            .expect("sessao");
        std::fs::create_dir_all(paths.bridge_dir.join("node_modules")).expect("deps");

        // Sem mexer em `GARRAIA_CONFIG_DIR` (#1346, causa do flake): o estado
        // aponta o config dir direto para o tempdir.
        let state: SharedState = std::sync::Arc::new(estado_no_config_dir(config, dir.path()));
        let Json(report) = diagnostics_handler(State(state)).await;
        let linha = report
            .checks
            .iter()
            .find(|c| c.id == "whatsapp.linked")
            .expect("linha whatsapp.linked");
        assert!(
            matches!(linha.status, CheckStatus::Warning),
            "vinculado, ligado e ninguem autorizado: {:?} {}",
            linha.status,
            linha.detail
        );
        assert!(
            linha
                .next_step
                .as_deref()
                .is_some_and(|p| p.contains("whatsapp allow")),
            "{:?}",
            linha.next_step
        );
    }

    /// O relatorio e auth-free: nada do material de sessao pode vazar para ele.
    #[test]
    fn a_linha_do_whatsapp_nao_carrega_material_de_sessao() {
        for saude in [
            LinkHealth::NotLinked,
            LinkHealth::MissingDependencies,
            LinkHealth::BridgeDown,
            LinkHealth::Connected,
            LinkHealth::Linked,
        ] {
            let c = wa(saude);
            let json = serde_json::to_string(&c).expect("serializa");
            for proibido in [
                "session.enc",
                "session.key",
                "whatsapp/default",
                "@s.whatsapp",
            ] {
                assert!(
                    !json.contains(proibido),
                    "{saude:?} vazou {proibido:?} num corpo auth-free: {json}"
                );
            }
        }
    }

    // ─── ADR 0024 (#1329): perfil de execucao + raiz do MCP filesystem ────

    use crate::bootstrap::PoliticaDeExecucao;
    use garraia_config::{ExecutionProfile, ProfileSource};
    use std::path::{Path, PathBuf};

    fn politica(perfil: ExecutionProfile, origem: ProfileSource) -> PoliticaDeExecucao {
        PoliticaDeExecucao {
            perfil,
            origem,
            pod_root: None,
        }
    }

    /// `standard` e `ok`, cita a fonte e nao tem o que consertar.
    #[test]
    fn perfil_standard_e_ok_com_a_fonte() {
        for origem in [
            ProfileSource::Default,
            ProfileSource::File,
            ProfileSource::Env,
        ] {
            let c = execution_profile_check(
                &politica(ExecutionProfile::Standard, origem),
                "search",
                0,
                &[PathBuf::from("/tmp/ws")],
                Path::new("/tmp/data"),
            );
            assert_eq!(c.id, "execution.profile");
            assert!(matches!(c.status, CheckStatus::Ok), "{origem}");
            assert!(c.detail.contains("standard"), "{}", c.detail);
            assert!(c.detail.contains(origem.as_str()), "{}", c.detail);
            assert!(c.next_step.is_none());
        }
    }

    /// `isolated-pod` e SEMPRE `warning`: o risco do ADR e o perfil ligado
    /// fora de um pod. O detalhe diz o que foi liberado (piso, contagem de
    /// donos, raiz) e o passo diz como reverter pelos dois caminhos.
    #[test]
    fn perfil_isolated_pod_e_warning_com_o_que_foi_liberado_e_como_reverter() {
        let c = execution_profile_check(
            &politica(ExecutionProfile::IsolatedPod, ProfileSource::Env),
            "code",
            2,
            &[PathBuf::from("/workspace")],
            Path::new("/tmp/data"),
        );
        assert!(matches!(c.status, CheckStatus::Warning));
        for esperado in ["isolated-pod", "env", "code", "2", "/workspace"] {
            assert!(c.detail.contains(esperado), "{esperado:?} em {}", c.detail);
        }

        // F-1: a raiz de politica dentro do `data_dir` sai relativa — a rota
        // e auth-free e o caminho absoluto do host nao acrescenta nada.
        let c = execution_profile_check(
            &politica(ExecutionProfile::IsolatedPod, ProfileSource::File),
            "code",
            1,
            &[PathBuf::from("/home/ana/.garraia/data/workspace")],
            Path::new("/home/ana/.garraia/data"),
        );
        assert!(c.detail.contains("<data_dir>/workspace"), "{}", c.detail);
        assert!(!c.detail.contains("/home/ana"), "{}", c.detail);
        let passo = c.next_step.expect("isolated-pod precisa de passo");
        assert!(passo.contains("execution.profile = standard"), "{passo}");
        assert!(passo.contains("GARRAIA_EXECUTION_PROFILE"), "{passo}");
        assert!(passo.contains("pod"), "{passo}");
    }

    /// Piso e contagem de donos vem da secao `channels.whatsapp_linked`; o
    /// piso default segue o perfil (`search`/`code`) como no turno; as
    /// identidades NUNCA saem — a rota e auth-free.
    #[test]
    fn piso_e_donos_saem_da_secao_sem_as_identidades() {
        let mut config = garraia_config::AppConfig::default();
        assert_eq!(
            piso_e_donos_do_whatsapp(&config, ExecutionProfile::Standard),
            ("search".to_string(), 0),
            "sem secao, standard: piso search e zero donos"
        );
        assert_eq!(
            piso_e_donos_do_whatsapp(&config, ExecutionProfile::IsolatedPod),
            ("code".to_string(), 0),
            "sem secao, isolated-pod: o dono teria piso code (mas ha zero donos)"
        );

        config.channels.insert(
            "whatsapp_linked".into(),
            garraia_config::ChannelConfig {
                channel_type: "whatsapp_linked".into(),
                enabled: Some(true),
                settings: std::collections::HashMap::from([
                    ("default_mode".to_string(), serde_json::json!(" search ")),
                    (
                        "owners".to_string(),
                        serde_json::json!(["+55 11 99999-8888", "abc@lid"]),
                    ),
                ]),
            },
        );
        let (piso, donos) = piso_e_donos_do_whatsapp(&config, ExecutionProfile::IsolatedPod);
        assert_eq!(
            piso, "search",
            "default_mode explicito vence o default do perfil"
        );
        assert_eq!(donos, 2);

        let c = execution_profile_check(
            &politica(ExecutionProfile::IsolatedPod, ProfileSource::File),
            &piso,
            donos,
            &[],
            Path::new("/tmp/data"),
        );
        let json = serde_json::to_string(&c).expect("serializa");
        for proibido in ["5511999998888", "99999-8888", "abc@lid", "@lid"] {
            assert!(!json.contains(proibido), "vazou {proibido:?}: {json}");
        }

        // Secao com `type` de outro canal nao e este canal.
        if let Some(ch) = config.channels.get_mut("whatsapp_linked") {
            ch.channel_type = "whatsapp".into();
        }
        assert_eq!(
            piso_e_donos_do_whatsapp(&config, ExecutionProfile::Standard),
            ("search".to_string(), 0)
        );
    }

    /// Sem entrada `filesystem` nao ha o que comparar: `skipped`.
    #[test]
    fn mcp_root_sem_entrada_e_skipped() {
        let c = mcp_filesystem_root_check(
            false,
            None,
            &[PathBuf::from("/tmp/ws")],
            Path::new("/tmp/data"),
        );
        assert_eq!(c.id, "mcp.filesystem_root");
        assert!(matches!(c.status, CheckStatus::Skipped));
        assert!(c.next_step.is_none());
    }

    /// O caso que a #1329 apontou: instalacao anterior com `$HOME` como raiz
    /// em `standard` — fora do jail, `warning`, nomeando a raiz e os dois
    /// caminhos de saida.
    #[test]
    fn mcp_root_fora_do_jail_em_standard_e_warning_nomeando_a_raiz() {
        let dir = tempfile::tempdir().expect("tempdir");
        let jail = dir.path().join("workspace");
        std::fs::create_dir_all(&jail).expect("mkdir");
        let home = dir.path().join("home-legada");
        std::fs::create_dir_all(&home).expect("mkdir");

        let c = mcp_filesystem_root_check(
            false,
            Some(std::slice::from_ref(&home)),
            std::slice::from_ref(&jail),
            dir.path(),
        );
        assert!(matches!(c.status, CheckStatus::Warning));
        assert!(
            c.detail.contains(&home.display().to_string()),
            "o detalhe nomeia a raiz ofensora como esta: {}",
            c.detail
        );
        // C1/C6/C14: o texto nomeia o que foi comparado — as raizes
        // declaradas — e nao "o jail", que e mais largo (env + working_dir).
        assert!(
            c.detail.contains("fora das raizes declaradas"),
            "{}",
            c.detail
        );
        assert!(!c.detail.contains("jail"), "{}", c.detail);
        // F-1: a raiz permitida dentro do data_dir sai relativa.
        assert!(c.detail.contains("<data_dir>/workspace"), "{}", c.detail);
        let passo = c.next_step.expect("warning precisa de passo");
        assert!(passo.contains("mcp.json"), "{passo}");
        assert!(
            passo.contains("GARRAIA_DISABLE_MCP_AUTOPROVISION=1"),
            "{passo}"
        );
        assert!(passo.contains("isolated-pod"), "{passo}");

        // Uma raiz dentro e outra fora: a fora e a que aparece.
        let dentro = jail.join("sub");
        std::fs::create_dir_all(&dentro).expect("mkdir");
        let c =
            mcp_filesystem_root_check(false, Some(&[dentro, home.clone()]), &[jail], dir.path());
        assert!(matches!(c.status, CheckStatus::Warning));
        assert!(
            c.detail.contains(&home.display().to_string()),
            "{}",
            c.detail
        );
    }

    /// Raiz dentro do jail (igual ou subdiretorio, em qualquer das
    /// permitidas) e `ok` — inclusive quando o diretorio ainda nao existe,
    /// que e o fallback lexico.
    #[test]
    fn mcp_root_dentro_do_jail_em_standard_e_ok() {
        let dir = tempfile::tempdir().expect("tempdir");
        let ws = dir.path().join("workspace");
        std::fs::create_dir_all(&ws).expect("mkdir");
        let outra = PathBuf::from("/srv/nao-existe/projeto");

        for raiz in [ws.clone(), ws.join("sub"), outra.join("fundo")] {
            let c = mcp_filesystem_root_check(
                false,
                Some(std::slice::from_ref(&raiz)),
                &[ws.clone(), outra.clone()],
                dir.path(),
            );
            assert!(
                matches!(c.status, CheckStatus::Ok),
                "{} deveria estar dentro: {}",
                raiz.display(),
                c.detail
            );
            assert!(c.next_step.is_none());
        }
    }

    /// Symlink que sai do jail nao passa por comparacao lexica: canonico
    /// quando os dois lados existem.
    #[cfg(unix)]
    #[test]
    fn mcp_root_symlink_para_fora_do_jail_e_warning() {
        let dir = tempfile::tempdir().expect("tempdir");
        let jail = dir.path().join("workspace");
        std::fs::create_dir_all(&jail).expect("mkdir");
        let fora = dir.path().join("fora");
        std::fs::create_dir_all(&fora).expect("mkdir");
        let link = jail.join("atalho");
        std::os::unix::fs::symlink(&fora, &link).expect("symlink");

        let c = mcp_filesystem_root_check(false, Some(&[link]), &[jail], dir.path());
        assert!(
            matches!(c.status, CheckStatus::Warning),
            "symlink para fora nao e 'dentro': {}",
            c.detail
        );
    }

    /// Em `isolated-pod` o pod e a fronteira: qualquer raiz e `ok`, e uma
    /// entrada sem raiz em `standard` e `warning`.
    #[test]
    fn mcp_root_em_isolated_pod_e_ok_e_entrada_vazia_em_standard_e_warning() {
        let c = mcp_filesystem_root_check(
            true,
            Some(&[PathBuf::from("/")]),
            &[PathBuf::from("/workspace")],
            Path::new("/tmp/data"),
        );
        assert!(matches!(c.status, CheckStatus::Ok), "{}", c.detail);
        assert!(c.detail.contains("isolated-pod"), "{}", c.detail);

        let c = mcp_filesystem_root_check(
            false,
            Some(&[]),
            &[PathBuf::from("/workspace")],
            Path::new("/tmp/data"),
        );
        assert!(matches!(c.status, CheckStatus::Warning), "{}", c.detail);
        assert!(c.next_step.is_some());
    }

    /// Review C4: o fallback lexico dobra `.`/`..`. `/srv/x/../../etc` nao
    /// "comeca com" `/srv/x` so porque `/srv/x` ainda nao existe — e um `..`
    /// que escapa da raiz do filesystem e sempre "fora".
    #[test]
    fn mcp_root_com_ponto_ponto_nao_passa_por_lexico() {
        let permitida = PathBuf::from("/srv/nao-existe-garra-1329");
        let escapa = permitida.join("..").join("..").join("etc");
        let c = mcp_filesystem_root_check(
            false,
            Some(std::slice::from_ref(&escapa)),
            std::slice::from_ref(&permitida),
            Path::new("/tmp/data"),
        );
        assert!(
            matches!(c.status, CheckStatus::Warning),
            "`..` que sai da permitida e fora: {}",
            c.detail
        );
        assert!(!dentro_de(&escapa, &permitida));

        // `..` que volta para dentro continua dentro; `.` e ignorado.
        let volta = permitida.join("sub").join("..").join(".").join("outro");
        assert!(dentro_de(&volta, &permitida));
        assert_eq!(normalizar_lexico(&volta), Some(permitida.join("outro")));

        // Alem da raiz do filesystem: sem forma comparavel, nunca "dentro".
        assert_eq!(normalizar_lexico(Path::new("/..")), None);
        assert!(!dentro_de(Path::new("/srv/../.."), Path::new("/")));
        // Relativo com `..` na frente tambem nao tem ancestral.
        assert_eq!(normalizar_lexico(Path::new("../x")), None);

        // Um diretorio ainda nao criado dentro de uma permitida que EXISTE
        // e comparado pelo canonico do prefixo que existe + o resto.
        let dir = tempfile::tempdir().expect("tempdir");
        let ws = dir.path().join("workspace");
        std::fs::create_dir_all(&ws).expect("mkdir");
        assert!(dentro_de(&ws.join("ainda-nao").join("existe"), &ws));
        assert!(!dentro_de(&ws.join("..").join("fora"), &ws));
    }

    /// F-1: caminhos de politica saem relativos a `<data_dir>` quando estao
    /// dentro dele; fora dele saem como estao.
    #[test]
    fn exibir_raiz_relativiza_so_o_que_esta_no_data_dir() {
        let data = Path::new("/home/ana/.garraia/data");
        assert_eq!(
            exibir_raiz(&data.join("workspace"), data),
            "<data_dir>/workspace"
        );
        assert_eq!(exibir_raiz(data, data), "<data_dir>");
        assert_eq!(exibir_raiz(Path::new("/srv/projeto"), data), "/srv/projeto");
        assert_eq!(
            lista_de_caminhos(&[data.join("workspace"), PathBuf::from("/srv/p")], data),
            "<data_dir>/workspace, /srv/p"
        );
        assert_eq!(lista_de_caminhos(&[], data), "(nenhuma)");
    }

    /// F-6 da auditoria: `AppState::new` provisiona `mcp.json` no config dir
    /// real quando ele nao existe. Os testes passam o config dir (um tempdir)
    /// direto, sem mexer em `GARRAIA_CONFIG_DIR`: apontar a env deixava
    /// qualquer outro teste que montasse um `AppState` em paralelo escrever o
    /// PROPRIO `mcp.json` no tempdir deste, e a linha `mcp.filesystem_root`
    /// virava aviso de vez em quando (flake do
    /// `o_relatorio_de_verdade_inclui_perfil_e_raiz_do_mcp`). `#[serial]`
    /// continua: e o lock das envs de provisionamento
    /// (`GARRAIA_DISABLE_MCP_AUTOPROVISION`, `HOME`) que os testes de
    /// `persistence` escrevem.
    fn estado_no_config_dir(
        config: garraia_config::AppConfig,
        config_dir: &Path,
    ) -> crate::state::AppState {
        crate::state::AppState::with_config_dir(
            config,
            std::sync::Arc::new(garraia_agents::AgentRuntime::new()),
            garraia_channels::ChannelRegistry::new(),
            config_dir,
        )
    }

    fn opt_out_de_provisionamento_ligado() -> bool {
        std::env::var_os(crate::mcp::McpPersistenceService::DISABLE_AUTOPROVISION_ENV)
            .is_some_and(|v| !v.is_empty() && v != "0")
    }

    /// **A fiacao.** As duas linhas precisam estar no relatorio de verdade;
    /// sem este teste apagar os `checks.push` deixaria os puros verdes.
    #[tokio::test]
    #[serial_test::serial]
    async fn o_relatorio_de_verdade_inclui_perfil_e_raiz_do_mcp() {
        let dir = tempfile::tempdir().expect("tempdir");
        let config = garraia_config::AppConfig {
            data_dir: Some(dir.path().to_path_buf()),
            ..Default::default()
        };
        let state: SharedState = std::sync::Arc::new(estado_no_config_dir(config, dir.path()));

        let Json(report) = diagnostics_handler(State(state)).await;
        let perfil = report
            .checks
            .iter()
            .find(|c| c.id == "execution.profile")
            .expect("linha `execution.profile`");
        assert!(matches!(perfil.status, CheckStatus::Ok), "{:?}", perfil);
        assert!(perfil.detail.contains("standard"), "{}", perfil.detail);

        let raiz = report
            .checks
            .iter()
            .find(|c| c.id == "mcp.filesystem_root")
            .expect("linha `mcp.filesystem_root`");
        assert!(
            !matches!(raiz.status, CheckStatus::Error),
            "raiz fora das raizes declaradas e aviso, nao erro: {raiz:?}"
        );
        // Com o config dir apontado para o tempdir o resultado e
        // deterministico: ou o opt-out esta ligado (CI) e nao ha entrada, ou
        // o boot provisionou o workspace default AQUI — nunca no config dir
        // real — e a raiz esta dentro das declaradas, exibida relativa.
        if opt_out_de_provisionamento_ligado() {
            assert!(matches!(raiz.status, CheckStatus::Skipped), "{raiz:?}");
            assert!(!dir.path().join("mcp.json").exists());
        } else {
            assert!(
                dir.path().join("mcp.json").exists(),
                "o provisionamento escreve no config dir de teste"
            );
            assert!(matches!(raiz.status, CheckStatus::Ok), "{raiz:?}");
            assert!(
                raiz.detail.contains("<data_dir>/workspace"),
                "{}",
                raiz.detail
            );
        }
    }

    /// #1098 + #1437: com o modo voz desligado nao ha servidor para alcancar, e
    /// isso nao e defeito — a linha e `disabled` (havia um interruptor e ele
    /// esta desligado), nunca `error` nem `warning`.
    #[test]
    fn voz_desligada_e_disabled_e_nao_erro() {
        let c = voice_check(
            "voice.tts",
            "TTS server",
            ENDPOINT,
            TTS_NEXT_STEP,
            TTS_LOGS_STEP,
            VoiceProbe::Disabled,
        );
        assert_eq!(c.status, CheckStatus::Disabled);
        assert!(
            c.next_step.is_none(),
            "desligado nao sugere proximo passo: nao ha nada a consertar"
        );
    }

    #[test]
    fn servidor_alcancavel_e_ok_sem_proximo_passo() {
        let c = voice_check(
            "voice.tts",
            "TTS server",
            ENDPOINT,
            TTS_NEXT_STEP,
            TTS_LOGS_STEP,
            VoiceProbe::Reachable,
        );
        assert!(matches!(c.status, CheckStatus::Ok));
        assert_eq!(c.detail, format!("reachable at {ENDPOINT}"));
        assert!(c.next_step.is_none());
    }

    /// O ponto da issue: servidor fora tem de ser `error` com o comando que
    /// levanta o servico — um `warning` era exatamente o que passava batido.
    #[test]
    fn servidor_fora_e_error_com_o_comando_de_subida() {
        let c = voice_check(
            "voice.tts",
            "TTS server",
            ENDPOINT,
            TTS_NEXT_STEP,
            TTS_LOGS_STEP,
            VoiceProbe::Unreachable("nothing listening (connection refused)"),
        );
        assert!(matches!(c.status, CheckStatus::Error));
        assert!(c.detail.contains(ENDPOINT));
        assert!(c.detail.contains("nothing listening"));
        assert_eq!(c.next_step.as_deref(), Some(TTS_NEXT_STEP));
        assert!(
            TTS_NEXT_STEP.contains("7860"),
            "o proximo passo do TTS tem de citar a porta certa"
        );
        assert!(STT_NEXT_STEP.contains("9090"), "e o STT a porta certa");
    }

    /// Regressao #1146: os dois next_step ja mandavam rodar `chatterbox-tts
    /// serve` e `fwsh serve`, comandos que nao existem em pacote nenhum. O
    /// console nao pode voltar a mandar o usuario para um beco sem saida.
    #[test]
    fn next_steps_nao_citam_comandos_inexistentes() {
        for fake in [
            "chatterbox-tts serve",
            "fwsh ",
            "faster-whisper-server serve",
        ] {
            assert!(!TTS_NEXT_STEP.contains(fake), "TTS cita {fake}");
            assert!(!STT_NEXT_STEP.contains(fake), "STT cita {fake}");
        }
    }

    /// Endpoint que nao e URL chamavel tambem e `error` — falha fechada, nunca
    /// se finge que esta tudo bem.
    #[test]
    fn endpoint_invalido_e_error() {
        let c = voice_check(
            "voice.stt",
            "STT server",
            "file:///etc/passwd",
            STT_NEXT_STEP,
            STT_LOGS_STEP,
            VoiceProbe::Invalid("scheme not allowed".to_string()),
        );
        assert!(matches!(c.status, CheckStatus::Error));
        assert!(c.next_step.is_some());
        assert!(
            c.detail.contains("<endpoint invalido>"),
            "detalhe nunca ecoa a URL rejeitada verbatim: {}",
            c.detail
        );
    }

    /// `hibiki` tem endpoint proprio; o default e o do chatterbox.
    #[test]
    fn provider_escolhe_o_endpoint() {
        let mut cfg = garraia_config::VoiceConfig::default();
        assert_eq!(active_tts_endpoint(&cfg), "http://127.0.0.1:7860");
        cfg.hibiki_endpoint = "http://127.0.0.1:8912".to_string();
        assert_eq!(
            active_tts_endpoint(&cfg),
            "http://127.0.0.1:7860",
            "sem trocar o provider, o endpoint do hibiki nao vale"
        );
        cfg.tts_provider = "hibiki".to_string();
        assert_eq!(active_tts_endpoint(&cfg), "http://127.0.0.1:8912");
    }

    /// O escopo privado e o que faz os servicos locais de voz alcancaveis; sem
    /// ele o proprio 127.0.0.1 do default seria barrado pelo guarda.
    #[test]
    fn a_policy_de_voz_permite_loopback() {
        assert_eq!(voice_policy().ip_scope, IpScope::AllowPrivate);
        assert!(
            ssrf::vet_url(ENDPOINT, &voice_policy()).is_ok(),
            "o endpoint default de voz tem de passar pelo vet_url"
        );
        assert!(
            ssrf::vet_url("http://169.254.169.254/", &voice_policy()).is_err(),
            "metadata de cloud continua barrado mesmo no escopo privado"
        );
    }

    /// FIX A (#SA-HIGH): userinfo embutida na config do dono jamais chega ao
    /// corpo do /api/diagnostics, que e auth-free sem a chave do gateway.
    #[test]
    fn credencial_na_url_nao_vaza_no_detail() {
        let c = voice_check(
            "voice.tts",
            "TTS server",
            "http://user:senha@127.0.0.1:7860",
            TTS_NEXT_STEP,
            TTS_LOGS_STEP,
            VoiceProbe::Reachable,
        );
        assert!(c.detail.contains("http://127.0.0.1:7860"), "{}", c.detail);
        assert!(!c.detail.contains("user"), "{}", c.detail);
        assert!(!c.detail.contains("senha"), "{}", c.detail);
    }

    /// Um dial a uma porta morta no loopback e recusado, nao timeout.
    #[tokio::test]
    async fn conexao_recusada_mapeia_para_unreachable() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        drop(listener); // porta livre de novo: nada escutando nela.
        let probe = probe_voice_endpoint(&format!("http://{addr}")).await;
        assert_eq!(
            probe,
            VoiceProbe::Unreachable("nothing listening (connection refused)"),
        );
    }

    /// Servidor HTTP de mentira num socket cru. LÊ o request antes de
    /// escrever a resposta: fechar o socket com o request ainda nao lido
    /// faz o kernel mandar RST, que destrói a resposta antes de o hyper
    /// conseguir le-la — o teste falharia com "request failed" sem ter
    /// provado nada sobre a semantica que se quer testar.
    fn drena_request_e_responde(sock: std::net::TcpStream, resposta: &'static [u8]) {
        use std::io::{Read, Write};
        let mut s = sock;
        let mut buf = [0u8; 2048];
        let mut got = 0usize;
        // Consome os cabeçalhos do request (GET não tem corpo).
        while got < buf.len() {
            let n = s.read(&mut buf[got..]).unwrap_or(0);
            if n == 0 {
                break;
            }
            got += n;
            if buf[..got].windows(4).any(|w| w == b"\r\n\r\n") {
                break;
            }
        }
        let _ = s.write_all(resposta);
        let _ = s.flush();
        // FIN limpo: nada ficou nao-lido no buffer de recepção.
        let _ = s.shutdown(std::net::Shutdown::Both);
    }

    /// Listener HTTP de mentira que conta quantas conexoes chegaram — nos
    /// testes de cache a prova e a CONTAGEM, nao a resposta.
    async fn contador_na_porta() -> (String, std::sync::Arc<std::sync::atomic::AtomicUsize>) {
        use std::sync::Arc;
        use std::sync::atomic::{AtomicUsize, Ordering};

        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let count = Arc::new(AtomicUsize::new(0));
        let c2 = count.clone();
        std::thread::spawn(move || {
            for sock in listener.incoming() {
                c2.fetch_add(1, Ordering::SeqCst);
                drena_request_e_responde(
                    sock.unwrap(),
                    b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                );
            }
        });
        (format!("http://{addr}"), count)
    }

    /// `VOICE_PROBE_CACHE` e um global de processo, e tests do mesmo binario
    /// rodam em paralelo por padrao: os dois testes que exercitam o cache se
    /// serializam por aqui, para um nao sobrescrever a entrada do outro na
    /// janela entre sonda e assert.
    static CACHE_TEST_GUARD: LazyLock<tokio::sync::Mutex<()>> =
        LazyLock::new(|| tokio::sync::Mutex::new(()));

    /// FIX B: 5xx significa "de pe, quebrado" — error, nao ok.
    #[tokio::test]
    async fn http_500_e_unhealthy() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let t = std::thread::spawn(move || {
            let (sock, _) = listener.accept().unwrap();
            drena_request_e_responde(
                sock,
                b"HTTP/1.1 500 Internal Server Error\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
            );
        });
        let probe = probe_voice_endpoint(&format!("http://{addr}")).await;
        t.join().unwrap();
        assert_eq!(probe, VoiceProbe::Unhealthy(500));
        let c = voice_check(
            "voice.tts",
            "TTS server",
            &format!("http://{addr}"),
            TTS_NEXT_STEP,
            TTS_LOGS_STEP,
            probe,
        );
        assert!(matches!(c.status, CheckStatus::Error));
        assert!(c.detail.contains("HTTP 500"), "{}", c.detail);
        // #1144: um 5xx aponta para os logs, nao para o comando de subida —
        // o processo esta de pe; restart seria a instrucao errada.
        assert_eq!(c.next_step.as_deref(), Some(TTS_LOGS_STEP));
        assert!(
            !TTS_LOGS_STEP.contains("chatterbox-tts serve"),
            "o passo do 5xx nao e o comando de subida"
        );
    }

    /// 4xx e servidor de pe e saudavel o bastante: GET / pode nao existir.
    #[tokio::test]
    async fn http_404_continua_reachable() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let t = std::thread::spawn(move || {
            let (sock, _) = listener.accept().unwrap();
            drena_request_e_responde(
                sock,
                b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
            );
        });
        let probe = probe_voice_endpoint(&format!("http://{addr}")).await;
        t.join().unwrap();
        assert_eq!(probe, VoiceProbe::Reachable);
    }

    /// Servidor aceita a conexao e nunca responde: estoura o budget de 1.5s.
    #[tokio::test]
    async fn sem_resposta_em_1_5s_e_timeout() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let t = std::thread::spawn(move || {
            let (sock, _) = listener.accept().unwrap();
            // Consome o request e fica calado: soltar o socket na hora
            // mandaria RST (request nao lido) e o timeout viraria
            // "request failed". Segura o socket alem do budget de 1.5s.
            let mut s = sock;
            let mut buf = [0u8; 2048];
            use std::io::Read;
            let _ = s.read(&mut buf);
            std::thread::sleep(std::time::Duration::from_secs(2));
        });
        let probe = probe_voice_endpoint(&format!("http://{addr}")).await;
        t.join().unwrap();
        assert_eq!(probe, VoiceProbe::Unreachable("no answer within 1.5s"));
    }

    /// FIX C, parte 2: dentro do TTL a segunda chamada nao reproba nenhum
    /// servidor — cada um dos dois listeners ve EXATAMENTE uma conexao.
    #[tokio::test]
    async fn sonda_repetida_dentro_do_ttl_nao_reproba() {
        use std::sync::atomic::Ordering;

        let _guard = CACHE_TEST_GUARD.lock().await;

        let (tts_endpoint, tts_count) = contador_na_porta().await;
        let (stt_endpoint, stt_count) = contador_na_porta().await;

        *VOICE_PROBE_CACHE.lock().await = None;
        let cfg = garraia_config::VoiceConfig {
            tts_endpoint: tts_endpoint.clone(),
            stt_endpoint: stt_endpoint.clone(),
            ..garraia_config::VoiceConfig::default()
        };

        let _ = sondas_de_voz(&cfg).await;
        let _ = sondas_de_voz(&cfg).await;

        assert_eq!(
            tts_count.load(Ordering::SeqCst),
            1,
            "TTS tem de ser sondado uma unica vez dentro do TTL"
        );
        assert_eq!(
            stt_count.load(Ordering::SeqCst),
            1,
            "STT tem de ser sondado uma unica vez dentro do TTL"
        );

        *VOICE_PROBE_CACHE.lock().await = None;
    }

    /// #1144 (CR r2, achado 1): o motivo de um veto sai de `motivo_do_veto`,
    /// montado campo a campo — jamais a URL crua, e muito menos a userinfo
    /// que ela carrega. O link-local e barrado mesmo no escopo privado e o
    /// veto carrega host+ip: se um dia um caminho descuidado ecoar a entrada,
    /// este teste e o que pega.
    #[tokio::test]
    async fn veto_com_userinfo_nao_vaza_no_reason() {
        let probe = probe_voice_endpoint("http://fulano:senha@169.254.169.254:80").await;
        let VoiceProbe::Invalid(reason) = &probe else {
            panic!("link-local e barrado mesmo no escopo privado: {probe:?}");
        };
        assert!(
            !reason.contains("fulano") && !reason.contains("senha"),
            "o motivo do veto nao pode ecoar a userinfo: {reason}"
        );

        let c = voice_check(
            "voice.tts",
            "TTS server",
            "http://fulano:senha@169.254.169.254:80",
            TTS_NEXT_STEP,
            TTS_LOGS_STEP,
            probe,
        );
        assert!(matches!(c.status, CheckStatus::Error));
        assert!(
            c.detail.contains("http://169.254.169.254"),
            "o detail mostra o endpoint publico (porta default e omitida pelo url): {}",
            c.detail
        );
        assert!(!c.detail.contains("fulano"), "{}", c.detail);
        assert!(!c.detail.contains("senha"), "{}", c.detail);

        // Userinfo num host permitido e porta morta: passa do veto e morre no
        // dial — e o motivo que sobe e a string estatica, nao o erro do reqwest.
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        drop(listener);
        let probe = probe_voice_endpoint(&format!("http://fulano:senha@{addr}")).await;
        assert_eq!(
            probe,
            VoiceProbe::Unreachable("nothing listening (connection refused)"),
            "loopback com userinfo passa do veto e morre no dial, com motivo estatico"
        );
    }

    /// #1144 (CR r2, achado 3): a sonda e single-flight. Sem o lock preso
    /// atravessando a sonda, N requests simultaneos numa janela de cache
    /// frio disparavam N pares de dials — exatamente o amplificador que o
    /// TTL existe para impedir. Cada listener ve EXATAMENTE uma conexao.
    #[tokio::test]
    async fn sonda_concorrente_nao_amplifica_dials() {
        use std::sync::atomic::Ordering;

        let _guard = CACHE_TEST_GUARD.lock().await;

        let (tts_endpoint, tts_count) = contador_na_porta().await;
        let (stt_endpoint, stt_count) = contador_na_porta().await;

        *VOICE_PROBE_CACHE.lock().await = None;
        let cfg = std::sync::Arc::new(garraia_config::VoiceConfig {
            tts_endpoint: tts_endpoint.clone(),
            stt_endpoint: stt_endpoint.clone(),
            ..garraia_config::VoiceConfig::default()
        });

        let mut set = tokio::task::JoinSet::new();
        for _ in 0..8 {
            let cfg = std::sync::Arc::clone(&cfg);
            set.spawn(async move {
                let (tts, stt) = sondas_de_voz(&cfg).await;
                assert_eq!(tts, VoiceProbe::Reachable);
                assert_eq!(stt, VoiceProbe::Reachable);
            });
        }
        while let Some(j) = set.join_next().await {
            j.unwrap();
        }

        assert_eq!(
            tts_count.load(Ordering::SeqCst),
            1,
            "8 chamadas concorrentes = 1 dial no TTS: single-flight"
        );
        assert_eq!(
            stt_count.load(Ordering::SeqCst),
            1,
            "8 chamadas concorrentes = 1 dial no STT: single-flight"
        );

        *VOICE_PROBE_CACHE.lock().await = None;
    }

    // ─── #1437: desligado / nao configurado / quebrado ────────────────────

    fn linha(id: &'static str, status: CheckStatus) -> DiagnosticCheck {
        DiagnosticCheck {
            id,
            label: id,
            status,
            detail: String::new(),
            next_step: None,
        }
    }

    /// O contrato JSON e **aditivo**: as quatro variantes originais mantem o
    /// nome serializado que qualquer cliente ja le, e as duas novas entram ao
    /// lado. Trocar `lowercase` por `snake_case` nao podia mexer em nenhuma
    /// das quatro — este teste e o que segura isso.
    #[test]
    fn o_vocabulario_serializado_e_aditivo() {
        for (status, esperado) in [
            (CheckStatus::Ok, "\"ok\""),
            (CheckStatus::Warning, "\"warning\""),
            (CheckStatus::Error, "\"error\""),
            (CheckStatus::Skipped, "\"skipped\""),
            (CheckStatus::Disabled, "\"disabled\""),
            (CheckStatus::NotConfigured, "\"not_configured\""),
        ] {
            let json = serde_json::to_string(&status).expect("serializa");
            assert_eq!(json, esperado, "{status:?}");
        }
    }

    /// O coracao da #1437: uma instalacao local que nunca ligou Postgres, nem
    /// storage S3, nem voz, nem canal nenhum tem varias linhas neutras — e o
    /// relatorio inteiro continua `ok`. `disabled` e `not_configured` sao
    /// pares de `skipped` para efeito de agregacao.
    #[test]
    fn os_tres_estados_neutros_nao_tiram_o_agregado_do_ok() {
        let checks = vec![
            linha("gateway.responds", CheckStatus::Ok),
            linha("workspace.postgres", CheckStatus::NotConfigured),
            linha("storage.s3", CheckStatus::NotConfigured),
            linha("secrets.jwt", CheckStatus::NotConfigured),
            linha("voice.tts", CheckStatus::Disabled),
            linha("voice.stt", CheckStatus::Disabled),
            linha("mcp.servers", CheckStatus::Skipped),
        ];
        assert_eq!(status_agregado(&checks), "ok");
        assert_eq!(status_agregado(&[]), "ok");
    }

    /// E o outro lado da mesma moeda: um subsistema que ESTA configurado e
    /// falha continua `error`, e continua pintando o agregado de vermelho.
    /// Amaciar "nao configurado" nao pode amaciar "quebrado".
    #[test]
    fn subsistema_configurado_e_quebrado_continua_error() {
        let quebrado = voice_check(
            "voice.tts",
            "TTS server",
            ENDPOINT,
            TTS_NEXT_STEP,
            TTS_LOGS_STEP,
            VoiceProbe::Unreachable("nothing listening (connection refused)"),
        );
        assert_eq!(quebrado.status, CheckStatus::Error);
        assert!(quebrado.next_step.is_some(), "todo erro tem proximo passo");

        let checks = vec![
            linha("secrets.jwt", CheckStatus::NotConfigured),
            linha("voice.stt", CheckStatus::Disabled),
            quebrado,
        ];
        assert_eq!(status_agregado(&checks), "error");

        // E o `warning` segue acima do `ok` e abaixo do `error`.
        assert_eq!(
            status_agregado(&[
                linha("a", CheckStatus::NotConfigured),
                linha("b", CheckStatus::Warning),
            ]),
            "warning"
        );
        assert_eq!(
            status_agregado(&[
                linha("a", CheckStatus::Warning),
                linha("b", CheckStatus::Error),
            ]),
            "error"
        );
    }

    /// **A fiacao.** Uma instalacao local single-user (SQLite, sem Postgres,
    /// sem S3, sem voz, sem TLS, sem aparelho vinculado) nao pode ganhar
    /// amarelo nem vermelho por nada disso no relatorio DE VERDADE — era
    /// exatamente o que a #1437 reporta. Os dois checks que dependem de env do
    /// host (`secrets.jwt`, `env.dotenv`, tokens de canal) sao aceitos como
    /// `ok` OU `not_configured`: o que este teste proibe e o amarelo.
    #[tokio::test]
    #[serial_test::serial]
    async fn ausencia_de_subsistema_opcional_nunca_pinta_o_relatorio() {
        let dir = tempfile::tempdir().expect("tempdir");
        let config = garraia_config::AppConfig {
            data_dir: Some(dir.path().to_path_buf()),
            ..Default::default()
        };
        let state: SharedState = std::sync::Arc::new(estado_no_config_dir(config, dir.path()));

        let Json(report) = diagnostics_handler(State(state)).await;
        let acha = |id: &str| {
            report
                .checks
                .iter()
                .find(|c| c.id == id)
                .unwrap_or_else(|| panic!("linha `{id}`"))
                .clone()
        };

        // Sem interruptor ligado: voz desligada e `disabled`, nao `error`.
        for id in ["voice.tts", "voice.stt"] {
            assert_eq!(acha(id).status, CheckStatus::Disabled, "{id}");
        }
        // Nunca configurados nesta instalacao.
        for id in ["security.tls", "whatsapp.linked"] {
            assert_eq!(acha(id).status, CheckStatus::NotConfigured, "{id}");
        }
        // Dependem de env do host: `ok` quando a env existe, `not_configured`
        // quando nao — nunca `warning`.
        for id in [
            "secrets.jwt",
            "env.dotenv",
            "channel.telegram",
            "channel.discord",
        ] {
            let c = acha(id);
            assert!(
                matches!(c.status, CheckStatus::Ok | CheckStatus::NotConfigured),
                "{id} nao pode pedir atencao por uma ausencia opcional: {:?}",
                c.status
            );
        }

        // E o vocabulario novo chega mesmo ao corpo da resposta.
        let json = serde_json::to_string(&report).expect("serializa");
        assert!(json.contains("\"not_configured\""), "{json}");
        assert!(json.contains("\"disabled\""), "{json}");
    }
}

#[cfg(test)]
mod tests_mcp_1346 {
    use super::*;
    use crate::mcp::persistence::{McpPersistenceService, VersaoDoFilesystem};
    use garraia_agents::{McpFailureCause, McpServerState, McpServerStatus};
    use std::path::PathBuf;

    fn st(name: &str, state: McpServerState, cause: Option<McpFailureCause>) -> McpServerStatus {
        McpServerStatus {
            name: name.into(),
            state,
            tool_count: if state == McpServerState::Connected {
                3
            } else {
                0
            },
            attempts: 5,
            max_restarts: 5,
            cause,
            last_error: None,
        }
    }

    #[test]
    fn sem_servidores_e_skipped() {
        assert!(matches!(
            mcp_servers_check(None, "garraia").status,
            CheckStatus::Skipped
        ));
        assert!(matches!(
            mcp_servers_check(Some(&[]), "garraia").status,
            CheckStatus::Skipped
        ));
    }

    #[test]
    fn todos_conectados_e_ok() {
        let c = mcp_servers_check(
            Some(&[st("filesystem", McpServerState::Connected, None)]),
            "garraia",
        );
        assert!(matches!(c.status, CheckStatus::Ok));
        assert!(c.next_step.is_none());
        assert_eq!(c.id, "mcp.servers");
    }

    #[test]
    fn cache_npx_corrompido_e_error_nomeando_o_diretorio() {
        let dir = PathBuf::from("/home/ana/.npm/_npx/0123456789abcdef");
        let c = mcp_servers_check(
            Some(&[
                st("github", McpServerState::Connected, None),
                st(
                    "filesystem",
                    McpServerState::Failed,
                    Some(McpFailureCause::NpxCacheCorrupt {
                        dir: Some(dir.clone()),
                    }),
                ),
            ]),
            "garraia",
        );
        assert!(matches!(c.status, CheckStatus::Error), "{c:?}");
        let passo = c.next_step.expect("next_step");
        assert!(
            passo.contains("/home/ana/.npm/_npx/0123456789abcdef"),
            "{passo}"
        );
        assert!(passo.contains("npm cache verify"), "{passo}");
        assert!(
            passo.contains("POST /admin/api/mcp/filesystem/restart"),
            "{passo}"
        );
        assert!(passo.contains("`garraia`"), "{passo}");
        assert!(c.detail.contains("filesystem failed"), "{}", c.detail);
        assert!(c.detail.contains("npx_cache_corrupt"), "{}", c.detail);
    }

    #[test]
    fn disco_cheio_e_error_mandando_liberar_espaco() {
        let c = mcp_servers_check(
            Some(&[st(
                "filesystem",
                McpServerState::Failed,
                Some(McpFailureCause::DiskFull),
            )]),
            "garra",
        );
        assert!(matches!(c.status, CheckStatus::Error));
        let passo = c.next_step.expect("next_step");
        assert!(
            passo.contains("ENOSPC") && passo.contains("Libere espaco"),
            "{passo}"
        );
        assert!(
            passo.contains("`garra`"),
            "o nome do binario instalado: {passo}"
        );
    }

    #[test]
    fn ainda_tentando_e_warning_com_passo() {
        let c = mcp_servers_check(
            Some(&[st(
                "x",
                McpServerState::Retrying,
                Some(McpFailureCause::Other),
            )]),
            "garraia",
        );
        assert!(matches!(c.status, CheckStatus::Warning));
        assert!(c.next_step.expect("passo").contains("/api/mcp/health"));
    }

    #[test]
    fn filesystem_sem_versao_e_warning_com_os_args_para_colar() {
        let c = mcp_filesystem_pinned_check(
            &VersaoDoFilesystem::SemVersao {
                args_sugeridos: vec![
                    "-y".into(),
                    McpPersistenceService::FILESYSTEM_PACKAGE_SPEC.into(),
                    "/srv".into(),
                ],
            },
            "garraia",
        );
        assert_eq!(c.id, "mcp.filesystem_pinned");
        assert!(matches!(c.status, CheckStatus::Warning));
        let passo = c.next_step.expect("passo");
        assert!(
            passo.contains(r#"["-y","@modelcontextprotocol/server-filesystem@2026.8.31","/srv"]"#),
            "{passo}"
        );
        assert!(passo.contains("`garraia`"), "{passo}");
    }

    #[test]
    fn filesystem_fixado_ou_fora_do_npx_e_ok_e_ausente_e_skipped() {
        for v in [
            VersaoDoFilesystem::Fixada("2026.8.31".into()),
            VersaoDoFilesystem::ForaDoNpx,
        ] {
            let c = mcp_filesystem_pinned_check(&v, "garraia");
            assert!(matches!(c.status, CheckStatus::Ok), "{v:?}");
            assert!(c.next_step.is_none());
        }
        let c = mcp_filesystem_pinned_check(&VersaoDoFilesystem::Ausente, "garraia");
        assert!(matches!(c.status, CheckStatus::Skipped));
    }

    /// **A fiacao**: as duas linhas novas estao no relatorio de verdade.
    #[tokio::test]
    #[serial_test::serial]
    async fn o_relatorio_de_verdade_inclui_as_linhas_do_1346() {
        use garraia_agents::{AgentRuntime, McpManager};
        use garraia_channels::ChannelRegistry;

        let dir = tempfile::tempdir().expect("tempdir");

        let mgr = std::sync::Arc::new(McpManager::new());
        let missing = dir
            .path()
            .join("nao-existe")
            .join("npx")
            .to_string_lossy()
            .into_owned();
        let args = vec!["-y".to_string(), "pacote".to_string()];
        let env = std::collections::HashMap::new();
        // max_restarts 0: ja nasce esgotado, como um servidor que gastou tudo.
        mgr.register_pending_stdio(
            "quebrado",
            &missing,
            &args,
            &env,
            5,
            vec![],
            None,
            0,
            0,
            false,
        )
        .await;

        let config = garraia_config::AppConfig {
            data_dir: Some(dir.path().to_path_buf()),
            ..Default::default()
        };
        let mut state = crate::state::AppState::with_config_dir(
            config,
            std::sync::Arc::new(AgentRuntime::new()),
            ChannelRegistry::new(),
            dir.path(),
        );
        state.mcp_manager_arc = Some(mgr);
        let Json(report) = diagnostics_handler(State(std::sync::Arc::new(state))).await;

        let servers = report
            .checks
            .iter()
            .find(|c| c.id == "mcp.servers")
            .expect("linha mcp.servers");
        assert!(matches!(servers.status, CheckStatus::Error), "{servers:?}");
        assert!(
            servers.detail.contains("quebrado failed"),
            "{}",
            servers.detail
        );
        assert!(
            report
                .checks
                .iter()
                .any(|c| c.id == "mcp.filesystem_pinned"),
            "linha mcp.filesystem_pinned"
        );
    }

    // ─── #1378: a linha `files.workspace` ─────────────────────────────────

    /// Raiz declarada pelo operador: `ok`, e o detalhe diz que a fonte foi a
    /// declaracao — nao o default.
    #[test]
    fn workspace_declarado_e_ok_e_nomeia_a_fonte() {
        let dir = tempfile::tempdir().expect("tempdir");
        let raiz = dir.path().join("projeto");
        let c = files_workspace_check(
            crate::bootstrap::FonteDasRaizesDasFileTools::Declaradas,
            &[raiz],
            // Raiz declarada nao tem escopo por sessao (#1449).
            None,
            dir.path(),
        );
        assert_eq!(c.id, "files.workspace");
        assert!(matches!(c.status, CheckStatus::Ok), "{c:?}");
        assert!(c.detail.contains("agent.file_roots"), "{}", c.detail);
        assert!(c.next_step.is_none(), "{c:?}");
    }

    /// Workspace padrao: `ok`, com o caminho relativo a `<data_dir>` — a rota
    /// e auth-free e nao precisa publicar o caminho absoluto do host.
    ///
    /// #1449: o jail nao tem raiz fixa nesta fonte (`raizes` chega vazio), e a
    /// linha descreve `<data_dir>/workspace/<sessao>` — nao o pai sozinho, que
    /// prometeria mais alcance do que o turno tem.
    #[test]
    fn workspace_padrao_e_ok_e_sai_relativo_ao_data_dir() {
        let dir = tempfile::tempdir().expect("tempdir");
        let ws = dir.path().join("workspace");
        let c = files_workspace_check(
            crate::bootstrap::FonteDasRaizesDasFileTools::WorkspacePadrao,
            &[],
            Some(&ws),
            dir.path(),
        );
        assert!(matches!(c.status, CheckStatus::Ok), "{c:?}");
        assert!(
            c.detail.contains("<data_dir>/workspace"),
            "o detalhe tem de sair relativo ao data_dir: {}",
            c.detail
        );
        assert!(
            c.detail.contains("<data_dir>/workspace/<sessao>"),
            "o detalhe tem de dizer que a raiz efetiva e por sessao (#1449): {}",
            c.detail
        );
        assert!(
            c.detail.contains("workspace padrao"),
            "o detalhe tem de dizer POR QUE a raiz e essa (#1378): {}",
            c.detail
        );
    }

    /// Sem raiz efetiva a linha e `warning` com passo acionavel: e o defeito
    /// da #1378 ainda de pe, e o operador precisa ver isso no console.
    #[test]
    fn sem_raiz_efetiva_a_linha_avisa_com_passo() {
        let dir = tempfile::tempdir().expect("tempdir");
        let c = files_workspace_check(
            crate::bootstrap::FonteDasRaizesDasFileTools::SomenteSessao,
            &[],
            None,
            dir.path(),
        );
        assert!(matches!(c.status, CheckStatus::Warning), "{c:?}");
        assert!(c.detail.contains("file_read"), "{}", c.detail);
        let passo = c.next_step.expect("a linha tem de dizer o que fazer");
        assert!(passo.contains("agent.file_roots"), "{passo}");
    }

    /// **A fiacao.** Sem este teste, apagar o `checks.push` deixaria os tres
    /// puros acima verdes e o `/api/diagnostics` sem a linha. E ele descreve a
    /// instalacao limpa da #1378 de ponta a ponta: o boot prepara o workspace,
    /// e o console reporta esse workspace e a razao dele.
    #[tokio::test]
    #[serial_test::serial]
    async fn o_relatorio_de_verdade_inclui_a_linha_do_workspace() {
        let dir = tempfile::tempdir().expect("tempdir");
        let config = garraia_config::AppConfig {
            data_dir: Some(dir.path().to_path_buf()),
            ..Default::default()
        };
        // O passo que o `server.rs` da na subida, antes de montar o runtime.
        crate::bootstrap::garantir_workspace_padrao(&config).expect("workspace padrao");
        let state: SharedState = std::sync::Arc::new(crate::state::AppState::with_config_dir(
            config,
            std::sync::Arc::new(garraia_agents::AgentRuntime::new()),
            garraia_channels::ChannelRegistry::new(),
            dir.path(),
        ));

        let Json(report) = diagnostics_handler(State(state)).await;
        let linha = report
            .checks
            .iter()
            .find(|c| c.id == "files.workspace")
            .expect("o relatorio precisa carregar a linha `files.workspace` (#1378)");
        assert!(
            matches!(linha.status, CheckStatus::Ok),
            "instalacao limpa com o workspace preparado e `ok`: {linha:?}"
        );
        assert!(
            linha.detail.contains("workspace"),
            "a linha tem de nomear o workspace efetivo: {}",
            linha.detail
        );
        assert!(
            linha.detail.contains("workspace padrao"),
            "a linha tem de dizer de onde veio a decisao: {}",
            linha.detail
        );
    }

    /// **F-1 da #1329, o caso que escapou.** `/api/diagnostics` e auth-free, e
    /// a linha do workspace promete sair relativa (`<data_dir>/…`).
    ///
    /// A relativizacao e um `strip_prefix` do `data_dir` cru da config contra
    /// raizes que o `FileJail` ja canonicalizou. Quando o `data_dir` passa por
    /// symlink (ou e relativo), os dois lados deixam de casar, o `strip_prefix`
    /// falha em silencio e o fallback imprime o caminho ABSOLUTO do host para
    /// qualquer um que chame a rota.
    #[tokio::test]
    #[serial_test::serial]
    #[cfg(unix)]
    async fn data_dir_com_symlink_nao_vaza_caminho_do_host() {
        let dir = tempfile::tempdir().expect("tempdir");
        let real = dir.path().join("data-real");
        std::fs::create_dir_all(&real).expect("cria o data dir real");
        let link = dir.path().join("data-link");
        std::os::unix::fs::symlink(&real, &link).expect("planta o link");

        // O operador configurou o caminho COM o link; o jail vai canonicalizar.
        let config = garraia_config::AppConfig {
            data_dir: Some(link),
            ..Default::default()
        };
        crate::bootstrap::garantir_workspace_padrao(&config).expect("workspace padrao");
        let state: SharedState = std::sync::Arc::new(crate::state::AppState::with_config_dir(
            config,
            std::sync::Arc::new(garraia_agents::AgentRuntime::new()),
            garraia_channels::ChannelRegistry::new(),
            dir.path(),
        ));

        let Json(report) = diagnostics_handler(State(state)).await;
        let linha = report
            .checks
            .iter()
            .find(|c| c.id == "files.workspace")
            .expect("o relatorio precisa carregar a linha `files.workspace` (#1378)");

        let real_canonico = std::fs::canonicalize(&real).expect("canonicalize do data dir real");
        assert!(
            !linha.detail.contains(&real_canonico.display().to_string()),
            "a rota auth-free vazou o caminho absoluto do host: {}",
            linha.detail
        );
        assert!(
            linha.detail.contains("<data_dir>"),
            "a raiz dentro do data dir tem de sair relativa: {}",
            linha.detail
        );
    }
}
