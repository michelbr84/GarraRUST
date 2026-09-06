//! `garra memory` — inspeciona, repara e limpa a memoria semantica (#950).
//!
//! Ate aqui a memoria era uma caixa-preta: o operador nao tinha como saber
//! quantas entradas existiam, quantas tinham vetor, se o indice estava
//! consistente, nem como consertar as que a cadeia de perda silenciosa
//! (#948/#951/#962) deixou sem embedding. O `docs/src/memory.md` chegava a
//! documentar seis subcomandos que nunca existiram.
//!
//! Codigos de saida (sysexits, iguais aos do `garra config check`):
//!
//! - `0`  — ok.
//! - `2`  — `EX_USAGE`: argumento invalido, id inexistente, confirmacao negada.
//! - `69` — `EX_UNAVAILABLE`: a operacao exige um provider de embeddings e nao
//!   ha nenhum configurado.
//! - `74` — `EX_IOERR`: o banco existe mas nao abre.
//!
//! Sobre conteudo: as entradas sao dados do proprio operador, no terminal do
//! proprio operador — listar e o objetivo da ferramenta. O que **nao** pode
//! acontecer e conteudo de memoria virar log estruturado, entao nada aqui vai
//! para o `tracing`; tudo sai por `stdout`.

use std::io::IsTerminal;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use chrono::{DateTime, Duration, Utc};
use garraia_agents::{ReindexOptions, reindex_missing_embeddings};
use garraia_config::AppConfig;
use garraia_db::{MemoryEntry, MemoryStore, RecallQuery};

/// Tudo certo.
const EXIT_OK: i32 = 0;
/// `EX_USAGE` — argumento invalido, id inexistente, confirmacao negada.
const EXIT_USAGE: i32 = 2;
/// `EX_UNAVAILABLE` — a operacao precisa de um provider de embeddings.
const EXIT_UNAVAILABLE: i32 = 69;
/// `EX_IOERR` — o arquivo existe mas nao abre.
const EXIT_IOERR: i32 = 74;

/// Quanto do conteudo aparece numa listagem humana.
///
/// Corta por **caractere**, nao por byte: memoria em portugues e cheia de
/// acento, e cortar no meio de um `ç` entrega mojibake ou entra em panico.
const PREVIEW_CHARS: usize = 110;

/// O que aconteceu ao tentar abrir o banco.
enum Opened {
    Store(Box<MemoryStore>, PathBuf),
    /// Nao ha banco ainda. Nao e erro: cada subcomando decide o que dizer.
    Absent(PathBuf),
    /// Existe e nao abre. A mensagem ja foi para `stderr`.
    Failed,
}

/// Abre o banco de memoria, sem cria-lo.
///
/// `MemoryStore::open` cria o arquivo se ele nao existir — util no boot do
/// gateway, ruim aqui: `garra memory stats` numa instalacao que nunca
/// conversou criaria um banco vazio como efeito colateral de uma leitura.
fn open_store(config: &AppConfig) -> Opened {
    let path = config.memory_db_path();
    if !path.exists() {
        return Opened::Absent(path);
    }
    match MemoryStore::open(&path) {
        Ok(store) => Opened::Store(Box::new(store), path),
        Err(e) => {
            eprintln!(
                "error: o banco de memoria existe mas nao abre ({}): {e}",
                path.display()
            );
            Opened::Failed
        }
    }
}

/// Mensagem unica para o caso "o banco ainda nao existe".
fn report_no_store(path: &Path) {
    println!("Nenhuma memoria ainda: {} nao existe.", path.display());
    println!("Ela e criada na primeira conversa com `memory.enabled: true` na config.");
}

/// Corta o conteudo para caber numa linha, respeitando limites de caractere.
pub(crate) fn preview(content: &str, max_chars: usize) -> String {
    let mut out: String = content.chars().take(max_chars).collect();
    // `chars().count()` so e pago quando o corte pode ter acontecido.
    if out.chars().count() < content.chars().count() {
        out.push('…');
    }
    out.replace('\n', " ")
}

/// Instante de corte para o `compact`, a partir de uma idade em dias.
pub(crate) fn cutoff_from_days(now: DateTime<Utc>, days: i64) -> Option<DateTime<Utc>> {
    if days < 0 {
        return None;
    }
    // `Duration::days` entra em **panico** acima de ~106 bilhoes de dias, e
    // `--older-than-days` e um `i64` que o operador digita: sem o `try_`, um
    // numero grande derruba o processo antes de o `checked_sub_signed` ter
    // chance de recusar.
    now.checked_sub_signed(Duration::try_days(days)?)
}

/// Uma entrada em JSON. O conteudo vai inteiro — quem pediu `--json` esta
/// scriptando, e truncar ali silenciosamente corromperia o dado.
fn entry_json(entry: &MemoryEntry) -> serde_json::Value {
    serde_json::json!({
        "id": entry.id,
        "session_id": entry.session_id,
        "role": role_str(entry),
        "content": entry.content,
        "created_at": entry.created_at.to_rfc3339(),
        "has_embedding": entry.embedding.is_some(),
        "embedding_model": entry.embedding_model,
        "embedding_dimensions": entry.embedding_dimensions,
    })
}

/// Uma entrada em duas linhas de terminal: cabecalho e previa do conteudo.
///
/// A distancia, quando ha, vai no **cabecalho** e nao depois da previa: o
/// conteudo e truncado, e pendurar um numero no fim de um texto cortado
/// confunde os dois.
fn entry_line(entry: &MemoryEntry, distancia: Option<f64>) -> String {
    let vetor = match (&entry.embedding, &entry.embedding_model) {
        (None, _) => "sem vetor".to_string(),
        (Some(_), None) => "vetor sem modelo".to_string(),
        (Some(_), Some(m)) => m.clone(),
    };
    let distancia = match distancia {
        Some(d) => format!("  d={d:.4}"),
        None => String::new(),
    };
    format!(
        "{}  {}  [{}]  ({}){}\n    {}",
        entry.created_at.format("%Y-%m-%d %H:%M:%SZ"),
        entry.id,
        vetor,
        role_str(entry),
        distancia,
        preview(&entry.content, PREVIEW_CHARS),
    )
}

fn role_str(entry: &MemoryEntry) -> &'static str {
    match entry.role {
        garraia_db::MemoryRole::User => "user",
        garraia_db::MemoryRole::Assistant => "assistant",
        garraia_db::MemoryRole::System => "system",
        garraia_db::MemoryRole::Tool => "tool",
    }
}

/// `garra memory stats` — contagens e integridade do indice vetorial (#960).
pub async fn run_stats(config: &AppConfig, json: bool) -> Result<i32> {
    let (store, path) = match open_store(config) {
        Opened::Store(store, path) => (*store, path),
        Opened::Absent(path) => {
            if json {
                let payload = serde_json::json!({
                    "db_path": path.display().to_string(),
                    "exists": false,
                });
                println!("{}", serde_json::to_string_pretty(&payload)?);
            } else {
                report_no_store(&path);
            }
            return Ok(EXIT_OK);
        }
        Opened::Failed => return Ok(EXIT_IOERR),
    };

    let report = store
        .integrity_report()
        .context("failed to build integrity report")?;
    let breakdown = store
        .embedding_breakdown()
        .context("failed to build per-model breakdown")?;
    let knn = store.knn_enabled();

    if json {
        let payload = serde_json::json!({
            "db_path": path.display().to_string(),
            "exists": true,
            "knn_enabled": knn,
            "entries_total": report.entries_total,
            "entries_with_embedding": report.entries_with_embedding,
            "entries_without_embedding": report.entries_without_embedding,
            "entries_missing_model": report.entries_missing_model,
            "map_rows": report.map_rows,
            "vec_rows_by_table": report.vec_rows_by_table
                .iter()
                .map(|(t, n)| serde_json::json!({ "table": t, "rows": n }))
                .collect::<Vec<_>>(),
            "orphan_map_entries": report.orphan_map_entries,
            "by_model": breakdown,
        });
        println!("{}", serde_json::to_string_pretty(&payload)?);
        return Ok(EXIT_OK);
    }

    println!("Banco:              {}", path.display());
    println!(
        "Busca vetorial:     {}",
        if knn {
            "ativa (sqlite-vec)"
        } else {
            "DESLIGADA — o recall cai para o caminho textual"
        }
    );
    println!("Entradas:           {}", report.entries_total);
    println!("  com vetor:        {}", report.entries_with_embedding);
    println!("  sem vetor:        {}", report.entries_without_embedding);
    println!("  vetor sem modelo: {}", report.entries_missing_model);
    println!("Linhas no mapa:     {}", report.map_rows);
    for (table, rows) in &report.vec_rows_by_table {
        println!("  {table}: {rows}");
    }

    // "Com o que este indice foi construido?" e a pergunta que decide se uma
    // troca de modelo exige reindexar tudo (#954) — e ela nao se responde
    // com um agregado.
    if !breakdown.is_empty() {
        println!("\nPor modelo:");
        for linha in &breakdown {
            let rotulo = match (&linha.embedding_model, linha.embedding_dimensions) {
                (Some(m), Some(d)) => format!("{m} ({d} dimensoes)"),
                (Some(m), None) => format!("{m} (dimensao nao registrada)"),
                (None, Some(d)) => format!("sem modelo registrado ({d} dimensoes)"),
                (None, None) => "sem vetor".to_string(),
            };
            println!("  {rotulo}: {}", linha.entries);
        }
    }

    if !report.orphan_map_entries.is_empty() {
        println!(
            "\nOrfaos no indice: {} (vetor mapeado sem entrada correspondente)",
            report.orphan_map_entries.len()
        );
        for id in report.orphan_map_entries.iter().take(10) {
            println!("  {id}");
        }
        if report.orphan_map_entries.len() > 10 {
            println!("  … e mais {}", report.orphan_map_entries.len() - 10);
        }
    }

    if report.entries_without_embedding > 0 {
        println!(
            "\n{} entrada(s) sem vetor. Elas so entram no recall pelo caminho textual;\n\
             `garra memory reindex` repovoa o indice.",
            report.entries_without_embedding
        );
    }
    if report.entries_missing_model > 0 {
        println!(
            "\n{} entrada(s) tem vetor mas nao registram o modelo que o produziu (legado\n\
             de antes do #954). Elas perdem o eixo semantico do score sempre que o recall\n\
             chega com um modelo definido — `garra memory reindex` tambem conserta isso.",
            report.entries_missing_model
        );
    }

    Ok(EXIT_OK)
}

/// `garra memory list` — as entradas mais recentes, ou so a fila de reindex.
pub async fn run_list(
    config: &AppConfig,
    limit: usize,
    only_missing: bool,
    json: bool,
) -> Result<i32> {
    if limit == 0 {
        eprintln!("error: --limit precisa ser maior que zero");
        return Ok(EXIT_USAGE);
    }

    let store = match open_store(config) {
        Opened::Store(store, _) => *store,
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

    let entries = if only_missing {
        store
            .entries_missing_embeddings(limit)
            .context("failed to list entries without embedding")?
    } else {
        store
            .recent_entries(limit)
            .context("failed to list recent entries")?
    };

    if json {
        let payload: Vec<_> = entries.iter().map(entry_json).collect();
        println!("{}", serde_json::to_string_pretty(&payload)?);
        return Ok(EXIT_OK);
    }

    if entries.is_empty() {
        if only_missing {
            println!("Nenhuma entrada sem vetor — a fila de reindexacao esta vazia.");
        } else {
            println!("Nenhuma entrada na memoria.");
        }
        return Ok(EXIT_OK);
    }

    for entry in &entries {
        println!("{}", entry_line(entry, None));
    }
    Ok(EXIT_OK)
}

/// `garra memory search` — recall pelo mesmo caminho que o agente usa.
///
/// Com provider configurado a busca e semantica (KNN + score hibrido, com o
/// filtro de modelo do #954); sem provider, cai para o caminho textual — o
/// mesmo fallback do gateway, e o subcomando diz qual dos dois rodou para o
/// operador nao confundir "nao achou" com "nao havia como achar".
pub async fn run_search(
    config: &AppConfig,
    query: String,
    limit: usize,
    json: bool,
) -> Result<i32> {
    if query.trim().is_empty() {
        eprintln!("error: a busca precisa de um termo");
        return Ok(EXIT_USAGE);
    }
    if limit == 0 {
        eprintln!("error: --limit precisa ser maior que zero");
        return Ok(EXIT_USAGE);
    }

    let store = match open_store(config) {
        Opened::Store(store, _) => *store,
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

    let provider = garraia_gateway::bootstrap::build_embedding_provider(config);

    let (query_embedding, embedding_model) = match &provider {
        Some(p) => match p.embed_query(&query).await {
            Ok(vetor) => (Some(vetor), Some(p.model().to_string())),
            Err(e) => {
                eprintln!(
                    "aviso: o provider de embeddings falhou ({e}); \
                     caindo para a busca textual"
                );
                (None, None)
            }
        },
        None => (None, None),
    };

    let semantica = query_embedding.is_some();

    // Distancias cruas do indice, so para anotar o que o recall devolver.
    // A ordem da lista continua sendo a do score hibrido do recall — que
    // combina distancia, recencia e casamento textual —, entao ela nao e
    // monotona na distancia, e o rotulo diz "d=" em vez de "rank".
    let distancias: std::collections::HashMap<String, f64> = match &query_embedding {
        Some(vetor) => store
            .knn_distances(vetor, limit.saturating_mul(4))
            .unwrap_or_default()
            .into_iter()
            .collect(),
        None => std::collections::HashMap::new(),
    };

    let results = store
        .recall(RecallQuery {
            tenant_id: None,
            query_text: Some(query.clone()),
            query_embedding,
            embedding_model,
            session_id: None,
            continuity_key: None,
            limit,
        })
        .await
        .context("recall failed")?;

    if json {
        let payload = serde_json::json!({
            "semantic": semantica,
            "results": results
                .iter()
                .map(|entry| {
                    let mut valor = entry_json(entry);
                    if let Some(objeto) = valor.as_object_mut() {
                        objeto.insert(
                            "distance".to_string(),
                            match distancias.get(&entry.id) {
                                Some(d) => serde_json::json!(d),
                                None => serde_json::Value::Null,
                            },
                        );
                    }
                    valor
                })
                .collect::<Vec<_>>(),
        });
        println!("{}", serde_json::to_string_pretty(&payload)?);
        return Ok(EXIT_OK);
    }

    // "Caiu para textual porque nao ha provider" e "caiu porque o provider
    // esta fora" levam a acoes diferentes; o cabecalho diz qual dos dois foi.
    let modo = match (semantica, provider.is_some()) {
        (true, _) => "semantica",
        (false, true) => "textual (o provider de embeddings falhou)",
        (false, false) => "textual (sem provider de embeddings)",
    };
    println!("Busca {modo} — {} resultado(s)\n", results.len());
    for entry in &results {
        println!("{}", entry_line(entry, distancias.get(&entry.id).copied()));
    }

    // O caminho textual do recall e um `LIKE '%frase inteira%'`: uma consulta
    // de duas palavras so casa se as duas aparecerem grudadas, na ordem. Sem
    // este aviso, "nao achou" e indistinguivel de "nao havia como achar".
    if !semantica && results.is_empty() && query.split_whitespace().count() > 1 {
        println!(
            "A busca textual casa a frase inteira, nao palavras soltas — tente um termo unico."
        );
        if provider.is_none() {
            println!(
                "Configurar `memory.embedding_provider` liga a busca semantica, que nao \
                 depende de casamento literal."
            );
        }
    }

    Ok(EXIT_OK)
}

/// `garra memory reindex` — repovoa os vetores que faltam (#953).
pub async fn run_reindex(
    config: &AppConfig,
    limit: Option<usize>,
    batch_size: usize,
    dry_run: bool,
    json: bool,
) -> Result<i32> {
    if batch_size == 0 {
        eprintln!("error: --batch-size precisa ser maior que zero");
        return Ok(EXIT_USAGE);
    }

    let store = match open_store(config) {
        Opened::Store(store, _) => *store,
        Opened::Absent(path) => {
            report_no_store(&path);
            return Ok(EXIT_OK);
        }
        Opened::Failed => return Ok(EXIT_IOERR),
    };

    // No dry-run nao ha chamada de provider, entao exigir um seria recusar a
    // pergunta "quanto tem para fazer?" justamente a quem ainda nao configurou.
    let provider = garraia_gateway::bootstrap::build_embedding_provider(config);
    if provider.is_none() && !dry_run {
        eprintln!(
            "error: nenhum provider de embeddings configurado — sem ele nao ha como\n\
             produzir os vetores. Aponte `memory.embedding_provider` para uma entrada\n\
             de `embeddings:` na config (`garra config check` mostra o efetivo)."
        );
        return Ok(EXIT_UNAVAILABLE);
    }

    let options = ReindexOptions {
        batch_size,
        max_entries: limit,
        dry_run,
    };

    let report = match &provider {
        Some(p) => reindex_missing_embeddings(&store, p.as_ref(), options)
            .await
            .context("reindex failed")?,
        // Dry-run sem provider: a fila e derivada do banco, e o relatorio de
        // integridade responde sozinho.
        None => garraia_agents::ReindexReport {
            pending_before: store.integrity_report()?.entries_without_embedding,
            index_repaired: 0,
            reindexed: 0,
            vanished: 0,
            stopped_early: false,
            dry_run: true,
        },
    };

    if json {
        let payload = serde_json::json!({
            "pending_before": report.pending_before,
            "index_repaired": report.index_repaired,
            "reindexed": report.reindexed,
            "vanished": report.vanished,
            "stopped_early": report.stopped_early,
            "dry_run": report.dry_run,
        });
        println!("{}", serde_json::to_string_pretty(&payload)?);
    } else if report.dry_run {
        println!(
            "{} entrada(s) sem vetor seriam reprocessadas (dry-run: nada foi gravado).",
            report.pending_before
        );
    } else {
        println!(
            "Fila inicial: {} | reindexadas: {} | sumiram no caminho: {}",
            report.pending_before, report.reindexed, report.vanished
        );
        if report.index_repaired > 0 {
            println!(
                "Alem dessas, {} entrada(s) ja tinham vetor na coluna e voltaram \
                 para o indice sem custo de provider.",
                report.index_repaired
            );
        }
        if report.stopped_early {
            println!(
                "\nA reindexacao parou no meio: o provider de embeddings falhou.\n\
                 Rode de novo quando ele voltar — a fila continua de onde parou."
            );
        }
    }

    // Parar no meio nao e sucesso: quem chamou de um script precisa saber.
    if report.stopped_early {
        return Ok(EXIT_UNAVAILABLE);
    }
    Ok(EXIT_OK)
}

/// `garra memory delete` — apaga uma entrada pelo id, junto do vetor.
pub async fn run_delete(config: &AppConfig, id: String, yes: bool) -> Result<i32> {
    let store = match open_store(config) {
        Opened::Store(store, _) => *store,
        Opened::Absent(path) => {
            report_no_store(&path);
            return Ok(EXIT_OK);
        }
        Opened::Failed => return Ok(EXIT_IOERR),
    };

    if !confirm(&format!("Apagar a entrada {id} (e o vetor dela)?"), yes)? {
        return Ok(EXIT_USAGE);
    }

    if store.delete_entry(&id).context("failed to delete entry")? {
        println!("Entrada {id} apagada.");
        Ok(EXIT_OK)
    } else {
        eprintln!("error: nenhuma entrada com o id {id}");
        Ok(EXIT_USAGE)
    }
}

/// `garra memory compact` — apaga tudo anterior a N dias.
pub async fn run_compact(config: &AppConfig, older_than_days: i64, yes: bool) -> Result<i32> {
    let Some(cutoff) = cutoff_from_days(Utc::now(), older_than_days) else {
        eprintln!("error: --older-than-days precisa ser um numero de dias nao-negativo");
        return Ok(EXIT_USAGE);
    };

    let store = match open_store(config) {
        Opened::Store(store, _) => *store,
        Opened::Absent(path) => {
            report_no_store(&path);
            return Ok(EXIT_OK);
        }
        Opened::Failed => return Ok(EXIT_IOERR),
    };

    let antes = store.integrity_report()?.entries_total;
    let pergunta = format!(
        "Apagar toda memoria anterior a {} ({} dia(s))? {} entrada(s) no banco hoje.",
        cutoff.format("%Y-%m-%d %H:%M:%SZ"),
        older_than_days,
        antes
    );
    if !confirm(&pergunta, yes)? {
        return Ok(EXIT_USAGE);
    }

    let report = store.compact(cutoff).await.context("compaction failed")?;
    println!(
        "{} entrada(s) apagadas (anteriores a {}).",
        report.deleted_entries,
        report.before.format("%Y-%m-%d %H:%M:%SZ")
    );
    Ok(EXIT_OK)
}

/// Confirmacao para operacoes destrutivas.
///
/// Sem TTY nao ha como perguntar, e assumir "sim" apagaria dado de quem so
/// esbarrou no comando dentro de um script: o caminho nao-interativo exige
/// `--yes` explicito.
fn confirm(prompt: &str, yes: bool) -> Result<bool> {
    if yes {
        return Ok(true);
    }
    if !std::io::stdin().is_terminal() {
        eprintln!("error: operacao destrutiva sem terminal para confirmar — use --yes");
        return Ok(false);
    }
    let answer = dialoguer::Confirm::new()
        .with_prompt(prompt)
        .default(false)
        .interact()
        .context("failed to read confirmation")?;
    if !answer {
        println!("Cancelado.");
    }
    Ok(answer)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preview_nao_corta_no_meio_de_caractere() {
        // "ç" e "ã" ocupam dois bytes: cortar por byte entraria em panico.
        let conteudo = "coração de leão";
        let curto = preview(conteudo, 7);
        assert_eq!(curto, "coração…");
    }

    #[test]
    fn preview_nao_marca_o_que_nao_cortou() {
        assert_eq!(preview("curto", 40), "curto");
    }

    #[test]
    fn preview_achata_quebras_de_linha() {
        assert_eq!(preview("uma\nlinha", 40), "uma linha");
    }

    #[test]
    fn cutoff_recusa_dias_negativos() {
        assert!(cutoff_from_days(Utc::now(), -1).is_none());
    }

    /// `Duration::days` entra em panico acima de ~106 bilhoes de dias, e
    /// `--older-than-days` e um numero que o operador digita: sem o `try_`,
    /// `garra memory compact --older-than-days 999999999999999` derrubava o
    /// processo com backtrace em vez de recusar o argumento.
    #[test]
    fn cutoff_recusa_dias_absurdos_em_vez_de_entrar_em_panico() {
        assert!(cutoff_from_days(Utc::now(), i64::MAX).is_none());
        assert!(cutoff_from_days(Utc::now(), 999_999_999_999_999).is_none());
    }

    #[test]
    fn cutoff_de_zero_dias_e_agora() {
        let agora = Utc::now();
        assert_eq!(cutoff_from_days(agora, 0), Some(agora));
    }

    /// `AppConfig` minima apontando o `data_dir` para um diretorio de teste.
    fn config_em(dir: &Path) -> AppConfig {
        AppConfig {
            data_dir: Some(dir.to_path_buf()),
            ..Default::default()
        }
    }

    fn entrada(conteudo: &str) -> garraia_db::NewMemoryEntry {
        garraia_db::NewMemoryEntry {
            tenant_id: "default".to_string(),
            session_id: "sessao-de-teste".to_string(),
            channel_id: None,
            user_id: None,
            continuity_key: None,
            role: garraia_db::MemoryRole::User,
            content: conteudo.to_string(),
            embedding: None,
            embedding_model: None,
            metadata: serde_json::Value::Null,
        }
    }

    /// A leitura nao pode ter efeito colateral: `stats` numa instalacao que
    /// nunca conversou nao cria um `memory.db` vazio.
    #[tokio::test]
    async fn stats_sem_banco_nao_cria_o_arquivo() {
        let dir = tempfile::tempdir().expect("tempdir");
        let config = config_em(dir.path());

        let code = run_stats(&config, false).await.expect("stats");

        assert_eq!(code, EXIT_OK);
        assert!(
            !config.memory_db_path().exists(),
            "uma leitura criou o banco"
        );
    }

    #[tokio::test]
    async fn list_com_limite_zero_e_erro_de_uso() {
        let dir = tempfile::tempdir().expect("tempdir");
        let code = run_list(&config_em(dir.path()), 0, false, false)
            .await
            .expect("list");
        assert_eq!(code, EXIT_USAGE);
    }

    #[tokio::test]
    async fn delete_de_id_inexistente_e_erro_de_uso() {
        let dir = tempfile::tempdir().expect("tempdir");
        let config = config_em(dir.path());
        MemoryStore::open(&config.memory_db_path()).expect("cria o banco");

        let code = run_delete(&config, "nao-existe".to_string(), true)
            .await
            .expect("delete");

        assert_eq!(code, EXIT_USAGE);
    }

    #[tokio::test]
    async fn delete_apaga_a_entrada() {
        let dir = tempfile::tempdir().expect("tempdir");
        let config = config_em(dir.path());
        let id = {
            let store = MemoryStore::open(&config.memory_db_path()).expect("cria o banco");
            store
                .remember(entrada("lembrar disto"))
                .await
                .expect("insere")
        };

        let code = run_delete(&config, id.clone(), true).await.expect("delete");
        assert_eq!(code, EXIT_OK);

        let store = MemoryStore::open(&config.memory_db_path()).expect("reabre");
        assert_eq!(store.integrity_report().expect("report").entries_total, 0);

        // Apagar de novo o mesmo id ja nao acha nada.
        assert_eq!(
            run_delete(&config, id, true).await.expect("delete"),
            EXIT_USAGE
        );
    }

    /// Perguntar "quanto tem para fazer?" nao exige provider — recusar o
    /// dry-run seria negar a resposta justamente a quem ainda nao configurou.
    #[tokio::test]
    async fn reindex_dry_run_dispensa_provider() {
        let dir = tempfile::tempdir().expect("tempdir");
        let config = config_em(dir.path());
        {
            let store = MemoryStore::open(&config.memory_db_path()).expect("cria o banco");
            store.remember(entrada("sem vetor")).await.expect("insere");
        }

        let code = run_reindex(&config, None, 32, true, false)
            .await
            .expect("reindex");

        assert_eq!(code, EXIT_OK);
        // Dry-run nao grava: a fila continua de pe.
        let store = MemoryStore::open(&config.memory_db_path()).expect("reabre");
        assert_eq!(
            store
                .integrity_report()
                .expect("report")
                .entries_without_embedding,
            1
        );
    }

    #[tokio::test]
    async fn reindex_sem_provider_e_indisponivel() {
        let dir = tempfile::tempdir().expect("tempdir");
        let config = config_em(dir.path());
        MemoryStore::open(&config.memory_db_path()).expect("cria o banco");

        let code = run_reindex(&config, None, 32, false, false)
            .await
            .expect("reindex");

        assert_eq!(code, EXIT_UNAVAILABLE);
    }

    #[tokio::test]
    async fn batch_size_zero_e_erro_de_uso() {
        let dir = tempfile::tempdir().expect("tempdir");
        let code = run_reindex(&config_em(dir.path()), None, 0, true, false)
            .await
            .expect("reindex");
        assert_eq!(code, EXIT_USAGE);
    }

    /// Sem TTY e sem `--yes` nada e apagado — um script que esbarre no
    /// comando nao pode levar o banco junto.
    #[tokio::test]
    async fn compact_sem_confirmacao_nao_apaga() {
        let dir = tempfile::tempdir().expect("tempdir");
        let config = config_em(dir.path());
        {
            let store = MemoryStore::open(&config.memory_db_path()).expect("cria o banco");
            store.remember(entrada("fica")).await.expect("insere");
        }

        // `cargo test` roda sem TTY em stdin, entao este e o caminho real.
        let code = run_compact(&config, 0, false).await.expect("compact");

        assert_eq!(code, EXIT_USAGE);
        let store = MemoryStore::open(&config.memory_db_path()).expect("reabre");
        assert_eq!(store.integrity_report().expect("report").entries_total, 1);
    }

    #[test]
    fn cutoff_anda_para_tras() {
        let agora = Utc::now();
        let corte = cutoff_from_days(agora, 30).expect("30 dias cabem no calendario");
        assert_eq!((agora - corte).num_days(), 30);
    }
}
