//! `garra runs` — le o ledger de runs de agente (#1227, slice 4).
//!
//! # O que este comando e, e o que ele nao e
//!
//! E um leitor do `sessions.db`, pelo mesmo caminho que o `garra logs` usa
//! para o arquivo de log: **nao** fala com o gateway, nao abre socket e nao
//! pede que nada esteja rodando. E o que o torna util justamente depois de
//! uma queda, quando a pergunta e "o que estava em voo?".
//!
//! **Ele nao cria o arquivo.** Numa instalacao que nunca rodou nada, `garra
//! runs list` sai 0 dizendo que nao ha ledger e deixa o disco como estava —
//! ha teste afirmando a ausencia do `sessions.db` depois da listagem. E o
//! que "somente leitura" quer dizer aqui, em forma falsificavel. Na mesma
//! linha, ele nao marca run `running` orfao como `interrupted`: essa
//! conversao e do hook de subida (`garraia_db::agent_runs::
//! log_interrupted_runs`, slice 1), que roda no boot do gateway e na
//! abertura do store pelo `chat`. Uma listagem que escrevesse mudaria o que
//! ela mesma esta reportando.
//!
//! # Codigos de saida (sysexits, iguais aos do `garra memory`)
//!
//! - `0`  — ok. **Lista vazia e 0**: "nenhum run" e uma resposta, nao um erro.
//! - `2`  — `EX_USAGE`: `--status` desconhecido, `--limit 0`.
//! - `74` — `EX_IOERR`: o banco existe mas nao abre.
//!
//! # Sobre conteudo
//!
//! `agent_runs.goal` **nao** e texto de confianca. Quem grava a coluna e o
//! `ledger_inicia_run_agendado` do gateway, com o `payload` da tarefa
//! agendada — e esse payload pode ter nascido de uma tool call de LLM
//! disparada por qualquer usuario de canal. Como a listagem escreve direto
//! no terminal do operador, todo campo da saida humana passa por
//! [`sanitize_control_chars`]: sem isso um `goal` com `\x1b[2K` limparia a
//! linha, escondendo os runs vizinhos de quem esta justamente investigando
//! uma queda.
//!
//! Esse conteudo sai por `stdout`, no terminal de quem pediu a listagem — e
//! **nunca** por log estruturado. A regra do projeto (CLAUDE.md §6) e sobre
//! log, e o hook de subida ja a respeita registrando so ids. Este modulo nao
//! emite uma linha de log de proposito, e ha teste varrendo o fonte atras de
//! uma. Os dois trechos de provider (`result_snippet`/`error_snippet`) saem
//! so no `--json`, e ainda assim redigidos por
//! `crate::ask::sanitize_provider_error`: corpo de erro de provider ja
//! chegou com chave de API dentro.
//!
//! # Por que nao ha `runs resume` (#1227, slice 7: wont-do)
//!
//! Run `interrupted` de tarefa agendada (`mode = heartbeat`, o unico produtor
//! de producao do ledger hoje) **ja e retomado sozinho**: a subida do gateway
//! roda `recover_expired_leases` (`garraia-db`, `session_store.rs`), que
//! devolve a tarefa `running` de lease vencido para `pending`, e o proximo
//! tick do scheduler a executa de novo — gravando um run novo no ledger. Um
//! `resume` manual sobre esse run executaria a tarefa **duas vezes**:
//! mensagem de sistema e entrega no canal em dobro, o bug que o slice 2
//! fechou.
//!
//! Alem disso o ledger nao tem como reexecutar fielmente: `goal` e cortado em
//! 500 caracteres na gravacao e nao ha coluna ligando o run a tarefa que o
//! gerou. E o caminho do `AgentCoordinator` (sub-agentes) nao tem chamador de
//! producao. Um `resume` que reexecuta LLM com tools e entrega em canal e
//! poder de escrita novo (R4) e so faz sentido quando existir um consumidor
//! real do coordinator — com confirmacao explicita e criando um run novo que
//! referencia o original, nunca reabrindo o antigo.

use std::path::{Path, PathBuf};

use anyhow::Result;
use garraia_config::AppConfig;
use garraia_db::{AgentRunRow, RunStatus, SessionStore};
// #1227 (slice 6): os leitores do ledger moram no `garraia-db` porque o
// `GET /api/runs` do gateway le o mesmo banco — duas copias seriam dois
// formatos de instante e duas higienizacoes para divergir em silencio.
use garraia_db::agent_runs::{
    iso8601_utc, parse_run_status as parse_status, preview, sanitize_control_chars,
};

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

/// Nota impressa quando a listagem traz run `interrupted` (#1227, slice 7).
/// Ver "Por que nao ha `runs resume`" no doc do modulo.
const NOTA_INTERRUPTED: &str = "\nNota: run `interrupted` de tarefa agendada nao precisa de retomada manual: \
     na subida,\no scheduler devolve a tarefa para a fila e o proximo tick a executa \
     de novo, gravando um run novo.";

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
    println!(
        "O ledger e criado quando o gateway sobe (`garra start`) ou quando o `garra chat`\n\
         roda com `--persist` ou `--resume` — uma conversa sem essas flags nao grava nada."
    );
}

/// Um run em JSON. Contrato estavel de chaves — quem pediu `--json` esta
/// scriptando. Os dois instantes saem normalizados com `Z`; o `serde` do
/// `AgentRunRow` devolveria a string crua do SQLite, sem fuso.
///
/// Os dois trechos de desfecho passam por `sanitize_provider_error`: o
/// `error_snippet` guarda corpo de erro de provider, e corpo de erro de
/// provider ja veio com `sk-or-v1-…` ecoado dentro. Redigir na leitura e
/// barato e vale mesmo para o ledger local — `--json` existe para ser
/// canalizado para outro lugar.
///
/// Controle de terminal **nao** e tratado aqui de proposito: a RFC 8259
/// exige que `serde_json` escape C0 (U+0000-U+001F, o que cobre ESC e CR)
/// como `\u00XX`, entao a saida `--json` nunca carrega esses bytes crus.
/// C1 (U+0080-U+009F) nao e coberto pela spec e sai como UTF-8 normal, mas
/// so importa se alguem despejar o JSON cru direto num terminal em vez de
/// canaliza-lo para `jq` ou um arquivo. Higienizar de novo so criaria um
/// segundo comportamento para manter.
pub(crate) fn run_json(run: &AgentRunRow) -> serde_json::Value {
    let redigir = |s: &String| crate::ask::sanitize_provider_error(s);
    serde_json::json!({
        "id": run.id,
        "session_id": run.session_id,
        "status": run.status.as_str(),
        "mode": run.mode,
        "goal": run.goal,
        "started_at": iso8601_utc(&run.started_at),
        "finished_at": run.finished_at.as_deref().map(iso8601_utc),
        "result_snippet": run.result_snippet.as_ref().map(redigir),
        "error_snippet": run.error_snippet.as_ref().map(redigir),
    })
}

/// Um run em duas linhas de terminal: cabecalho e previa do objetivo.
///
/// O objetivo vai na **segunda** linha, e nao no fim da primeira, porque ele
/// e truncado: pendurar campo de tamanho fixo depois de um texto cortado
/// embaralha os dois.
///
/// O `session_id` **nao** aparece na saida humana. Ele sai inteiro no
/// `--json`, que e por onde alguem correlaciona runs com sessoes de verdade;
/// na linha de terminal ele so ocupava largura e ainda fazia o `println!`
/// cair no `rust/cleartext-logging` do CodeQL (alerta #175). Tirar o campo
/// resolve os dois de uma vez, sem mexer no ledger de supressoes.
///
/// Todo campo passa por [`sanitize_control_chars`] — `id` e `mode` tambem,
/// nao so o `goal`: sao colunas do mesmo banco, e um leitor nao deve
/// precisar saber qual delas o gateway preencheu.
pub(crate) fn run_line(run: &AgentRunRow) -> String {
    let fim = match &run.finished_at {
        Some(f) => iso8601_utc(f),
        // Largura da coluna do instante, para as linhas nao dancarem.
        None => "—                   ".to_string(),
    };
    let modo = sanitize_control_chars(run.mode.as_deref().unwrap_or("—"));
    format!(
        "{:<11}  {}  {}  {:<10}  {}\n    {}",
        run.status.as_str(),
        iso8601_utc(&run.started_at),
        fim,
        modo,
        sanitize_control_chars(&run.id),
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

    // #1227 (slice 7): sem esta nota o operador procura um comando de
    // retomada que nao existe — e nao precisa existir para tarefa agendada.
    if runs.iter().any(|r| r.status == RunStatus::Interrupted) {
        println!("{}", NOTA_INTERRUPTED);
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

    /// #1227 (slice 7): resume e wont-do, e a listagem diz por que quando
    /// mostra um `interrupted` — sem a nota o operador procura um comando
    /// que nao existe.
    #[test]
    fn listagem_explica_que_interrompido_agendado_se_retoma_sozinho() {
        let corpo = corpo_de(concat!("pub fn run_", "list("));
        assert!(corpo.contains(concat!("NOTA_", "INTERRUPTED")), "{corpo}");
        assert!(NOTA_INTERRUPTED.contains("scheduler"));
        assert!(NOTA_INTERRUPTED.contains("run novo"));
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

    /// `goal` pode vir de tool call de LLM, e a linha humana vai direto para
    /// o terminal do operador: nenhum byte de controle pode sobreviver ao
    /// caminho. O payload de ataque aqui limparia a linha (`\x1b[2K`) e
    /// voltaria o cursor (`\r`), escondendo os runs vizinhos.
    #[test]
    fn controle_de_terminal_nao_sobrevive_a_saida_humana() {
        let hostil = "\x1b[31mred\r\x1b[2K";

        let limpo = sanitize_control_chars(hostil);
        assert!(!limpo.contains('\x1b'), "ESC cru sobreviveu: {limpo:?}");
        assert!(!limpo.contains('\r'), "CR cru sobreviveu: {limpo:?}");
        assert_eq!(limpo, "\u{FFFD}[31mred\u{FFFD}\u{FFFD}[2K");
        // Texto legitimo, incluindo acento, atravessa intacto.
        assert_eq!(sanitize_control_chars("coração"), "coração");

        // E o mesmo vale pela linha inteira, nao so pela funcao solta:
        // `goal`, `id` e `mode` sao todos campos do banco.
        let run = AgentRunRow {
            id: format!("run{hostil}"),
            session_id: Some("s-1".to_string()),
            goal: format!("objetivo {hostil}"),
            mode: Some(format!("modo{hostil}")),
            status: RunStatus::Done,
            started_at: "2026-09-21 12:34:56".to_string(),
            finished_at: None,
            result_snippet: None,
            error_snippet: None,
        };
        let linha = run_line(&run);
        assert!(!linha.contains('\x1b'), "ESC cru na linha: {linha:?}");
        assert!(!linha.contains('\r'), "CR cru na linha: {linha:?}");
        assert!(linha.contains('\u{FFFD}'), "{linha:?}");
        // A quebra que separa cabecalho de objetivo continua sendo a unica.
        assert_eq!(linha.matches('\n').count(), 1, "{linha:?}");
    }

    /// `error_snippet` guarda corpo de erro de provider, que ja chegou com
    /// chave de API ecoada dentro. O `--json` e feito para ser canalizado
    /// para outro lugar, entao a redacao acontece na leitura.
    #[test]
    fn json_redige_chave_de_api_nos_trechos_de_provider() {
        let run = AgentRunRow {
            id: "run-1".to_string(),
            session_id: None,
            goal: "rodar o build".to_string(),
            mode: None,
            status: RunStatus::Error,
            started_at: "2026-09-21 12:34:56".to_string(),
            finished_at: Some("2026-09-21 12:35:10".to_string()),
            result_snippet: Some("chave sk-or-v1-abc123def456 no resultado".to_string()),
            error_snippet: Some("401 from sk-or-v1-abc123def456".to_string()),
        };

        let texto = serde_json::to_string(&run_json(&run)).expect("serializa");
        assert!(!texto.contains("abc123def456"), "{texto}");
        assert!(texto.contains("[REDACTED]"), "{texto}");
        // Os dois campos, nao so o erro.
        for chave in ["result_snippet", "error_snippet"] {
            let v = run_json(&run)[chave].as_str().map(str::to_string);
            let v = v.unwrap_or_else(|| panic!("`{chave}` e texto"));
            assert!(v.contains("[REDACTED]"), "{chave}: {v}");
        }

        // Ausente continua ausente — a redacao nao inventa string vazia.
        let vazio = AgentRunRow {
            result_snippet: None,
            error_snippet: None,
            ..run
        };
        assert!(run_json(&vazio)["error_snippet"].is_null());
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

        // O `session_id` saiu da linha humana (CodeQL #175): quem precisa
        // dele usa `--json`, e e la que o teste de contrato o cobra.
        assert!(!linha.contains("sessao"), "{linha}");
        assert!(!linha.contains("s-1"), "{linha}");
        assert_eq!(run_json(&run)["session_id"], "s-1");

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
