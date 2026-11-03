//! Assistente de Primeiro Uso (onboarding) do desktop (#1636, fatia 3).
//!
//! A casca aqui e **fina de proposito**: toda decisao mora em
//! [`garraia_desktop_core::onboarding`] (puro, testado no CI — ver ADR 0021). Este
//! modulo so faz o que precisa de janela, processo e disco:
//!
//! - lanca o sidecar `garraia whatsapp link --machine` (fatia 1) e transmite
//!   cada linha NDJSON do stdout para a webview como evento Tauri `wa://ndjson`;
//! - cancela o pareamento matando o processo, com a mesma contencao de orfao do
//!   gateway (Job Object no Windows — fatia D);
//! - aplica a politica de acesso do modo (`whatsapp access open --yes` /
//!   `restricted`) rodando o sidecar, e so entao reinicia o gateway;
//! - grava a persona em `agent.system_prompt` pelo writer de config que o
//!   projeto ja usa (`ConfigLoader::set_agent_system_prompt`, testado no CI);
//! - persiste `onboarding.json` em `app_config_dir()` (marcador de primeiro uso)
//!   e decide se o wizard abre.
//!
//! O QR nunca chega aqui como payload: o sidecar emite so a MATRIZ (`modules`,
//! linhas de `0`/`1`) e a webview a rasteriza num canvas. A credencial fica no
//! pipe local entre processos irmaos, nunca na webview nem no `/api/*`.

use std::sync::Mutex;

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, WebviewUrl, WebviewWindowBuilder};
use tauri_plugin_shell::ShellExt;
use tauri_plugin_shell::process::{CommandChild, CommandEvent};

use garraia_desktop_core::onboarding as core;
use garraia_desktop_core::provedor::{self, Provedor};
use garraia_desktop_core::{EstiloPersona, Idioma, Modo, OnboardingPersistido, Personalidade};

/// Label da janela do wizard. Tem de estar em `capabilities/default.json` para a
/// webview poder chamar os comandos.
pub const LABEL: &str = "onboarding";

/// Evento Tauri que carrega cada linha NDJSON do sidecar para a webview.
const EVENTO_NDJSON: &str = "wa://ndjson";
/// Evento Tauri emitido quando o sidecar de pareamento termina (`code`).
const EVENTO_FIM: &str = "wa://fim";
/// Arquivo que o wizard grava ao aparecer — prova, sem celular, de que a tela
/// de primeiro uso abriu (roteiro manual e smoke do `desktop.yml`).
const MARCADOR_EXIBIDO: &str = "onboarding-shown.marker";

/// Estado gerenciado: o processo de pareamento em andamento, para o cancelar
/// poder mata-lo. `app.manage(OnboardingState::default())` antes de usar.
#[derive(Default)]
pub struct OnboardingState {
    pareamento: Mutex<Option<CommandChild>>,
}

/// O que a webview le para decidir a tela inicial e o que settings mostra.
#[derive(Debug, Serialize)]
pub struct OnboardingEstado {
    pub primeiro_uso: bool,
    /// `"proprio"` | `"dedicado"` | `null` (serde snake_case de [`Modo`]).
    pub modo: Option<Modo>,
    pub concluido: bool,
}

// ── Persistencia de onboarding.json (app_config_dir) ────────────────────────

fn caminho_persistido(app: &AppHandle) -> Option<std::path::PathBuf> {
    app.path()
        .app_config_dir()
        .ok()
        .map(|d| core::arquivo_onboarding(&d))
}

fn ler_persistido(app: &AppHandle) -> Option<OnboardingPersistido> {
    let raw = std::fs::read_to_string(caminho_persistido(app)?).ok()?;
    serde_json::from_str(&raw).ok()
}

fn gravar_persistido(app: &AppHandle, estado: &OnboardingPersistido) -> Result<(), String> {
    let path = caminho_persistido(app).ok_or("sem app_config_dir")?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("criar {}: {e}", parent.display()))?;
    }
    let json = serde_json::to_string_pretty(estado).map_err(|e| format!("serializar: {e}"))?;
    std::fs::write(&path, json).map_err(|e| format!("gravar {}: {e}", path.display()))
}

// ── Janela do wizard ────────────────────────────────────────────────────────

/// Abre (ou foca, se ja existir) a janela do wizard. Usado no primeiro uso e no
/// "refazer pareamento" de settings.
pub fn abrir(app: &AppHandle) -> tauri::Result<()> {
    if let Some(win) = app.get_webview_window(LABEL) {
        let _ = win.show();
        let _ = win.set_focus();
        return Ok(());
    }
    WebviewWindowBuilder::new(app, LABEL, WebviewUrl::App("onboarding.html".into()))
        .title("Assistente do Garra")
        .inner_size(480.0, 720.0)
        .min_inner_size(420.0, 600.0)
        .resizable(true)
        .center()
        .focused(true)
        .build()?;
    Ok(())
}

/// Abre o wizard **so no primeiro uso** (sem `onboarding.json` com
/// `concluido: true`). Chamado uma vez no `setup`. Falha nunca derruba o app —
/// o pior caso e o wizard nao abrir, e o usuario ainda tem a CLI.
pub fn abrir_se_primeiro_uso(app: &AppHandle) {
    let persistido = ler_persistido(app);
    if core::primeiro_uso(persistido.as_ref())
        && let Err(e) = abrir(app)
    {
        eprintln!("[garra] onboarding window failed: {e}");
    }
}

fn cancelar_pareamento(app: &AppHandle) {
    if let Some(st) = app.try_state::<OnboardingState>()
        && let Ok(mut guard) = st.pareamento.lock()
        && let Some(child) = guard.take()
    {
        let _ = child.kill();
    }
}

// ── Comandos ────────────────────────────────────────────────────────────────

/// Estado do onboarding para a webview (primeira tela / settings).
#[tauri::command]
pub async fn onboarding_estado(app: AppHandle) -> OnboardingEstado {
    let persistido = ler_persistido(&app);
    OnboardingEstado {
        primeiro_uso: core::primeiro_uso(persistido.as_ref()),
        modo: persistido.as_ref().and_then(|p| p.modo),
        concluido: persistido.map(|p| p.concluido).unwrap_or(false),
    }
}

/// Grava o marcador "a tela apareceu" (chamado no load da webview). Best-effort.
#[tauri::command]
pub async fn onboarding_marcar_exibido(app: AppHandle) {
    if let Ok(dir) = app.path().app_config_dir() {
        let _ = std::fs::create_dir_all(&dir);
        let _ = std::fs::write(dir.join(MARCADOR_EXIBIDO), b"1");
    }
}

/// O config do operador tem algum provedor de LLM (`llm:` nao vazio)? A tela
/// "Pronto" nao pode prometer conversa sem um — o doctor do WhatsApp ja julga
/// esse caso como "o WhatsApp recebe e nao responde" (doctor/tabela.rs,
/// `c.provedores.is_empty()`), e o mesmo config é o que o gateway le. Erro de
/// leitura conta como sem provedor: fail-closed para o proposito de avisar.
/// Nunca devolve chave, nome de modelo ou caminho — so o booleano.
#[tauri::command]
pub async fn onboarding_provedor_configurado() -> bool {
    garraia_config::ConfigLoader::new()
        .ok()
        .and_then(|l| l.load().ok())
        .map(|c| !c.llm.is_empty())
        .unwrap_or(false)
}

// ── "Quem responde": provedor de LLM sem terminal (#1700) ──────────────────

/// O que a tela "Quem responde" precisa saber para abrir. Nunca carrega
/// chave: `configurado` diz so se ha um provedor padrao no config.
#[derive(Debug, Serialize)]
pub struct ProvedorStatus {
    pub configurado: bool,
    pub provedor: Option<String>,
    pub modelo: Option<String>,
    /// O modelo que a tela sugere para o OpenRouter (o mesmo do `garraia init`).
    pub modelo_padrao_openrouter: String,
    pub ollama_disponivel: bool,
    pub ollama_modelos: Vec<String>,
}

/// Estado do provedor e o que ha de local para oferecer.
#[tauri::command]
pub async fn onboarding_provedor_status() -> ProvedorStatus {
    let config = garraia_config::ConfigLoader::new()
        .ok()
        .and_then(|l| l.load().ok());
    let padrao = config
        .as_ref()
        .and_then(|c| c.agent.default_provider.clone())
        .filter(|p| config.as_ref().is_some_and(|c| c.llm.contains_key(p)));
    let modelo = padrao.as_ref().and_then(|p| {
        config
            .as_ref()
            .and_then(|c| c.llm.get(p))
            .and_then(|e| e.model.clone())
    });
    let ollama = tauri::async_runtime::spawn_blocking(|| {
        provedor::detectar_ollama(std::time::Duration::from_millis(1500))
    })
    .await
    .ok()
    .flatten();
    ProvedorStatus {
        configurado: padrao.is_some(),
        provedor: padrao,
        modelo,
        modelo_padrao_openrouter: garraia_config::defaults::DEFAULT_CLOUD_MODEL.to_string(),
        ollama_disponivel: ollama.is_some(),
        ollama_modelos: ollama.unwrap_or_default(),
    }
}

/// Grava o provedor escolhido pelo `garraia config set-routing` do sidecar —
/// o mesmo writer da CLI (arquivo 0600). A chave vai pelo **stdin**
/// (`--api-key-stdin`), nunca em argumento de processo, e nao volta para a
/// webview nem aparece em erro.
#[tauri::command]
pub async fn onboarding_configurar_provedor(
    app: AppHandle,
    window: tauri::WebviewWindow,
    provedor: Provedor,
    modelo: String,
    chave: Option<String>,
) -> Result<(), String> {
    // Zerada na memoria ao sair de escopo, inclusive no caminho de erro.
    let chave = chave.map(provedor::Zeroizing::new);
    so_da_janela_do_assistente(&window)?;
    let escolha = provedor::validar_escolha(provedor, &modelo, chave.as_ref().map(|c| c.as_str()))?;
    drop(chave);
    let (code, _saida) =
        rodar_sidecar_capturando(&app, escolha.args_set_routing(), escolha.stdin()).await?;
    if code == Some(0) {
        Ok(())
    } else {
        Err(format!(
            "nao consegui gravar o provedor (codigo {code:?}). Confira se o config.yml esta legivel."
        ))
    }
}

/// O teste de conectividade de verdade: `garraia ask --json` com o provedor
/// padrao. So volta se conversou, a latencia e, se nao, motivo e dica.
#[tauri::command]
pub async fn onboarding_testar_provedor(
    app: AppHandle,
    window: tauri::WebviewWindow,
) -> provedor::TesteProvedor {
    if let Err(e) = so_da_janela_do_assistente(&window) {
        return provedor::interpretar_teste(&e, None);
    }
    match rodar_sidecar_capturando(&app, provedor::args_teste(), None).await {
        Ok((code, saida)) => provedor::interpretar_teste(&saida, code),
        Err(e) => provedor::interpretar_teste(&e, None),
    }
}

/// Revisao de seguranca (#1821): no Tauri v2 todo comando do app vale para
/// toda janela local. Gravar a chave do provedor e rodar o teste so fazem
/// sentido no assistente — uma falha em outra tela (chat bar, overlay) nao
/// pode trocar a chave por uma de outra conta.
fn so_da_janela_do_assistente(window: &tauri::WebviewWindow) -> Result<(), String> {
    if window.label() == LABEL {
        Ok(())
    } else {
        Err("este comando so roda no assistente de primeiro uso".to_string())
    }
}

/// O que o gateway que esta atendendo diz no `/api/health` (#1700, revisao
/// M2): "pronto para conversar" exige que ELE tenha registrado o provedor —
/// nao basta o `config.yml` estar certo.
#[derive(Debug, Serialize)]
pub struct GatewaySaude {
    pub respondeu: bool,
    pub versao: Option<String>,
    pub provedor: Option<String>,
}

#[tauri::command]
pub async fn gateway_saude() -> GatewaySaude {
    let porta = garraia_config::bind::endereco_do_cliente().1;
    let h = tauri::async_runtime::spawn_blocking(move || {
        provedor::health_na_porta(porta, std::time::Duration::from_millis(1500))
    })
    .await
    .ok()
    .flatten();
    GatewaySaude {
        respondeu: h.is_some(),
        versao: h.as_ref().map(|h| h.versao.clone()),
        provedor: h.and_then(|h| h.provedor),
    }
}

/// De onde vem o gateway em uso (#1816): o do app, ou um de fora.
#[derive(Debug, Serialize)]
pub struct GatewayEstado {
    #[serde(flatten)]
    pub origem: Option<crate::gateway::Origem>,
    pub versao_app: String,
}

#[tauri::command]
pub async fn gateway_estado(app: AppHandle) -> GatewayEstado {
    GatewayEstado {
        origem: crate::gateway::origem(),
        versao_app: app.package_info().version.to_string(),
    }
}

/// Roda o sidecar ate o fim, escrevendo `stdin` (se houver) e juntando o
/// stdout. O stdout so serve ao chamador (o envelope do `ask`); nada aqui e
/// logado.
async fn rodar_sidecar_capturando(
    app: &AppHandle,
    args: Vec<String>,
    stdin: Option<Vec<u8>>,
) -> Result<(Option<i32>, String), String> {
    let subcomando = args.iter().take(2).cloned().collect::<Vec<_>>().join(" ");
    let (mut rx, mut child) = app
        .shell()
        .sidecar("garraia")
        .map_err(|e| format!("sidecar garraia ausente: {e}"))?
        .args(args)
        .spawn()
        .map_err(|e| format!("falha ao rodar `garraia {subcomando}`: {e}"))?;
    if let Some(bytes) = stdin {
        child
            .write(&bytes)
            .map_err(|_| format!("falha ao entregar a chave ao `garraia {subcomando}`"))?;
    }
    let mut saida = String::new();
    let mut code = None;
    while let Some(event) = rx.recv().await {
        match event {
            CommandEvent::Stdout(bytes) => {
                saida.push_str(&String::from_utf8_lossy(&bytes));
                saida.push('\n');
            }
            CommandEvent::Terminated(payload) => {
                code = payload.code;
                break;
            }
            _ => {}
        }
    }
    Ok((code, saida))
}

/// Inicia o pareamento: lanca `garraia whatsapp link --machine` e transmite cada
/// linha NDJSON do stdout como evento `wa://ndjson`. Idempotente: um pareamento
/// anterior e substituido e morto na troca.
#[tauri::command]
pub async fn wa_iniciar_pareamento(app: AppHandle) -> Result<(), String> {
    let (mut rx, child) = app
        .shell()
        .sidecar("garraia")
        .map_err(|e| format!("sidecar garraia ausente: {e}"))?
        .args(core::ARGS_PAREAMENTO)
        .spawn()
        .map_err(|e| format!("falha ao iniciar o pareamento: {e}"))?;

    // Fatia D: no Windows, contem a arvore (sidecar + ponte Node) no mesmo Job
    // Object do gateway — o `link` nao aceita `--exit-with-parent`, entao se o
    // app morrer de forma abrupta durante o pareamento o SO mata a arvore ao
    // fechar o handle do job. No Unix a ponte Node ja morre com o sidecar.
    #[cfg(windows)]
    crate::gateway::contain_in_kill_on_close_job(&child);

    // F2: guarda o novo child e mata o anterior NA TROCA, sob o lock — sem a
    // janela do cancelar-depois-spawnar-depois-guardar. O `pid` identifica ESTE
    // pareamento para o ramo `Terminated` limpar so a si mesmo.
    let pid = child.pid();
    if let Some(st) = app.try_state::<OnboardingState>()
        && let Ok(mut guard) = st.pareamento.lock()
        && let Some(antigo) = guard.replace(child)
    {
        let _ = antigo.kill();
    }

    let app_ev = app.clone();
    tauri::async_runtime::spawn(async move {
        while let Some(event) = rx.recv().await {
            match event {
                CommandEvent::Stdout(bytes) => {
                    // Defensivo: o plugin ja entrega por linha, mas um chunk com
                    // varias linhas nao pode virar um emit so — o consumidor le
                    // uma linha por evento.
                    if let Ok(texto) = String::from_utf8(bytes) {
                        for linha in texto.split('\n') {
                            let linha = linha.trim();
                            if !linha.is_empty() {
                                let _ = app_ev.emit(EVENTO_NDJSON, linha);
                            }
                        }
                    }
                }
                CommandEvent::Terminated(payload) => {
                    let _ = app_ev.emit(EVENTO_FIM, payload.code);
                    // F2: limpa o handle SE ele ainda for este pareamento — um
                    // novo spawn pode te-lo substituido no meio tempo.
                    if let Some(st) = app_ev.try_state::<OnboardingState>()
                        && let Ok(mut guard) = st.pareamento.lock()
                        && guard.as_ref().map(|c| c.pid()) == Some(pid)
                    {
                        *guard = None;
                    }
                    break;
                }
                _ => {}
            }
        }
    });
    Ok(())
}

/// Roda o sidecar `garraia` com `args` ate o fim e falha se o exit nao for 0.
async fn rodar_sidecar(app: &AppHandle, args: Vec<String>) -> Result<(), String> {
    let descr = args.join(" ");
    let (mut rx, _child) = app
        .shell()
        .sidecar("garraia")
        .map_err(|e| format!("sidecar garraia ausente: {e}"))?
        .args(args)
        .spawn()
        .map_err(|e| format!("falha ao rodar `garraia {descr}`: {e}"))?;
    let mut code = None;
    while let Some(event) = rx.recv().await {
        if let CommandEvent::Terminated(payload) = event {
            code = payload.code;
            break;
        }
    }
    if code == Some(0) {
        Ok(())
    } else {
        Err(format!(
            "`garraia {descr}` terminou com codigo {code:?} (esperado 0)"
        ))
    }
}

/// Valida e normaliza o numero pessoal (modo dedicado) ANTES do pareamento,
/// para a tela recusar um numero invalido na hora. Devolve `+<digitos>`.
#[tauri::command]
pub async fn onboarding_validar_numero(numero: String) -> Result<String, String> {
    core::normalizar_numero(&numero)
}

/// Cancela o pareamento em andamento (botao voltar / fechar a janela do QR).
#[tauri::command]
pub async fn wa_cancelar_pareamento(app: AppHandle) {
    cancelar_pareamento(&app);
}

/// Grava a persona escolhida em `agent.system_prompt`. O texto e montado no
/// core (puro, testado); aqui so se escreve pelo writer de config padrao.
#[tauri::command]
pub async fn onboarding_aplicar_persona(
    nome: String,
    estilo: EstiloPersona,
    idioma: Idioma,
) -> Result<(), String> {
    let persona = Personalidade {
        nome,
        estilo,
        idioma,
    };
    let prompt = core::montar_system_prompt(&persona);
    let loader = garraia_config::ConfigLoader::new().map_err(|e| format!("abrir config: {e}"))?;
    loader
        .set_agent_system_prompt(&prompt)
        .map_err(|e| format!("gravar persona: {e}"))?;
    Ok(())
}

/// Aplica a politica de acesso do modo escolhido, AUTORIZA o usuario quando for
/// preciso, persiste `onboarding.json` e so entao reinicia o gateway. Usado no
/// fim do wizard e no botao de settings.
///
/// - **proprio** → `access open --yes`, mas so com `confirmo_aberto == true`:
///   `open` responde qualquer um, e um invoke silencioso nunca pode afrouxar a
///   admissao (F3). O proprio usuario fala consigo por "Conversa com voce mesmo"
///   do WhatsApp — `open` admite inclusive o proprio numero.
/// - **dedicado** → `access restricted` (fecha a admissao) **e**
///   `allow <numero_pessoal>` (F1): o Garra fica num chip dedicado e o usuario
///   fala dele pelo numero pessoal, que PRECISA estar autorizado — senao o
///   portao fica com 0 autorizados e descarta o proprio dono em silencio
///   (`acesso.rs`: o numero vinculado nao se autoriza sozinho). O numero e
///   validado no core ([`core::normalizar_numero`]) antes de ir ao CLI.
///
/// `concluido` so e gravado DEPOIS de todos os passos saírem com exit 0 — a tela
/// "Tudo pronto" nao pode mentir.
#[tauri::command]
pub async fn onboarding_aplicar_modo(
    app: AppHandle,
    modo: Modo,
    numero_pessoal: Option<String>,
    confirmo_aberto: bool,
) -> Result<(), String> {
    match modo {
        Modo::Proprio => {
            if !confirmo_aberto {
                return Err("o modo \"meu proprio numero\" abre o Garra para qualquer \
pessoa que mandar mensagem; marque a confirmacao na tela para continuar"
                    .to_string());
            }
            rodar_sidecar(&app, to_owned_args(core::args_aplicar_modo(Modo::Proprio))).await?;
        }
        Modo::Dedicado => {
            let numero = numero_pessoal
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .ok_or_else(|| {
                    "informe seu numero pessoal (com codigo do pais) para o modo dedicado"
                        .to_string()
                })?;
            let numero = core::normalizar_numero(numero)?;
            // Fecha a admissao primeiro, autoriza o dono depois.
            rodar_sidecar(&app, to_owned_args(core::args_aplicar_modo(Modo::Dedicado))).await?;
            rodar_sidecar(&app, core::args_autorizar_numero(&numero)).await?;
        }
    }

    // Concluido so apos a autorizacao ter dado certo.
    gravar_persistido(
        &app,
        &OnboardingPersistido {
            concluido: true,
            modo: Some(modo),
            versao: core::VERSAO_PERSISTIDA,
        },
    )?;

    // Reinicia o gateway (thread bloqueante: `restart` espera a porta soltar
    // antes de respawnar — nunca no executor async). Um gateway de fora do app
    // (#1816) nao e reiniciado por aqui: a tela Pronto diz o que fazer.
    let app_restart = app.clone();
    let gw = app
        .try_state::<crate::gateway::GatewayHandle>()
        .map(|s| s.inner().clone());
    if let Some(gw) = gw {
        let _ = tauri::async_runtime::spawn_blocking(move || {
            crate::gateway::restart(&app_restart, &gw);
        })
        .await;
    }
    Ok(())
}

/// `Vec<&str>` do core -> `Vec<String>` que [`rodar_sidecar`] consome.
fn to_owned_args(args: Vec<&str>) -> Vec<String> {
    args.into_iter().map(String::from).collect()
}

/// Fecha a janela do wizard (botao "Abrir o Garra" / fim).
#[tauri::command]
pub async fn onboarding_fechar(app: AppHandle) {
    cancelar_pareamento(&app);
    if let Some(win) = app.get_webview_window(LABEL) {
        let _ = win.close();
    }
}

/// Reabre o wizard (settings → "Refazer pareamento").
#[tauri::command]
pub async fn onboarding_reabrir(app: AppHandle) -> Result<(), String> {
    abrir(&app).map_err(|e| format!("abrir o wizard: {e}"))
}
