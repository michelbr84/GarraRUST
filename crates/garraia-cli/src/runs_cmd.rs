//! `garra runs` — le o ledger de runs de agente (#1227, slice 4).
//!
//! # O que este comando e, e o que ele nao e
//!
//! E um leitor do `sessions.db`, pelo mesmo caminho que o `garra logs` usa
//! para o arquivo de log: **nao** fala com o gateway, nao abre socket e nao
//! pede que nada esteja rodando. E o que o torna util justamente depois de
//! uma queda, quando a pergunta e "o que estava em voo?".
//!
//! Ele e **somente leitura**. Em particular, ele nao marca run `running`
//! orfao como `interrupted`: essa conversao e do hook de subida
//! (`garraia_db::agent_runs::log_interrupted_runs`, slice 1), que roda no
//! boot do gateway e na abertura do store pelo `chat`. Uma listagem que
//! escrevesse mudaria o que ela mesma esta reportando.
//!
//! # Codigos de saida (sysexits, iguais aos do `garra memory`)
//!
//! - `0`  — ok. **Lista vazia e 0**: "nenhum run" e uma resposta, nao um erro.
//! - `2`  — `EX_USAGE`: `--status` desconhecido, `--limit 0`.
//! - `74` — `EX_IOERR`: o banco existe mas nao abre.
//!
//! # Sobre conteudo
//!
//! `agent_runs.goal` e o payload da tarefa do proprio operador. Ele sai por
//! `stdout`, no terminal de quem pediu a listagem — e **nunca** por log
//! estruturado. A regra do projeto (CLAUDE.md §6) e sobre log, e o hook de
//! subida ja a respeita registrando so ids. Este modulo nao emite uma linha
//! de log de proposito, e ha teste varrendo o fonte atras de uma.

use std::path::{Path, PathBuf};

use anyhow::Result;
use garraia_config::AppConfig;
use garraia_db::{AgentRunRow, RunStatus, SessionStore};

use crate::chat::SESSIONS_DB;

/// Tudo certo.
const EXIT_OK: i32 = 0;
/// `EX_USAGE` — argumento invalido.
const EXIT_USAGE: i32 = 2;
/// `EX_IOERR` — o arquivo existe mas nao abre.
const EXIT_IOERR: i32 = 74;

/// Quantos runs aparecem quando ninguem pede outra coisa.
pub const LIMITE_PADRAO: u32 = 50;

/// Quanto do `goal` cabe numa linha de terminal.
///
/// Corta por **caractere**, nao por byte: payload de tarefa em portugues e
/// cheio de acento, e cortar no meio de um `ç` entrega mojibake.
const GOAL_PREVIEW_CHARS: usize = 72;

/// Os valores que `--status` aceita, na mensagem de erro e na ajuda.
const STATUS_VALIDOS: &str = "running, done, error, cancelled, interrupted";

/// O que aconteceu ao tentar abrir o banco.
enum Opened {
    Store(Box<SessionStore>),
    /// Nao ha banco ainda. Nao e erro — e o caminho que carrega o path,
    /// porque so a mensagem de "nao existe" precisa dele.
    Absent(PathBuf),
    /// Existe e nao abre. A mensagem ja foi para `stderr`.
    Failed,
}

/// Abre o `sessions.db`, sem cria-lo.
///
/// `SessionStore::open` cria o arquivo se ele faltar — util no boot do
/// gateway, ruim aqui: `garra runs list` numa instalacao que nunca rodou
/// nada criaria um banco vazio como efeito colateral de uma leitura.
fn open_store(config: &AppConfig) -> Opened {
    let path = config.resolved_data_dir().join(SESSIONS_DB);
    if !path.exists() {
        return Opened::Absent(path);
    }
    match SessionStore::open(&path) {
        Ok(store) => Opened::Store(Box::new(store)),
        Err(e) => {
            eprintln!(
                "error: o banco de sessoes existe mas nao abre ({}): {e}",
                path.display()
            );
            Opened::Failed
        }
    }
}

fn report_no_store(path: &Path) {
    println!("Nenhum run registrado: {} nao existe.", path.display());
    println!("O ledger e criado quando o gateway sobe (`garra start`) ou na primeira conversa.");
}

/// Le o `--status` do operador.
///
/// Estrito de proposito: `RunStatus::from_str` do `garraia-db` e leniente
/// (qualquer coisa vira `Running`), o que transformaria um erro de digitacao
/// em "nenhum run encontrado" — a resposta mais enganosa possivel para quem
/// procura um run que existe.
pub(crate) fn parse_status(bruto: &str) -> Option<RunStatus> {
    match bruto.trim().to_ascii_lowercase().as_str() {
        "running" => Some(RunStatus::Running),
        "done" => Some(RunStatus::Done),
        "error" => Some(RunStatus::Error),
        "cancelled" => Some(RunStatus::Cancelled),
        "interrupted" => Some(RunStatus::Interrupted),
        _ => None,
    }
}

/// Normaliza o instante do banco para ISO 8601 UTC com sufixo `Z`.
///
/// O ledger grava com `datetime('now')` do SQLite, que devolve
/// `YYYY-MM-DD HH:MM:SS` **em UTC, sem dizer que e UTC**. Timestamp de
/// auditoria do projeto sai sempre com o `Z` explicito (CLAUDE.md §Convencao
/// de datas), entao a marca e colocada aqui, na leitura.
///
/// Um valor que nao parseia volta como veio: inventar um instante para uma
/// linha corrompida seria pior que mostrar o byte cru.
pub(crate) fn iso8601_utc(bruto: &str) -> String {
    let t = bruto.trim();
    if let Ok(naive) = chrono::NaiveDateTime::parse_from_str(t, "%Y-%m-%d %H:%M:%S") {
        return naive
            .and_utc()
            .to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
    }
    // Linha gravada por outro produtor, ja com fuso: normaliza para UTC.
    if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(t) {
        return dt
            .with_timezone(&chrono::Utc)
            .to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
    }
    t.to_string()
}

/// Corta o texto para caber numa linha, respeitando limites de caractere.
pub(crate) fn preview(texto: &str, max_chars: usize) -> String {
    let mut out: String = texto.chars().take(max_chars).collect();
    if out.chars().count() < texto.chars().count() {
        out.push('…');
    }
    out.replace('\n', " ")
}

/// Um run em JSON. Contrato estavel de chaves — quem pediu `--json` esta
/// scriptando. Os dois instantes saem normalizados com `Z`; o `serde` do
/// `AgentRunRow` devolveria a string crua do SQLite, sem fuso.
pub(crate) fn run_json(run: &AgentRunRow) -> serde_json::Value {
    serde_json::json!({
        "id": run.id,
        "session_id": run.session_id,
        "status": run.status.as_str(),
        "mode": run.mode,
        "goal": run.goal,
        "started_at": iso8601_utc(&run.started_at),
        "finished_at": run.finished_at.as_deref().map(iso8601_utc),
        "result_snippet": run.result_snippet,
        "error_snippet": run.error_snippet,
    })
}

/// Um run em duas linhas de terminal: cabecalho e previa do objetivo.
///
/// O objetivo vai na **segunda** linha, e nao no fim da primeira, porque ele
/// e truncado: pendurar campo de tamanho fixo depois de um texto cortado
/// embaralha os dois.
pub(crate) fn run_line(run: &AgentRunRow) -> String {
    let fim = match &run.finished_at {
        Some(f) => iso8601_utc(f),
        // Largura da coluna do instante, para as linhas nao dancarem.
        None => "—                   ".to_string(),
    };
    let sessao = run.session_id.as_deref().unwrap_or("—");
    let modo = run.mode.as_deref().unwrap_or("—");
    format!(
        "{:<11}  {}  {}  {:<10}  {}  [sessao {}]\n    {}",
        run.status.as_str(),
        iso8601_utc(&run.started_at),
        fim,
        modo,
        run.id,
        sessao,
        preview(&run.goal, GOAL_PREVIEW_CHARS),
    )
}

/// `garra runs list` — os runs mais recentes do ledger, do mais novo ao mais
/// velho.
pub fn run_list(config: &AppConfig, status: Option<String>, limit: u32, json: bool) -> Result<i32> {
    if limit == 0 {
        eprintln!("error: --limit precisa ser maior que zero");
        return Ok(EXIT_USAGE);
    }
    let filtro = match status.as_deref() {
        None => None,
        Some(bruto) => match parse_status(bruto) {
            Some(s) => Some(s),
            None => {
                eprintln!("error: status desconhecido {bruto:?} — use um de: {STATUS_VALIDOS}");
                return Ok(EXIT_USAGE);
            }
        },
    };

    let store = match open_store(config) {
        Opened::Store(store) => *store,
        Opened::Absent(path) => {
            if json {
                println!("[]");
            } else {
                report_no_store(&path);
            }
            return Ok(EXIT_OK);
        }
        Opened::Failed => return Ok(EXIT_IOERR),
    };

    let runs = match &filtro {
        Some(s) => store.list_recent_agent_runs_by_status(s, limit)?,
        None => store.list_recent_agent_runs(limit)?,
    };

    if json {
        let payload: Vec<_> = runs.iter().map(run_json).collect();
        println!("{}", serde_json::to_string_pretty(&payload)?);
        return Ok(EXIT_OK);
    }

    if runs.is_empty() {
        match &filtro {
            Some(s) => println!("Nenhum run com status `{}`.", s.as_str()),
            None => println!("Nenhum run registrado ainda."),
        }
        return Ok(EXIT_OK);
    }

    for run in &runs {
        println!("{}", run_line(run));
    }

    // Um `running` na listagem so e "em execucao" se houver um gateway vivo;
    // depois de uma queda ele e residuo, e quem converte e a subida seguinte.
    // Sem esta nota, "running" numa maquina parada e leitura errada garantida.
    if runs.iter().any(|r| r.status == RunStatus::Running) {
        println!(
            "\nNota: `running` so significa \"em execucao\" com o gateway de pe. Apos uma\n\
             queda, a proxima subida converte esses runs em `interrupted`."
        );
    }

    Ok(EXIT_OK)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Recorta o corpo de uma funcao deste fonte, do cabecalho ate o `}` em
    /// coluna zero — mesma tecnica do scan de `run_scheduler` no
    /// `server.rs`. Sem o recorte, os scans casariam com o proprio modulo de
    /// teste, que tambem chama as funcoes vigiadas.
    fn corpo_de(assinatura: &str) -> &'static str {
        let src = include_str!("runs_cmd.rs");
        let inicio = src
            .find(assinatura)
            .unwrap_or_else(|| panic!("{assinatura} tem de existir em runs_cmd.rs"));
        let resto = &src[inicio..];
        let fim = resto
            .find("\n}\n")
            .expect("a funcao tem de fechar em coluna zero");
        &resto[..fim]
    }

    /// #1227 (slice 4): a listagem tem de LER o ledger. As duas chamadas —
    /// sem filtro e com filtro — vivem em `run_list`; se alguem as remover, o
    /// comando passa a imprimir "nenhum run" para sempre e nenhum teste de
    /// saida notaria. Os `concat!` impedem o teste de casar consigo mesmo.
    #[test]
    fn listagem_le_o_ledger_de_verdade() {
        let corpo = corpo_de(concat!("pub fn run_", "list("));
        assert!(
            corpo.contains(concat!("list_recent_agent", "_runs(")),
            "run_list deve ler o ledger sem filtro (#1227 slice 4)"
        );
        assert!(
            corpo.contains(concat!("list_recent_agent_runs_by", "_status(")),
            "run_list deve ler o ledger filtrado por status (#1227 slice 4)"
        );
    }

    /// `goal` e payload do operador: sai por `stdout` para quem pediu a
    /// listagem e nunca por log estruturado (CLAUDE.md §6). A garantia mais
    /// barata e nao haver uma unica emissao de log no modulo — o scan olha o
    /// fonte inteiro, incluindo este proprio bloco de teste.
    #[test]
    fn nada_deste_comando_vai_para_o_log() {
        let src = include_str!("runs_cmd.rs");
        for agulha in [concat!("tra", "cing::"), concat!("lo", "g::")] {
            assert_eq!(
                src.matches(agulha).count(),
                0,
                "a saida deste comando e stdout, nunca log estruturado ({agulha})"
            );
        }
    }

    #[test]
    fn status_desconhecido_nao_vira_running() {
        assert_eq!(parse_status("done"), Some(RunStatus::Done));
        assert_eq!(parse_status("  INTERRUPTED "), Some(RunStatus::Interrupted));
        assert_eq!(parse_status("dnoe"), None);
        assert_eq!(parse_status(""), None);
    }

    /// O `datetime('now')` do SQLite e UTC sem dizer que e: a marca `Z` e
    /// posta na leitura, e um valor ilegivel volta como veio.
    #[test]
    fn instante_do_sqlite_ganha_o_z() {
        assert_eq!(iso8601_utc("2026-09-21 12:34:56"), "2026-09-21T12:34:56Z");
        assert_eq!(iso8601_utc("2026-09-21T12:34:56Z"), "2026-09-21T12:34:56Z");
        // Com fuso: normaliza para UTC em vez de repetir o offset.
        assert_eq!(
            iso8601_utc("2026-09-21T09:34:56-03:00"),
            "2026-09-21T12:34:56Z"
        );
        assert_eq!(iso8601_utc("nao-e-data"), "nao-e-data");
    }

    #[test]
    fn preview_nao_corta_no_meio_de_caractere() {
        assert_eq!(preview("coração de leão", 7), "coração…");
        assert_eq!(preview("curto", 40), "curto");
        assert_eq!(preview("uma\nlinha", 40), "uma linha");
    }

    /// `AppConfig` minima apontando o `data_dir` para um diretorio de teste.
    fn config_em(dir: &Path) -> AppConfig {
        AppConfig {
            data_dir: Some(dir.to_path_buf()),
            ..Default::default()
        }
    }

    fn semeia(dir: &Path) -> PathBuf {
        let path = dir.join(SESSIONS_DB);
        let store = SessionStore::open(&path).expect("cria o banco");
        store
            .start_agent_run("run-done", Some("s-1"), "rodar o build", Some("heartbeat"))
            .expect("abre");
        store
            .finish_agent_run("run-done", RunStatus::Done, Some("verde"), None)
            .expect("fecha");
        store
            .start_agent_run("run-vivo", None, "tarefa em voo", Some("heartbeat"))
            .expect("abre");
        // Simula a subida depois de uma queda: o `running` vira `interrupted`.
        garraia_db::agent_runs::log_interrupted_runs(&store);
        path
    }

    /// Banco ausente nao e erro: `garra runs list` numa instalacao nova sai 0
    /// e — isto e o que importa — **nao cria** o `sessions.db`.
    #[test]
    fn sem_banco_sai_zero_e_nao_cria_o_arquivo() {
        let dir = tempfile::tempdir().expect("tempdir");
        let config = config_em(dir.path());

        assert_eq!(
            run_list(&config, None, LIMITE_PADRAO, false).expect("list"),
            EXIT_OK
        );
        assert_eq!(
            run_list(&config, None, LIMITE_PADRAO, true).expect("list --json"),
            EXIT_OK
        );
        assert!(
            !dir.path().join(SESSIONS_DB).exists(),
            "uma leitura criou o banco"
        );
    }

    /// Banco vazio tambem nao e erro.
    #[test]
    fn banco_vazio_sai_zero() {
        let dir = tempfile::tempdir().expect("tempdir");
        SessionStore::open(&dir.path().join(SESSIONS_DB)).expect("cria o banco");

        let config = config_em(dir.path());
        assert_eq!(
            run_list(&config, None, LIMITE_PADRAO, false).expect("list"),
            EXIT_OK
        );
        assert_eq!(
            run_list(&config, Some("done".to_string()), LIMITE_PADRAO, true).expect("json"),
            EXIT_OK
        );
    }

    /// O caminho completo: dois runs semeados, a listagem devolve os dois, o
    /// filtro isola cada um e o `--json` fecha o contrato de chaves.
    #[test]
    fn lista_os_runs_semeados_e_filtra_por_status() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = semeia(dir.path());
        let config = config_em(dir.path());

        assert_eq!(
            run_list(&config, None, LIMITE_PADRAO, false).expect("list"),
            EXIT_OK
        );
        assert_eq!(
            run_list(&config, Some("interrupted".to_string()), 10, true).expect("json"),
            EXIT_OK
        );

        // O conteudo se verifica pelo store, que e a fonte do que foi impresso.
        let store = SessionStore::open(&path).expect("reabre");
        let todos = store.list_recent_agent_runs(LIMITE_PADRAO).expect("todos");
        assert_eq!(todos.len(), 2, "{todos:?}");

        let dones = store
            .list_recent_agent_runs_by_status(&RunStatus::Done, 10)
            .expect("done");
        assert_eq!(dones.len(), 1);
        assert_eq!(dones[0].id, "run-done");

        let interrompidos = store
            .list_recent_agent_runs_by_status(&RunStatus::Interrupted, 10)
            .expect("interrupted");
        assert_eq!(interrompidos.len(), 1);
        assert_eq!(interrompidos[0].id, "run-vivo");

        // Contrato do `--json`: array valido, chaves esperadas, instantes com `Z`.
        let payload: Vec<_> = todos.iter().map(run_json).collect();
        let texto = serde_json::to_string(&payload).expect("serializa");
        let de_volta: serde_json::Value = serde_json::from_str(&texto).expect("json valido");
        let arr = de_volta.as_array().expect("array");
        assert_eq!(arr.len(), 2);
        for item in arr {
            for chave in [
                "id",
                "session_id",
                "status",
                "mode",
                "goal",
                "started_at",
                "finished_at",
                "result_snippet",
                "error_snippet",
            ] {
                assert!(item.get(chave).is_some(), "falta `{chave}` em {item}");
            }
            let inicio = item["started_at"].as_str().expect("started_at e texto");
            assert!(inicio.ends_with('Z'), "instante sem `Z`: {inicio}");
        }

        let fechado = arr
            .iter()
            .find(|i| i["id"] == "run-done")
            .expect("o run fechado");
        assert!(
            fechado["finished_at"]
                .as_str()
                .expect("finished_at e texto")
                .ends_with('Z'),
            "{fechado}"
        );
        assert_eq!(fechado["status"], "done");
        assert_eq!(fechado["mode"], "heartbeat");
        assert_eq!(fechado["session_id"], "s-1");
    }

    /// Status invalido e erro de uso, nao "nenhum run": um erro de digitacao
    /// nao pode se passar por lista vazia.
    #[test]
    fn status_invalido_e_limite_zero_sao_erro_de_uso() {
        let dir = tempfile::tempdir().expect("tempdir");
        semeia(dir.path());
        let config = config_em(dir.path());

        assert_eq!(
            run_list(&config, Some("dnoe".to_string()), 10, false).expect("list"),
            EXIT_USAGE
        );
        assert_eq!(run_list(&config, None, 0, false).expect("list"), EXIT_USAGE);
    }

    /// A listagem e somente leitura: ela nao converte `running` residual em
    /// `interrupted` — isso e do hook de subida (slice 1).
    #[test]
    fn listar_nao_marca_runs_como_interrompidos() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join(SESSIONS_DB);
        {
            let store = SessionStore::open(&path).expect("cria o banco");
            store
                .start_agent_run("residual", None, "ficou em voo", None)
                .expect("abre");
        }

        let config = config_em(dir.path());
        assert_eq!(
            run_list(&config, None, LIMITE_PADRAO, false).expect("list"),
            EXIT_OK
        );

        let store = SessionStore::open(&path).expect("reabre");
        let runs = store.list_recent_agent_runs(10).expect("lista");
        assert_eq!(
            runs[0].status,
            RunStatus::Running,
            "a listagem escreveu no ledger"
        );
    }

    #[test]
    fn linha_humana_carrega_instante_com_z_e_o_modo() {
        let run = AgentRunRow {
            id: "run-1".to_string(),
            session_id: Some("s-1".to_string()),
            goal: "rodar o build".to_string(),
            mode: Some("heartbeat".to_string()),
            status: RunStatus::Done,
            started_at: "2026-09-21 12:34:56".to_string(),
            finished_at: Some("2026-09-21 12:35:10".to_string()),
            result_snippet: None,
            error_snippet: None,
        };
        let linha = run_line(&run);
        assert!(linha.contains("2026-09-21T12:34:56Z"), "{linha}");
        assert!(linha.contains("2026-09-21T12:35:10Z"), "{linha}");
        assert!(linha.contains("heartbeat"), "{linha}");
        assert!(linha.contains("run-1"), "{linha}");
        assert!(linha.contains("s-1"), "{linha}");

        // Run aberto: sem instante de fim, e sem mentir um.
        let aberto = AgentRunRow {
            finished_at: None,
            status: RunStatus::Running,
            ..run
        };
        let linha = run_line(&aberto);
        assert!(linha.contains("running"), "{linha}");
        assert!(!linha.contains("12:35:10"), "{linha}");
    }
}
