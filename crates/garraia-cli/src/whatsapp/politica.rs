//! `garraia whatsapp access ...` — a Access Policy v2 pela CLI (ADR 0025;
//! #1396, #1397, #1398, #1399, #1400, #1401, #1413, #1414).
//!
//! Casca fina sobre o motor do gateway: a leitura e
//! `whatsapp_linked_politica::impacto` (a MESMA matriz que o turno aplica), a
//! escrita e `whatsapp_linked_politica::mutacao::aplicar` (validacao antes
//! de gravar), a gravacao e a escrita atomica `0600` do `ConfigLoader::save`,
//! e cada mutacao aplicada vai para `whatsapp_linked_politica::auditoria`.
//! Nada aqui reimplementa regra; o que este modulo decide e o contrato de
//! operador — flags, exit codes, confirmacoes e o que sai na tela (nunca uma
//! identidade inteira, salvo `--reveal`).
//!
//! Exit codes (sysexits): 0 ok · 1 cancelado · 64 uso (`open`/`reset` sem
//! terminal e sem `--yes`; nivel e `--preset` juntos, ou nenhum dos dois, nos
//! comandos que aceitam os dois) · 65 dado invalido (numero, combinacao de nivel e
//! write, identidade desconhecida) · 70 config ilegivel ou falha ao gravar ·
//! 73 mudanca gravada mas audit falhou.

use garraia_agents::modes::Nivel;
use garraia_config::{AppConfig, ExecutionProfile};
use garraia_gateway::bootstrap::whatsapp_linked_politica::impacto::{self, Diferenca};
use garraia_gateway::bootstrap::whatsapp_linked_politica::mutacao::{
    self, Aplicada, Mutacao, MutacaoInvalida,
};
use garraia_gateway::bootstrap::whatsapp_linked_politica::presets::Preset;
use garraia_gateway::bootstrap::whatsapp_linked_politica::visao::{
    self, mapa_de_revelacao, nome_do_perfil,
};
use garraia_gateway::bootstrap::whatsapp_linked_politica::{Admission, Alcance, auditoria};
use garraia_gateway::bootstrap::{
    WHATSAPP_LINKED_CONFIG_KEY as CONFIG_KEY, WhatsAppLinkedSettings, whatsapp_linked_settings,
};

use super::acesso::{
    EX_DATAERR, EX_USAGE, MensagemDeNumero, carregar, dica_do_gateway, final4, normalizar_numero,
    secao_criando,
};
use super::{Context, EX_CANCELLED, EX_SOFTWARE, Lang, t, tb};
use crate::wizard::prompts::Prompter;

/// Mudanca gravada, audit nao (sysexits `EX_CANTCREAT`).
pub(crate) const EX_CANTCREAT: i32 = 73;

/// `garraia whatsapp access <subcomando>`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ComandoDeAcesso {
    /// `access [--json] [--reveal]`: a politica efetiva inteira (#1400).
    Mostrar { json: bool, revelar: bool },
    /// `access open [--yes] [--dry-run]` (#1396).
    Abrir { yes: bool, dry_run: bool },
    /// `access restricted [--dry-run]` (#1396).
    Restringir { dry_run: bool },
    /// `access default <chat|read> [--write] [--dry-run]` (#1399), ou
    /// `access default --preset <nome> [--dry-run]` (#1434): exatamente um dos
    /// dois — `preset` vence e ja traz o `write` canonico. O motor recusa
    /// `developer`/`full_pod` aqui, como recusa `full` (#1390).
    Default {
        nivel: Option<Nivel>,
        write: bool,
        preset: Option<Preset>,
        dry_run: bool,
    },
    /// `level <numero> <chat|read|full> [--dry-run]` (#1398).
    Nivel {
        numero: String,
        nivel: Nivel,
        dry_run: bool,
    },
    /// `write <numero> on|off [--dry-run]` (#1397).
    Write {
        numero: String,
        on: bool,
        dry_run: bool,
    },
    /// `preset <numero> <chat_only|read|developer|full_pod> [--dry-run]`
    /// (#1434): grava `level` + `write` de uma vez, pelo alcance canonico do
    /// preset.
    Preset {
        numero: String,
        preset: Preset,
        dry_run: bool,
    },
    /// `block <numero> [--dry-run]`.
    Bloquear { numero: String, dry_run: bool },
    /// `unblock <numero> [--dry-run]`.
    Desbloquear { numero: String, dry_run: bool },
    /// `access groups on|off [--dry-run]` (#1423).
    Grupos { ligados: bool, dry_run: bool },
    /// `access groups default <chat|read|full> [--write] [--dry-run]`, ou
    /// `--preset <nome>` no lugar do nivel (#1434).
    GrupoDefault {
        nivel: Option<Nivel>,
        write: bool,
        preset: Option<Preset>,
        dry_run: bool,
    },
    /// `access group <jid> <chat|read|full> [--write] [--dry-run]`, ou
    /// `--preset <nome>` no lugar do nivel (#1434).
    Grupo {
        jid: String,
        nivel: Option<Nivel>,
        write: bool,
        preset: Option<Preset>,
        dry_run: bool,
    },
    /// `access reset [--yes] [--dry-run]` (#1401).
    Reset { yes: bool, dry_run: bool },
    /// `access audit [--json] [--limit N]` (#1414).
    Audit { json: bool, limit: usize },
}

/// O que [`aplicar`] fez.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Aplicacao {
    pub aplicada: Aplicada,
    /// O impacto por principal, pelo motor real (#1413).
    pub diferencas: Vec<Diferenca>,
    /// `false` em `--dry-run` ou quando nada mudou.
    pub gravou: bool,
    /// Foi so simulacao (`--dry-run`)?
    pub simulado: bool,
    /// O canal esta ligado na config resultante (decide a dica do gateway).
    pub canal_ligado: bool,
    /// Gravou, mas o audit falhou: a mensagem, para quem chama avisar.
    pub audit_falhou: Option<String>,
}

/// Quem rodou o comando, para o audit: o usuario do SO. Nunca segredo.
fn ator() -> String {
    std::env::var("USER")
        .or_else(|_| std::env::var("USERNAME"))
        .ok()
        .filter(|u| !u.trim().is_empty())
        .unwrap_or_else(|| "desconhecido".to_string())
}

/// `channels.whatsapp_linked.audit_max_bytes`, ou o default do motor.
fn teto_do_audit(config: &AppConfig) -> u64 {
    config
        .channels
        .get(CONFIG_KEY)
        .and_then(|s| s.settings.get("audit_max_bytes"))
        .and_then(|v| v.as_u64())
        .filter(|n| *n > 0)
        .unwrap_or(auditoria::MAX_BYTES_DEFAULT)
}

/// O evento de uma mudanca JA gravada, no audit local: origem `cli`, o
/// usuario do SO como ator, o alvo mascarado por `Evento::novo` e o resumo
/// antes/depois pelo mesmo leitor do gateway. `Err` traz a mensagem do SO;
/// quem chama decide como avisar.
fn registrar_evento(
    ctx: &Context,
    acao: &str,
    alvo: Option<&str>,
    antes: &WhatsAppLinkedSettings,
    depois: &WhatsAppLinkedSettings,
    teto: u64,
) -> Result<(), String> {
    let evento = auditoria::Evento::novo("cli", &ator(), acao, alvo, antes, depois);
    auditoria::registrar(&ctx.data_dir, &evento, teto)
        .map(|_| ())
        .map_err(|e| e.to_string())
}

/// A frase de "gravou, mas o audit nao" — a MESMA em todos os comandos que
/// mexem no acesso, para o operador reconhecer o estado de longe.
pub(crate) fn aviso_de_audit_falhou(lang: Lang, erro: &str) -> String {
    match lang {
        Lang::Pt => format!(
            "A mudanca foi gravada, mas o audit local nao: {erro} (arquivo `{}`).",
            auditoria::ARQUIVO
        ),
        Lang::En => format!(
            "The change was written, but the local audit was not: {erro} (file `{}`).",
            auditoria::ARQUIVO
        ),
    }
}

/// Audita uma mudanca que um comando LEGADO ja gravou por fora do motor de
/// mutacao — `allow`, `remove`, `owner`, `unowner` e o passo pos-QR do `link`
/// (#1414).
///
/// Ate a #1414 esses comandos gravavam `allow`/`owners` sem deixar rastro, e
/// a trilha de `access audit` so contava metade da historia. O evento e o
/// MESMO que [`aplicar`] grava — `acao` e o nome do subcomando, `antes` e
/// `depois` sao as duas configs (a lida e a gravada), resumidas pelo mesmo
/// leitor do gateway, e o alvo sai mascarado (`…1234`). Falha vira o mesmo
/// aviso e o mesmo exit 73 (`Err`, ja impresso): a mudanca ficou, o audit
/// nao, e o operador tem de saber.
pub(crate) fn auditar(
    ctx: &Context,
    acao: &str,
    alvo: &str,
    antes: &AppConfig,
    depois: &AppConfig,
) -> Result<(), i32> {
    let de = whatsapp_linked_settings(antes);
    let para = whatsapp_linked_settings(depois);
    registrar_evento(ctx, acao, Some(alvo), &de, &para, teto_do_audit(depois)).map_err(|erro| {
        eprintln!("{}", aviso_de_audit_falhou(ctx.lang, &erro));
        EX_CANTCREAT
    })
}

/// Aplica uma mutacao a config em disco (ou so simula, com `dry_run`).
///
/// Carrega, aplica pelo motor, calcula o impacto e — fora do `dry_run` e so
/// se algo mudou — grava atomico e audita. `Err(exit code)` ja com a mensagem
/// impressa em `stderr`.
pub fn aplicar(ctx: &Context, mutacao: &Mutacao, dry_run: bool) -> Result<Aplicacao, i32> {
    let (loader, config) = carregar(ctx)?;
    let original = config.clone();
    let mut config = config;
    let perfil = config.execution.perfil();
    let secao = secao_criando(&mut config).map_err(|e| {
        eprintln!("{e}");
        EX_SOFTWARE
    })?;
    let aplicada = mutacao::aplicar(secao, mutacao).map_err(|e| {
        eprintln!("{e}");
        match e {
            MutacaoInvalida::SecaoDeOutroCanal(_) | MutacaoInvalida::NaoEMapa(_) => EX_SOFTWARE,
            _ => EX_DATAERR,
        }
    })?;
    let antes = whatsapp_linked_settings(&original);
    let depois = whatsapp_linked_settings(&config);
    let diferencas = impacto::diferencas(&antes, &depois, perfil);
    let mut aplicacao = Aplicacao {
        aplicada,
        diferencas,
        gravou: false,
        simulado: dry_run,
        canal_ligado: depois.enabled,
        audit_falhou: None,
    };
    if dry_run || !aplicacao.aplicada.mudou {
        return Ok(aplicacao);
    }
    if let Err(e) = loader.ensure_dirs().and_then(|()| loader.save(&config)) {
        eprintln!("{e}");
        return Err(EX_SOFTWARE);
    }
    aplicacao.gravou = true;
    if let Err(erro) = registrar_evento(
        ctx,
        mutacao.acao(),
        mutacao.alvo(),
        &antes,
        &depois,
        teto_do_audit(&config),
    ) {
        aplicacao.audit_falhou = Some(erro);
    }
    Ok(aplicacao)
}

/// O documento de `access --json`: o MESMO que a API admin devolve
/// (`whatsapp_linked_politica::visao::documento`), com `--reveal` local.
pub fn json_da_politica(
    config: &AppConfig,
    perfil: ExecutionProfile,
    revelar: bool,
) -> serde_json::Value {
    visao::documento(config, perfil, revelar)
}

fn linha_de_principal(
    lang: Lang,
    l: &impacto::LinhaDaMatriz,
    revelacao: Option<&std::collections::HashMap<String, String>>,
) -> String {
    let alvo = match &l.alvo {
        Some(a) => revelacao
            .and_then(|m| m.get(a))
            .cloned()
            .unwrap_or_else(|| a.clone()),
        None => "—".to_string(),
    };
    let alcance = match l.efetivo.alcance {
        Some(a) => a.to_string(),
        None if l.efetivo.modo == "—" => t(lang, "nao entra", "not admitted").to_string(),
        None => t(lang, "sem teto", "no ceiling").to_string(),
    };
    let caps = l.efetivo.capacidades.ligadas();
    let caps = if caps.is_empty() {
        t(lang, "nenhuma ferramenta", "no tools").to_string()
    } else {
        caps.join(", ")
    };
    format!(
        "  {:<12} {:<18} {:<8} {:<18} {caps}",
        l.principal, alvo, l.efetivo.modo, alcance
    )
}

/// As linhas de `access` (humano).
pub fn linhas_da_politica(
    lang: Lang,
    config: &AppConfig,
    perfil: ExecutionProfile,
    revelar: bool,
) -> Vec<String> {
    let s = whatsapp_linked_settings(config);
    let revelacao = revelar.then(|| mapa_de_revelacao(&s));
    let mut out = Vec::new();
    out.push(match (lang, s.enabled) {
        (Lang::Pt, true) => "Canal:               ligado".to_string(),
        (Lang::Pt, false) => "Canal:               desligado".to_string(),
        (Lang::En, true) => "Channel:             on".to_string(),
        (Lang::En, false) => "Channel:             off".to_string(),
    });
    let piso_dono = s.modo_padrao_efetivo(if perfil.is_isolated_pod() {
        ExecutionProfile::IsolatedPod
    } else {
        ExecutionProfile::Standard
    });
    out.push(match lang {
        Lang::Pt => format!(
            "Perfil de execucao:  {} (piso do dono em 1:1: `{piso_dono}`; dos demais: `{}`)",
            nome_do_perfil(perfil),
            s.modo_padrao_efetivo(ExecutionProfile::Standard)
        ),
        Lang::En => format!(
            "Execution profile:   {} (owner floor in 1:1: `{piso_dono}`; everyone else: `{}`)",
            nome_do_perfil(perfil),
            s.modo_padrao_efetivo(ExecutionProfile::Standard)
        ),
    });
    out.push(match (lang, s.access.admission) {
        (Lang::Pt, Admission::Restricted) => {
            "Admissao:            restricted (so quem esta declarado ou pareou por codigo)"
                .to_string()
        }
        (Lang::Pt, Admission::Open) => format!(
            "Admissao:            open — QUALQUER numero entra, com o default ({})",
            s.access.default
        ),
        (Lang::En, Admission::Restricted) => {
            "Admission:           restricted (only declared identities or code pairing)".to_string()
        }
        (Lang::En, Admission::Open) => format!(
            "Admission:           open — ANY number gets in, with the default ({})",
            s.access.default
        ),
    });
    out.push(match lang {
        Lang::Pt => format!("Default (desconhecido): {}", s.access.default),
        Lang::En => format!("Default (unknown):   {}", s.access.default),
    });
    out.push(match (lang, s.responde_em_grupo()) {
        (Lang::Pt, true) => format!(
            "Grupos:              ligados · default {} · {} com politica propria",
            s.access.groups.default,
            s.access.groups.por_grupo.len()
        ),
        (Lang::Pt, false) => "Grupos:              desligados".to_string(),
        (Lang::En, true) => format!(
            "Groups:              on · default {} · {} with their own policy",
            s.access.groups.default,
            s.access.groups.por_grupo.len()
        ),
        (Lang::En, false) => "Groups:              off".to_string(),
    });
    let bloqueados = s.access.users.values().filter(|u| u.bloqueado).count();
    out.push(match lang {
        Lang::Pt => format!(
            "Autorizados: {} · Donos: {} · Bloqueados: {bloqueados}",
            s.autorizados(),
            s.donos()
        ),
        Lang::En => format!(
            "Authorized: {} · Owners: {} · Blocked: {bloqueados}",
            s.autorizados(),
            s.donos()
        ),
    });
    out.push(String::new());
    out.push(match lang {
        Lang::Pt => format!(
            "  {:<12} {:<18} {:<8} {:<18} {}",
            "principal", "identidade", "piso", "nivel", "pode"
        ),
        Lang::En => format!(
            "  {:<12} {:<18} {:<8} {:<18} {}",
            "principal", "identity", "floor", "level", "can"
        ),
    });
    for l in impacto::matriz(&s, perfil) {
        out.push(linha_de_principal(lang, &l, revelacao.as_ref()));
    }
    if !s.access.avisos.is_empty() {
        out.push(String::new());
        for aviso in &s.access.avisos {
            out.push(format!("  ! {aviso}"));
        }
    }
    if !perfil.is_isolated_pod() && s.donos() > 0 {
        out.push(String::new());
        out.push(
            t(
                lang,
                "Em `standard` o dono nao tem poder extra: o piso `code` do dono so existe em `execution.profile = isolated-pod`.",
                "In `standard` the owner has no extra power: the owner's `code` floor only exists under `execution.profile = isolated-pod`.",
            )
            .to_string(),
        );
    }
    out
}

/// As linhas do preview/resultado de uma mutacao: o que mudou na config e o
/// que cada principal ganha e perde.
pub fn linhas_de_impacto(lang: Lang, aplicacao: &Aplicacao) -> Vec<String> {
    let mut out = Vec::new();
    if aplicacao.simulado {
        out.push(
            t(
                lang,
                "Simulacao (--dry-run): nada foi gravado nem auditado.",
                "Simulation (--dry-run): nothing was written or audited.",
            )
            .to_string(),
        );
    }
    if !aplicacao.aplicada.mudou {
        out.push(
            t(
                lang,
                "Nada a mudar: a politica ja esta assim.",
                "Nothing to change: the policy is already like that.",
            )
            .to_string(),
        );
        return out;
    }
    out.push(t(lang, "Mudancas:", "Changes:").to_string());
    for m in &aplicacao.aplicada.mudancas {
        out.push(format!("  - {m}"));
    }
    if aplicacao.diferencas.is_empty() {
        out.push(
            t(
                lang,
                "Impacto: nenhum principal ganha ou perde ferramenta com isso.",
                "Impact: no principal gains or loses any tool with this.",
            )
            .to_string(),
        );
    } else {
        out.push(t(lang, "Impacto por principal:", "Impact per principal:").to_string());
        for d in &aplicacao.diferencas {
            let quem = match &d.alvo {
                Some(a) => format!("{} {a}", d.principal),
                None => d.principal.to_string(),
            };
            let mut partes = Vec::new();
            if !d.ganha.is_empty() {
                partes.push(format!(
                    "{} {}",
                    t(lang, "ganha", "gains"),
                    d.ganha.join(", ")
                ));
            }
            if !d.perde.is_empty() {
                partes.push(format!(
                    "{} {}",
                    t(lang, "perde", "loses"),
                    d.perde.join(", ")
                ));
            }
            out.push(format!("  - {quem}: {}", partes.join(" · ")));
        }
    }
    if aplicacao.gravou {
        out.push(t(lang, "Gravado no config.yml.", "Written to config.yml.").to_string());
    }
    out
}

/// O documento de `access audit --json`: `{ "events": [...] }`, mais recente
/// primeiro.
pub fn json_do_audit(ctx: &Context, limit: usize) -> Result<serde_json::Value, i32> {
    let eventos = auditoria::ler(&ctx.data_dir, limit).map_err(|e| {
        eprintln!("{e}");
        EX_SOFTWARE
    })?;
    let events: Vec<serde_json::Value> = eventos
        .into_iter()
        .map(|e| {
            serde_json::json!({
                "ts": e.ts, "source": e.origem, "actor": e.ator, "action": e.acao,
                "target": e.alvo, "before": e.antes, "after": e.depois,
            })
        })
        .collect();
    Ok(serde_json::json!({
        "file": ctx.data_dir.join(auditoria::ARQUIVO).display().to_string(),
        "events": events,
    }))
}

// ---------------------------------------------------------------------------
// O wizard `link` (#1429)
// ---------------------------------------------------------------------------

/// O que `open` significa NESTA config: o alcance que um desconhecido
/// recebe. Dito antes do aviso de abrir, para a decisao ser sobre o valor
/// real e nao sobre o `chat` "salvo configurado" da frase generica.
pub(crate) fn linha_do_default_do_desconhecido(lang: Lang, default: Alcance) -> String {
    tb(
        lang,
        "Com `open`, um número desconhecido entra com o default: {default} (muda com `{bin} whatsapp access default`).",
        "With `open`, an unknown number gets in with the default: {default} (change it with `{bin} whatsapp access default`).",
    )
    .replace("{default}", &default.to_string())
}

/// As tres opcoes de nivel, na ordem de [`Nivel`]: `chat`, `read`, `full`.
fn opcoes_de_nivel(lang: Lang) -> [&'static str; 3] {
    match lang {
        Lang::Pt => [
            "chat — só conversa, nenhuma ferramenta",
            "read — só leitura (arquivos, busca); nunca shell nem escrita (padrão)",
            "full — sem teto próprio: o modo e o perfil de execução decidem",
        ],
        Lang::En => [
            "chat — conversation only, no tools",
            "read — read-only (files, search); never shell nor writes (default)",
            "full — no ceiling of its own: the mode and the execution profile decide",
        ],
    }
}

/// As duas admissoes, na ordem `restricted`, `open`.
fn opcoes_de_admissao(lang: Lang) -> [&'static str; 2] {
    match lang {
        Lang::Pt => [
            "restricted — só quem você autorizar (padrão)",
            "open — QUALQUER número, com o default do desconhecido",
        ],
        Lang::En => [
            "restricted — only who you authorize (default)",
            "open — ANY number, with the unknown-sender default",
        ],
    }
}

/// As perguntas de politica do wizard `link` (#1429), depois do numero.
///
/// Pura quanto ao disco: pergunta e devolve o que aplicar, na ordem em que
/// foi respondido; quem grava e [`aplicar`], mutacao a mutacao, pelo mesmo
/// motor e com o mesmo audit do `access` — o wizard nao tem uma segunda
/// forma de escrever politica. Defaults seguros em tudo: `read` sem escrita
/// para quem acabou de entrar (Enter nao da shell a ninguem), `restricted`
/// para a admissao, e `open` so depois do MESMO aviso do `access open`, com
/// default nao.
///
/// - `recem_autorizado`: o numero que o passo anterior acabou de gravar em
///   `allow`, se houve. Nivel e escrita so fazem sentido para ele: dono nao
///   tem teto (o motor recusaria com `EDono`), e quem ja estava na lista nao
///   e mexido por um `link`.
/// - `default_do_desconhecido`: o alcance que um desconhecido receberia em
///   `open` nesta config, mostrado antes de perguntar se abre.
///
/// Escrita nao e perguntada em `chat` (nao ha onde escrever) e so vira
/// mutacao quando ligada — `off` ja e o estado de quem acabou de entrar.
pub(crate) fn perguntar_politica(
    ctx: &Context,
    prompter: &dyn Prompter,
    recem_autorizado: Option<&str>,
    default_do_desconhecido: Alcance,
) -> Vec<Mutacao> {
    let lang = ctx.lang;
    let mut out = Vec::new();
    if let Some(numero) = recem_autorizado {
        let fim = final4(numero);
        let pergunta = match lang {
            Lang::Pt => {
                format!("Nível de acesso de …{fim} (o teto do que ele pode pedir ao GarraIA)")
            }
            Lang::En => format!("Access level for …{fim} (the ceiling on what it may ask GarraIA)"),
        };
        let escolha = prompter
            .select(&pergunta, &opcoes_de_nivel(lang), 1)
            .unwrap_or(1);
        let nivel = match escolha {
            0 => Nivel::Chat,
            2 => Nivel::Full,
            _ => Nivel::Read,
        };
        out.push(Mutacao::Nivel {
            identidade: numero.to_string(),
            nivel,
        });
        if nivel != Nivel::Chat {
            let pergunta = match lang {
                Lang::Pt => format!(
                    "Liberar escrita de arquivo para …{fim}? (só escrita de arquivo, nativa e MCP — nunca shell)"
                ),
                Lang::En => format!(
                    "Allow file writes for …{fim}? (file writes only, native and MCP — never shell)"
                ),
            };
            if prompter.confirm(&pergunta, false).unwrap_or(false) {
                out.push(Mutacao::Write {
                    identidade: numero.to_string(),
                    on: true,
                });
            }
        }
    }
    let admissao = prompter
        .select(
            t(
                lang,
                "Admissão: quem pode falar com o GarraIA por este WhatsApp?",
                "Admission: who may talk to GarraIA through this WhatsApp?",
            ),
            &opcoes_de_admissao(lang),
            0,
        )
        .unwrap_or(0);
    if admissao == 1 {
        println!(
            "{}",
            linha_do_default_do_desconhecido(lang, default_do_desconhecido)
        );
        if prompter
            .confirm(pergunta_de_abrir(lang), false)
            .unwrap_or(false)
        {
            out.push(Mutacao::Admissao(Admission::Open));
        }
    } else {
        // O que foi respondido e o que vai para o disco — inclusive quando
        // ja era assim (o motor nao grava nem audita o que nao mudou).
        out.push(Mutacao::Admissao(Admission::Restricted));
    }
    out
}

fn pergunta_de_abrir(lang: Lang) -> &'static str {
    t(
        lang,
        "Com `admission: open`, QUALQUER pessoa que mandar mensagem para o seu numero passa a falar com o GarraIA (com o default do desconhecido, `chat` salvo configurado). Abrir mesmo assim?",
        "With `admission: open`, ANYONE who messages your number gets to talk to GarraIA (with the unknown-sender default, `chat` unless configured). Open anyway?",
    )
}

fn pergunta_de_reset(lang: Lang) -> &'static str {
    t(
        lang,
        "Isto volta a politica ao seguro: admissao `restricted`, default `chat`, grupos desligados e sem politica, e todo nivel/write declarado removido. Donos e bloqueios ficam. Continuar?",
        "This resets the policy to safe defaults: `restricted` admission, `chat` default, groups off and without policy, and every declared level/write removed. Owners and blocks stay. Continue?",
    )
}

/// `open` e `reset` sao decisoes explicitas: sem terminal exigem `--yes`; no
/// terminal perguntam, com default nao. `--dry-run` nunca pergunta.
fn confirmar(
    ctx: &Context,
    prompter: &dyn Prompter,
    yes: bool,
    dry_run: bool,
    pergunta: &str,
    comando: &str,
) -> Result<(), i32> {
    if yes || dry_run {
        return Ok(());
    }
    if !ctx.interactive {
        eprintln!(
            "{}",
            match ctx.lang {
                Lang::Pt => format!(
                    "`whatsapp access {comando}` sem terminal precisa de `--yes`: e uma decisao explicita (ou `--dry-run` para so ver o impacto)."
                ),
                Lang::En => format!(
                    "`whatsapp access {comando}` without a terminal needs `--yes`: it is an explicit decision (or `--dry-run` to only see the impact)."
                ),
            }
        );
        return Err(EX_USAGE);
    }
    match prompter.confirm(pergunta, false) {
        Ok(true) => Ok(()),
        Ok(false) | Err(_) => {
            println!("{}", t(ctx.lang, "Cancelado.", "Cancelled."));
            Err(EX_CANCELLED)
        }
    }
}

fn numero(ctx: &Context, raw: &str) -> Result<String, i32> {
    normalizar_numero(raw).map_err(|e| {
        eprintln!("{}", e.mensagem(ctx.lang));
        EX_DATAERR
    })
}

/// Um JID de grupo: `<digitos>@g.us`.
fn jid_de_grupo(ctx: &Context, raw: &str) -> Result<String, i32> {
    let jid = raw.trim();
    let valido = jid
        .strip_suffix("@g.us")
        .is_some_and(|id| !id.is_empty() && id.chars().all(|c| c.is_ascii_digit() || c == '-'));
    if valido {
        Ok(jid.to_string())
    } else {
        eprintln!(
            "{}",
            t(
                ctx.lang,
                "JID de grupo invalido: e `<digitos>@g.us` (aparece no log do gateway como `chat …1234` e no `whatsapp status`).",
                "Invalid group JID: it is `<digits>@g.us` (shown in the gateway log as `chat …1234` and in `whatsapp status`).",
            )
        );
        Err(EX_DATAERR)
    }
}

/// O alcance de `access default`, `access groups default` e `access group`:
/// o preset nomeado (#1434) OU o par `<nivel>` + `--write`, nunca os dois e
/// nunca nenhum. O `ArgGroup` do clap ja fecha isso na linha de comando (exit
/// 2, com o uso na tela); este `EX_USAGE` existe para nao haver caminho que
/// grave politica a partir de um pedido ambiguo.
fn alcance_de(
    ctx: &Context,
    nivel: Option<Nivel>,
    write: bool,
    preset: Option<Preset>,
) -> Result<Alcance, i32> {
    match (preset, nivel) {
        // O preset ja traz o `write` canonico: nao ha o que compor.
        (Some(p), None) => Ok(p.alcance()),
        (None, Some(nivel)) => Ok(Alcance { nivel, write }),
        _ => {
            eprintln!(
                "{}",
                t(
                    ctx.lang,
                    "informe o nivel OU `--preset <nome>` — exatamente um dos dois",
                    "give the level OR `--preset <name>` — exactly one of the two",
                )
            );
            Err(EX_USAGE)
        }
    }
}

/// O comando inteiro. Devolve o exit code.
pub fn access(ctx: &Context, prompter: &dyn Prompter, comando: &ComandoDeAcesso) -> i32 {
    match executar(ctx, prompter, comando) {
        Ok(code) | Err(code) => code,
    }
}

fn executar(ctx: &Context, prompter: &dyn Prompter, comando: &ComandoDeAcesso) -> Result<i32, i32> {
    let (mutacao, dry_run) = match comando {
        ComandoDeAcesso::Mostrar { json, revelar } => {
            let (_, config) = carregar(ctx)?;
            let perfil = config.execution.perfil();
            if *json {
                let doc = json_da_politica(&config, perfil, *revelar);
                println!(
                    "{}",
                    serde_json::to_string_pretty(&doc).map_err(|e| {
                        eprintln!("{e}");
                        EX_SOFTWARE
                    })?
                );
            } else {
                super::print_header(ctx);
                for linha in linhas_da_politica(ctx.lang, &config, perfil, *revelar) {
                    println!("{linha}");
                }
            }
            return Ok(0);
        }
        ComandoDeAcesso::Audit { json, limit } => {
            let doc = json_do_audit(ctx, *limit)?;
            if *json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&doc).map_err(|e| {
                        eprintln!("{e}");
                        EX_SOFTWARE
                    })?
                );
            } else {
                super::print_header(ctx);
                let eventos = doc["events"].as_array().cloned().unwrap_or_default();
                if eventos.is_empty() {
                    println!(
                        "{}",
                        t(
                            ctx.lang,
                            "Nenhuma mudanca auditada ainda.",
                            "No audited change yet."
                        )
                    );
                }
                for e in eventos {
                    println!(
                        "  {}  {:<10} {:<9} {:<7} {}",
                        e["ts"].as_str().unwrap_or("-"),
                        e["action"].as_str().unwrap_or("-"),
                        e["source"].as_str().unwrap_or("-"),
                        e["target"].as_str().unwrap_or("—"),
                        e["actor"].as_str().unwrap_or("-"),
                    );
                }
                println!(
                    "{}",
                    tb(ctx.lang, "Arquivo: {file}", "File: {file}")
                        .replace("{file}", doc["file"].as_str().unwrap_or(""))
                );
            }
            return Ok(0);
        }
        ComandoDeAcesso::Abrir { yes, dry_run } => {
            confirmar(
                ctx,
                prompter,
                *yes,
                *dry_run,
                pergunta_de_abrir(ctx.lang),
                "open",
            )?;
            (Mutacao::Admissao(Admission::Open), *dry_run)
        }
        ComandoDeAcesso::Restringir { dry_run } => {
            (Mutacao::Admissao(Admission::Restricted), *dry_run)
        }
        ComandoDeAcesso::Default {
            nivel,
            write,
            preset,
            dry_run,
        } => (
            Mutacao::DefaultDesconhecido(alcance_de(ctx, *nivel, *write, *preset)?),
            *dry_run,
        ),
        ComandoDeAcesso::Nivel {
            numero: raw,
            nivel,
            dry_run,
        } => (
            Mutacao::Nivel {
                identidade: numero(ctx, raw)?,
                nivel: *nivel,
            },
            *dry_run,
        ),
        ComandoDeAcesso::Write {
            numero: raw,
            on,
            dry_run,
        } => (
            Mutacao::Write {
                identidade: numero(ctx, raw)?,
                on: *on,
            },
            *dry_run,
        ),
        ComandoDeAcesso::Preset {
            numero: raw,
            preset,
            dry_run,
        } => (
            Mutacao::Preset {
                identidade: numero(ctx, raw)?,
                preset: *preset,
            },
            *dry_run,
        ),
        ComandoDeAcesso::Bloquear {
            numero: raw,
            dry_run,
        } => (Mutacao::Bloquear(numero(ctx, raw)?), *dry_run),
        ComandoDeAcesso::Desbloquear {
            numero: raw,
            dry_run,
        } => (Mutacao::Desbloquear(numero(ctx, raw)?), *dry_run),
        ComandoDeAcesso::Grupos { ligados, dry_run } => (Mutacao::Grupos(*ligados), *dry_run),
        ComandoDeAcesso::GrupoDefault {
            nivel,
            write,
            preset,
            dry_run,
        } => (
            Mutacao::DefaultDeGrupo(alcance_de(ctx, *nivel, *write, *preset)?),
            *dry_run,
        ),
        ComandoDeAcesso::Grupo {
            jid,
            nivel,
            write,
            preset,
            dry_run,
        } => (
            Mutacao::Grupo {
                jid: jid_de_grupo(ctx, jid)?,
                alcance: alcance_de(ctx, *nivel, *write, *preset)?,
            },
            *dry_run,
        ),
        ComandoDeAcesso::Reset { yes, dry_run } => {
            confirmar(
                ctx,
                prompter,
                *yes,
                *dry_run,
                pergunta_de_reset(ctx.lang),
                "reset",
            )?;
            (Mutacao::Reset, *dry_run)
        }
    };
    let aplicacao = aplicar(ctx, &mutacao, dry_run)?;
    for linha in linhas_de_impacto(ctx.lang, &aplicacao) {
        println!("{linha}");
    }
    if aplicacao.gravou {
        if aplicacao.canal_ligado {
            println!("{}", dica_do_gateway(ctx.lang, true, ctx.gateway_pid));
        } else {
            println!(
                "{}",
                tb(
                    ctx.lang,
                    "O canal ainda nao esta ligado: vincule o WhatsApp com `{bin} whatsapp link`.",
                    "The channel is not on yet: link WhatsApp with `{bin} whatsapp link`.",
                )
            );
        }
    }
    if let Some(erro) = &aplicacao.audit_falhou {
        eprintln!("{}", aviso_de_audit_falhou(ctx.lang, erro));
        return Ok(EX_CANTCREAT);
    }
    Ok(0)
}
