//! `garra tasks` — as tarefas agendadas e a autoridade com que rodam (#1985).
//!
//! # Por que este comando existe
//!
//! Uma tarefa agendada roda com a autoridade de quem a criou, gravada na
//! criacao (`scheduled_tasks.authority`). Tarefa anterior a esse registro nao
//! tem autoridade gravada: numa sessao remota ela roda so com leitura, e o
//! aviso que ela entrega ao canal manda o operador aqui.
//!
//! `garra tasks authorize <id>` e a **reautorizacao do operador**: grava na
//! tarefa uma autoridade sem teto. E poder de escrita novo sobre um texto que
//! pode ter vindo de fora, e por isso o comando tem duas marchas:
//!
//! - sem `--yes`, mostra a tarefa — canal, agenda e o texto que o agente vai
//!   receber — e **nao altera nada**;
//! - com `--yes`, grava.
//!
//! Quem roda este comando e quem tem acesso ao `sessions.db` da instalacao: a
//! prova de operador aqui e a do sistema de arquivos, como no `garra runs`.
//!
//! # O que a reautorizacao NAO faz
//!
//! Nao passa por cima da politica de acesso de hoje do canal. No WhatsApp
//! vinculado, o executor compoe a autoridade da tarefa com o acesso ATUAL de
//! quem a criou: um contato rebaixado ou bloqueado continua rebaixado ou
//! bloqueado nas tarefas dele, reautorizadas ou nao.
//!
//! # Codigos de saida (sysexits, iguais aos do `garra runs`)
//!
//! - `0`  — ok, inclusive "nada a listar" e a previa sem `--yes`.
//! - `2`  — `EX_USAGE`: id desconhecido, ou tarefa que ja terminou.
//! - `74` — `EX_IOERR`: o banco existe mas nao abre.
//!
//! # Sobre conteudo
//!
//! O `payload` da tarefa **nao** e texto de confianca (nasce de uma tool call
//! de LLM, disparada por qualquer usuario de canal), e sai no terminal do
//! operador: todo campo passa por `sanitize_control_chars`. O `user_id` — no
//! WhatsApp, um numero de telefone — nao sai em lugar nenhum; o `session_id`
//! so no `--json`. Este modulo nao emite linha de log.

use anyhow::Result;
use garraia_agents::modes::{AutoridadeDaTarefa, OrigemDaAutoridade};
use garraia_config::AppConfig;
use garraia_db::agent_runs::{iso8601_utc, preview, sanitize_control_chars};
use garraia_db::{ScheduledTaskSummary, SessionStore};

use crate::chat::SESSIONS_DB;

/// Tudo certo.
const EXIT_OK: i32 = 0;
/// `EX_USAGE` — argumento invalido.
const EXIT_USAGE: i32 = 2;
/// `EX_IOERR` — o arquivo existe mas nao abre.
const EXIT_IOERR: i32 = 74;

/// Quanto do texto da tarefa cabe na listagem.
const PAYLOAD_PREVIEW_CHARS: usize = 72;

/// O estado da coluna `authority`, como o operador precisa ler.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Estado {
    /// Gravada pelo turno que criou a tarefa.
    DoTurno,
    /// Reautorizada pelo operador.
    DoOperador,
    /// Sem autoridade gravada: roda pela superficie da sessao.
    Ausente,
    /// Documento que este binario nao entende: roda so com leitura.
    Invalida,
}

impl Estado {
    pub(crate) fn de(authority: Option<&str>) -> Self {
        match authority {
            None => Self::Ausente,
            Some(doc) => match AutoridadeDaTarefa::de_json(doc).map(|a| a.origem) {
                Some(OrigemDaAutoridade::Turno) => Self::DoTurno,
                Some(OrigemDaAutoridade::Operador) => Self::DoOperador,
                None => Self::Invalida,
            },
        }
    }

    /// Chave estavel do `--json`.
    pub(crate) fn chave(self) -> &'static str {
        match self {
            Self::DoTurno => "turn",
            Self::DoOperador => "operator",
            Self::Ausente => "none",
            Self::Invalida => "invalid",
        }
    }

    fn rotulo(self) -> &'static str {
        match self {
            Self::DoTurno => "gravada na criacao",
            Self::DoOperador => "reautorizada pelo operador",
            Self::Ausente => "SEM autoridade gravada",
            Self::Invalida => "INVALIDA (roda so com leitura)",
        }
    }
}

/// Abre o `sessions.db` sem cria-lo. `None` no `Ok` = nao ha banco ainda.
fn abrir(config: &AppConfig) -> std::result::Result<Option<SessionStore>, ()> {
    let path = config.resolved_data_dir().join(SESSIONS_DB);
    if !path.exists() {
        return Ok(None);
    }
    match SessionStore::open(&path) {
        Ok(store) => Ok(Some(store)),
        Err(e) => {
            eprintln!(
                "error: o banco de sessoes existe mas nao abre ({}): {e}",
                path.display()
            );
            Err(())
        }
    }
}

fn agenda(t: &ScheduledTaskSummary) -> String {
    match &t.cron_expr {
        Some(cron) => format!("recorrente ({})", sanitize_control_chars(cron)),
        None => "unica".to_string(),
    }
}

/// Uma tarefa em duas linhas de terminal: cabecalho e previa do texto.
pub(crate) fn linha(t: &ScheduledTaskSummary) -> String {
    format!(
        "{:<9}  {}  {:<16}  {}  [{}]\n    {}\n    id: {}",
        sanitize_control_chars(&t.status),
        iso8601_utc(&t.execute_at),
        sanitize_control_chars(t.channel_id.as_deref().unwrap_or("(sem sessao)")),
        agenda(t),
        Estado::de(t.authority.as_deref()).rotulo(),
        preview(&t.payload, PAYLOAD_PREVIEW_CHARS),
        sanitize_control_chars(&t.id),
    )
}

/// Uma tarefa em JSON. Contrato estavel de chaves; sem `user_id`.
pub(crate) fn json_de(t: &ScheduledTaskSummary) -> serde_json::Value {
    serde_json::json!({
        "id": t.id,
        "session_id": t.session_id,
        "channel_id": t.channel_id,
        "status": t.status,
        "execute_at": iso8601_utc(&t.execute_at),
        "cron_expr": t.cron_expr,
        "payload": t.payload,
        "authority": Estado::de(t.authority.as_deref()).chave(),
    })
}

/// `garra tasks list` — as tarefas que ainda podem rodar (ou todas, com
/// `--all`), da mais proxima a mais distante.
pub fn run_list(config: &AppConfig, all: bool, json: bool) -> Result<i32> {
    let store = match abrir(config) {
        Ok(Some(store)) => store,
        Ok(None) => {
            if json {
                println!("[]");
            } else {
                println!("Nenhuma tarefa agendada: o banco de sessoes ainda nao existe.");
            }
            return Ok(EXIT_OK);
        }
        Err(()) => return Ok(EXIT_IOERR),
    };
    let tarefas = store.list_scheduled_tasks(!all)?;

    if json {
        let payload: Vec<_> = tarefas.iter().map(json_de).collect();
        println!("{}", serde_json::to_string_pretty(&payload)?);
        return Ok(EXIT_OK);
    }
    if tarefas.is_empty() {
        println!(
            "Nenhuma tarefa agendada{}.",
            if all { "" } else { " em aberto" }
        );
        return Ok(EXIT_OK);
    }
    for t in &tarefas {
        println!("{}", linha(t));
    }
    let bin = crate::binario::nome();
    if tarefas
        .iter()
        .any(|t| Estado::de(t.authority.as_deref()) != Estado::DoTurno)
    {
        println!(
            "\nNota: tarefa SEM autoridade gravada roda pelo canal da sessao — numa sessao \
             remota,\nso com leitura. Para ela escrever ou executar: `{bin} tasks authorize <id>`."
        );
    }
    Ok(EXIT_OK)
}

/// O que `authorize`/`revoke` decidem antes de tocar no banco. Pura.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Alvo<'a> {
    Encontrada(&'a ScheduledTaskSummary),
    Desconhecida,
    /// Existe, mas ja terminou: nao ha execucao futura para autorizar.
    Encerrada(&'a ScheduledTaskSummary),
}

pub(crate) fn alvo<'a>(tarefas: &'a [ScheduledTaskSummary], id: &str) -> Alvo<'a> {
    match tarefas.iter().find(|t| t.id == id) {
        None => Alvo::Desconhecida,
        Some(t) if matches!(t.status.as_str(), "pending" | "running") => Alvo::Encontrada(t),
        Some(t) => Alvo::Encerrada(t),
    }
}

/// Acha a tarefa pelo id EXATO (sem prefixo: autorizar a tarefa errada e
/// pior que digitar o id inteiro) ou imprime o motivo e devolve o codigo.
fn localizar(
    store: &SessionStore,
    id: &str,
) -> Result<std::result::Result<ScheduledTaskSummary, i32>> {
    let tarefas = store.list_scheduled_tasks(false)?;
    Ok(match alvo(&tarefas, id) {
        Alvo::Encontrada(t) => Ok(t.clone()),
        Alvo::Desconhecida => {
            eprintln!(
                "error: nenhuma tarefa agendada com esse id. Veja os ids com `{} tasks list`.",
                crate::binario::nome()
            );
            Err(EXIT_USAGE)
        }
        Alvo::Encerrada(t) => {
            eprintln!(
                "error: essa tarefa ja terminou (status `{}`); nao ha execucao futura para \
                 autorizar.",
                sanitize_control_chars(&t.status)
            );
            Err(EXIT_USAGE)
        }
    })
}

/// O texto INTEIRO que o agente vai receber, higienizado. E o que o operador
/// precisa ler antes de dar poder de escrita a ele.
fn mostrar(t: &ScheduledTaskSummary) {
    println!("Tarefa   {}", sanitize_control_chars(&t.id));
    println!(
        "Canal    {}",
        sanitize_control_chars(t.channel_id.as_deref().unwrap_or("(sem sessao)"))
    );
    println!(
        "Agenda   {} — proxima em {}",
        agenda(t),
        iso8601_utc(&t.execute_at)
    );
    println!("Estado   {}", Estado::de(t.authority.as_deref()).rotulo());
    println!("Texto que o agente recebe:");
    for l in t.payload.lines() {
        println!("    {}", sanitize_control_chars(l));
    }
}

/// `garra tasks authorize <id> [--yes]` — a reautorizacao do operador.
pub fn run_authorize(config: &AppConfig, id: &str, yes: bool) -> Result<i32> {
    let store = match abrir(config) {
        Ok(Some(store)) => store,
        Ok(None) => {
            eprintln!("error: nao ha banco de sessoes; nenhuma tarefa para autorizar.");
            return Ok(EXIT_USAGE);
        }
        Err(()) => return Ok(EXIT_IOERR),
    };
    let tarefa = match localizar(&store, id)? {
        Ok(t) => t,
        Err(code) => return Ok(code),
    };
    mostrar(&tarefa);
    let bin = crate::binario::nome();

    if !yes {
        println!(
            "\nNada foi alterado. Reautorizar da a esta tarefa acesso de operador: escrita de\n\
             arquivo, execucao de comandos e envio de mensagem, em TODA execucao futura dela.\n\
             Leia o texto acima — ele pode ter sido escrito por quem falou com o agente num\n\
             canal remoto. E a tarefa le a conversa da sessao: o que for escrito nela ate a\n\
             execucao tambem a alcanca. Para confirmar: `{bin} tasks authorize {} --yes`",
            sanitize_control_chars(&tarefa.id)
        );
        return Ok(EXIT_OK);
    }

    let Some(doc) = AutoridadeDaTarefa::do_operador().para_json() else {
        eprintln!("error: nao foi possivel montar o documento de autoridade.");
        return Ok(EXIT_IOERR);
    };
    if !store.set_task_authority(&tarefa.id, Some(&doc))? {
        eprintln!("error: a tarefa sumiu antes de ser reautorizada; nada foi alterado.");
        return Ok(EXIT_USAGE);
    }
    println!(
        "\nReautorizada: a proxima execucao roda com acesso de operador.\n\
         A politica de acesso de hoje do canal continua valendo por cima (no WhatsApp\n\
         vinculado, um contato rebaixado ou bloqueado segue assim nas tarefas dele).\n\
         Para desfazer: `{bin} tasks revoke {}`",
        sanitize_control_chars(&tarefa.id)
    );
    Ok(EXIT_OK)
}

/// `garra tasks revoke <id>` — apaga a autoridade gravada da tarefa, que
/// volta a rodar pela superficie da sessao.
pub fn run_revoke(config: &AppConfig, id: &str) -> Result<i32> {
    let store = match abrir(config) {
        Ok(Some(store)) => store,
        Ok(None) => {
            eprintln!("error: nao ha banco de sessoes; nenhuma tarefa para revogar.");
            return Ok(EXIT_USAGE);
        }
        Err(()) => return Ok(EXIT_IOERR),
    };
    let tarefa = match localizar(&store, id)? {
        Ok(t) => t,
        Err(code) => return Ok(code),
    };
    if !store.set_task_authority(&tarefa.id, None)? {
        eprintln!("error: a tarefa sumiu antes de ser revogada; nada foi alterado.");
        return Ok(EXIT_USAGE);
    }
    println!(
        "Autoridade removida: a tarefa {} volta a rodar pelo canal da sessao (numa sessao \
         remota, so com leitura).",
        sanitize_control_chars(&tarefa.id)
    );
    Ok(EXIT_OK)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    fn config_em(dir: &Path) -> AppConfig {
        AppConfig {
            data_dir: Some(dir.to_path_buf()),
            ..Default::default()
        }
    }

    /// Um banco com uma tarefa aberta de sessao remota, sem autoridade.
    fn semeia(dir: &Path) -> String {
        let store = SessionStore::open(&dir.join(SESSIONS_DB)).expect("cria o banco");
        store
            .upsert_session("s-tg", "telegram", "u-1", &serde_json::json!({}))
            .expect("sessao");
        store
            .schedule_task(
                "s-tg",
                "u-1",
                chrono::Utc::now() + chrono::Duration::hours(1),
                "atualiza o relatorio semanal",
            )
            .expect("agenda")
    }

    fn autoridade_de(dir: &Path, id: &str) -> Option<String> {
        SessionStore::open(&dir.join(SESSIONS_DB))
            .expect("reabre")
            .list_scheduled_tasks(false)
            .expect("lista")
            .into_iter()
            .find(|t| t.id == id)
            .expect("a tarefa existe")
            .authority
    }

    #[test]
    fn estado_le_a_coluna_sem_confundir_lixo_com_sem_teto() {
        assert_eq!(Estado::de(None), Estado::Ausente);
        assert_eq!(Estado::de(Some("nao e json")), Estado::Invalida);
        assert_eq!(
            Estado::de(Some(r#"{"v":9,"origem":"operador"}"#)),
            Estado::Invalida
        );
        let operador = AutoridadeDaTarefa::do_operador().para_json().expect("json");
        assert_eq!(Estado::de(Some(&operador)), Estado::DoOperador);
        assert_eq!(
            Estado::de(Some(r#"{"v":1,"origem":"turno"}"#)),
            Estado::DoTurno
        );
    }

    /// Sem `--yes` o comando mostra e NAO grava: e a previa que protege o
    /// operador de dar escrita a um texto que ele nao leu.
    #[test]
    fn authorize_sem_yes_nao_altera_nada() {
        let dir = tempfile::tempdir().expect("tempdir");
        let id = semeia(dir.path());

        let code = run_authorize(&config_em(dir.path()), &id, false).expect("roda");

        assert_eq!(code, EXIT_OK);
        assert_eq!(autoridade_de(dir.path(), &id), None);
    }

    /// Com `--yes` grava a autoridade do operador — e `revoke` a tira.
    #[test]
    fn authorize_com_yes_grava_a_autoridade_do_operador_e_revoke_desfaz() {
        let dir = tempfile::tempdir().expect("tempdir");
        let id = semeia(dir.path());
        let config = config_em(dir.path());

        assert_eq!(run_authorize(&config, &id, true).expect("roda"), EXIT_OK);
        let doc = autoridade_de(dir.path(), &id).expect("gravou");
        assert_eq!(
            AutoridadeDaTarefa::de_json(&doc),
            Some(AutoridadeDaTarefa::do_operador())
        );

        assert_eq!(run_revoke(&config, &id).expect("roda"), EXIT_OK);
        assert_eq!(autoridade_de(dir.path(), &id), None);
    }

    /// Id desconhecido e tarefa encerrada sao erro de uso, e nada e gravado.
    /// Prefixo do id NAO casa: autorizar a tarefa errada seria pior.
    #[test]
    fn id_desconhecido_prefixo_e_tarefa_encerrada_sao_erro_de_uso() {
        let dir = tempfile::tempdir().expect("tempdir");
        let id = semeia(dir.path());
        let config = config_em(dir.path());

        assert_eq!(
            run_authorize(&config, "nao-existe", true).expect("roda"),
            EXIT_USAGE
        );
        assert_eq!(
            run_authorize(&config, &id[..8], true).expect("roda"),
            EXIT_USAGE,
            "prefixo nao e id"
        );
        assert_eq!(autoridade_de(dir.path(), &id), None);

        SessionStore::open(&dir.path().join(SESSIONS_DB))
            .expect("reabre")
            .complete_task(&id)
            .expect("conclui");
        assert_eq!(
            run_authorize(&config, &id, true).expect("roda"),
            EXIT_USAGE,
            "tarefa encerrada nao tem execucao futura"
        );
        assert_eq!(autoridade_de(dir.path(), &id), None);
    }

    /// Sem banco, listar nao cria o arquivo e autorizar e erro de uso.
    #[test]
    fn sem_banco_listar_nao_cria_o_arquivo() {
        let dir = tempfile::tempdir().expect("tempdir");
        let config = config_em(dir.path());

        assert_eq!(run_list(&config, false, false).expect("roda"), EXIT_OK);
        assert_eq!(run_list(&config, true, true).expect("roda"), EXIT_OK);
        assert!(!dir.path().join(SESSIONS_DB).exists());
        assert_eq!(run_authorize(&config, "x", true).expect("roda"), EXIT_USAGE);
        assert!(!dir.path().join(SESSIONS_DB).exists());
    }

    /// A linha de terminal higieniza o texto hostil e nao carrega o usuario;
    /// o JSON tem as chaves estaveis e tambem nao carrega o usuario.
    #[test]
    fn saida_higieniza_o_texto_e_nunca_mostra_o_usuario() {
        let t = ScheduledTaskSummary {
            id: "t-1".to_string(),
            session_id: "whatsapp-linked-5511999990000@s.whatsapp.net".to_string(),
            channel_id: Some("whatsapp_linked".to_string()),
            user_id: "5511999990000".to_string(),
            status: "pending".to_string(),
            execute_at: "2026-12-01T12:00:00+00:00".to_string(),
            cron_expr: Some("0 9 * * *".to_string()),
            payload: "limpa\u{1b}[2K a linha".to_string(),
            authority: None,
        };
        let humano = linha(&t);
        assert!(!humano.contains('\u{1b}'), "controle de terminal vazou");
        assert!(!humano.contains("5511999990000"), "{humano}");
        assert!(humano.contains("SEM autoridade gravada"), "{humano}");
        assert!(humano.contains("recorrente (0 9 * * *)"), "{humano}");

        let j = json_de(&t);
        assert_eq!(j["authority"], "none");
        assert_eq!(j["status"], "pending");
        assert!(j.get("user_id").is_none(), "o usuario nao sai no JSON");
    }

    /// Este modulo nao emite log: o texto da tarefa e de terceiros e a regra
    /// do projeto e nao leva-lo para log estruturado.
    #[test]
    fn o_modulo_nao_emite_log() {
        let src = include_str!("tasks_cmd.rs");
        let producao = src.split("#[cfg(test)]").next().unwrap_or(src);
        for macro_de_log in ["tracing::", "info!(", "warn!(", "error!(", "debug!("] {
            assert!(
                !producao.contains(macro_de_log),
                "`{macro_de_log}` apareceu em tasks_cmd.rs"
            );
        }
    }
}
