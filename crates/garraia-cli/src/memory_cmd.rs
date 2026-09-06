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
        "pinned_at": entry.pinned_at.map(|t| t.to_rfc3339()),
        "ttl_expires_at": entry.ttl_expires_at.map(|t| t.to_rfc3339()),
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
    // Marcas de retencao: fixada nunca e apagada, vencida ja saiu do recall.
    let mut marcas = String::new();
    if entry.pinned_at.is_some() {
        marcas.push_str("  [fixada]");
    }
    if entry
        .ttl_expires_at
        .is_some_and(|prazo| prazo <= chrono::Utc::now())
    {
        marcas.push_str("  [vencida]");
    } else if let Some(prazo) = entry.ttl_expires_at {
        marcas.push_str(&format!("  [vence {}]", prazo.format("%Y-%m-%d")));
    }
    format!(
        "{}  {}  [{}]  ({}){}{marcas}\n    {}",
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
            "entries_pinned": report.entries_pinned,
            "entries_expired": report.entries_expired,
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
    println!("Fixadas:            {}", report.entries_pinned);
    println!("Vencidas:           {}", report.entries_expired);
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
    if report.entries_expired > 0 {
        println!(
            "\n{} entrada(s) ja passaram do prazo. Elas nao aparecem mais no recall;\n\
             `garra memory compact` (ou a retencao automatica) as apaga do banco.",
            report.entries_expired
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

/// Prefixo dos arquivos que este comando cria — e o **unico** que a retencao
/// de backups apaga.
const BACKUP_PREFIX: &str = "memory-";
const BACKUP_SUFFIX: &str = ".db";

/// Onde os backups moram por padrao.
fn default_backup_dir(config: &AppConfig) -> PathBuf {
    config.resolved_data_dir().join("backups")
}

/// Nome do arquivo de backup para um instante.
///
/// Timestamp em **UTC** e ordenavel como texto:
/// `memory-20260906T054500123Z.db`. Nome de arquivo e artefato de maquina,
/// entao segue a regra de timestamp tecnico do projeto, nao a de data
/// narrativa.
///
/// Milissegundo, e nao segundo: com precisao de segundo, `garra memory backup`
/// duas vezes seguidas (ou num laco de script) colidia, e o `VACUUM INTO`
/// recusa sobrescrever — com razao. O operador via um erro em vez de um
/// backup. Descoberto rodando o comando duas vezes de verdade.
pub(crate) fn backup_file_name(agora: DateTime<Utc>) -> String {
    format!(
        "{BACKUP_PREFIX}{}{BACKUP_SUFFIX}",
        agora.format("%Y%m%dT%H%M%S%3fZ")
    )
}

/// O arquivo e um backup criado por nos?
///
/// A retencao apaga arquivo, entao a pergunta precisa ser estreita: so o que
/// tem o nosso prefixo E o nosso sufixo. Qualquer outra coisa no diretorio —
/// um backup manual do operador, um `.db` copiado a mao — fica.
pub(crate) fn is_our_backup(nome: &str) -> bool {
    nome.starts_with(BACKUP_PREFIX) && nome.ends_with(BACKUP_SUFFIX)
}

/// `garra memory backup` — retrato consistente do banco, com retencao (#955).
pub async fn run_backup(
    config: &AppConfig,
    dir: Option<PathBuf>,
    keep_days: Option<i64>,
    json: bool,
) -> Result<i32> {
    let store = match open_store(config) {
        Opened::Store(store, _) => *store,
        Opened::Absent(path) => {
            report_no_store(&path);
            return Ok(EXIT_OK);
        }
        Opened::Failed => return Ok(EXIT_IOERR),
    };

    let destino_dir = dir.unwrap_or_else(|| default_backup_dir(config));
    std::fs::create_dir_all(&destino_dir)
        .with_context(|| format!("failed to create backup dir {}", destino_dir.display()))?;

    let agora = Utc::now();
    let destino = destino_dir.join(backup_file_name(agora));

    let bytes = match store.backup_to(&destino) {
        Ok(bytes) => bytes,
        Err(e) => {
            // Erro de backup e operacional (disco cheio, permissao, nome ja
            // existente), nao defeito de programa: sai com mensagem e codigo,
            // nao com backtrace.
            eprintln!("error: nao foi possivel escrever o backup: {e}");
            return Ok(EXIT_IOERR);
        }
    };

    // A retencao roda DEPOIS do backup novo existir. Se ela rodasse antes e o
    // backup falhasse, o operador ficaria com menos copias do que tinha.
    let apagados = match keep_days {
        Some(dias) => prune_backups(&destino_dir, agora, dias, &destino)?,
        None => Vec::new(),
    };

    if json {
        let payload = serde_json::json!({
            "path": destino.display().to_string(),
            "bytes": bytes,
            "pruned": apagados.iter().map(|p| p.display().to_string()).collect::<Vec<_>>(),
        });
        println!("{}", serde_json::to_string_pretty(&payload)?);
        return Ok(EXIT_OK);
    }

    println!("Backup: {} ({})", destino.display(), formata_bytes(bytes));
    if !apagados.is_empty() {
        println!(
            "Apagados {} backup(s) mais velhos que {} dia(s).",
            apagados.len(),
            keep_days.unwrap_or_default()
        );
    }
    println!();
    println!("Para restaurar:");
    println!("  1. pare o gateway  (`garra stop`)");
    println!(
        "  2. troque o banco  (`cp {} {}`)",
        destino.display(),
        config.memory_db_path().display()
    );
    println!(
        "  3. apague o WAL    (`rm -f {}-wal {}-shm`)",
        config.memory_db_path().display(),
        config.memory_db_path().display()
    );
    println!("  4. suba de novo    (`garra start`)");
    println!("     O passo 3 e o que costuma ser esquecido: um `-wal` antigo ao lado");
    println!("     de um banco restaurado reintroduz o que voce acabou de descartar.");

    Ok(EXIT_OK)
}

/// Apaga backups nossos mais velhos que `keep_days`, pela data no **nome**.
///
/// Pelo nome, e nao pelo mtime: copiar o diretorio de backups para outra
/// maquina renova todo mtime e apagaria tudo na primeira execucao seguinte.
/// O nome carrega o instante real da copia.
///
/// Nunca apaga o backup recem-criado, nem arquivo que nao case com o nosso
/// padrao.
fn prune_backups(
    dir: &Path,
    agora: DateTime<Utc>,
    keep_days: i64,
    recem_criado: &Path,
) -> Result<Vec<PathBuf>> {
    if keep_days < 0 {
        return Ok(Vec::new());
    }
    let Some(corte) = cutoff_from_days(agora, keep_days) else {
        return Ok(Vec::new());
    };

    let mut apagados = Vec::new();
    let entradas = std::fs::read_dir(dir)
        .with_context(|| format!("failed to read backup dir {}", dir.display()))?;

    for entrada in entradas {
        let entrada = entrada.context("failed to read backup dir entry")?;
        let caminho = entrada.path();
        if caminho == recem_criado {
            continue;
        }
        let Some(nome) = caminho.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        if !is_our_backup(nome) {
            continue;
        }
        let Some(quando) = backup_timestamp(nome) else {
            // Nome com o nosso prefixo mas timestamp ilegivel: nao apaga.
            // Na duvida sobre a idade, o lado seguro e manter.
            continue;
        };
        if quando < corte {
            std::fs::remove_file(&caminho)
                .with_context(|| format!("failed to remove old backup {}", caminho.display()))?;
            apagados.push(caminho);
        }
    }

    Ok(apagados)
}

/// Le o instante de volta do nome do arquivo. `None` quando nao parseia.
pub(crate) fn backup_timestamp(nome: &str) -> Option<DateTime<Utc>> {
    let miolo = nome
        .strip_prefix(BACKUP_PREFIX)?
        .strip_suffix(BACKUP_SUFFIX)?;
    chrono::NaiveDateTime::parse_from_str(miolo, "%Y%m%dT%H%M%S%3fZ")
        .ok()
        // Nomes gravados antes da mudanca para milissegundo continuam legiveis:
        // a retencao precisa saber a idade deles para poder apaga-los.
        .or_else(|| chrono::NaiveDateTime::parse_from_str(miolo, "%Y%m%dT%H%M%SZ").ok())
        .map(|naive| naive.and_utc())
}

fn formata_bytes(bytes: u64) -> String {
    const MIB: f64 = 1024.0 * 1024.0;
    const KIB: f64 = 1024.0;
    let b = bytes as f64;
    if b >= MIB {
        format!("{:.1} MiB", b / MIB)
    } else if b >= KIB {
        format!("{:.1} KiB", b / KIB)
    } else {
        format!("{bytes} B")
    }
}

/// `garra memory pin` — protege uma entrada da compactacao (#959).
///
/// Nao ha confirmacao: fixar e soltar sao reversiveis e nao apagam nada. O
/// que a politica de retencao apaga sem perguntar e o que **nao** esta fixado.
pub async fn run_pin(config: &AppConfig, id: String, unpin: bool) -> Result<i32> {
    let store = match open_store(config) {
        Opened::Store(store, _) => *store,
        Opened::Absent(path) => {
            report_no_store(&path);
            return Ok(EXIT_OK);
        }
        Opened::Failed => return Ok(EXIT_IOERR),
    };

    if store.set_pinned(&id, !unpin).context("failed to set pin")? {
        if unpin {
            println!("Entrada {id} solta: volta a valer a politica de retencao.");
        } else {
            println!("Entrada {id} fixada: a compactacao nunca vai apaga-la.");
        }
        Ok(EXIT_OK)
    } else {
        eprintln!("error: nenhuma entrada com o id {id}");
        Ok(EXIT_USAGE)
    }
}

/// `garra memory ttl` — define ou limpa o prazo de validade (#959).
///
/// Prazo vencido nao apaga na hora: a entrada some do recall imediatamente e
/// a proxima compactacao a remove. E o que torna a operacao reversivel
/// enquanto a compactacao nao roda.
pub async fn run_ttl(
    config: &AppConfig,
    id: String,
    days: Option<i64>,
    clear: bool,
) -> Result<i32> {
    if clear && days.is_some() {
        eprintln!("error: --clear e <DAYS> sao mutuamente exclusivos");
        return Ok(EXIT_USAGE);
    }

    let prazo = if clear {
        None
    } else {
        let Some(days) = days else {
            eprintln!("error: informe o numero de dias, ou --clear para remover o prazo");
            return Ok(EXIT_USAGE);
        };
        // Mesma guarda do `compact`: `Duration::days` entra em panico com
        // numero absurdo, e um prazo negativo tem significado (vencer agora),
        // entao so o extremo e recusado.
        let Some(instante) = Utc::now().checked_add_signed(
            Duration::try_days(days)
                .ok_or_else(|| anyhow::anyhow!("numero de dias fora do calendario"))?,
        ) else {
            eprintln!("error: {days} dias nao cabem no calendario");
            return Ok(EXIT_USAGE);
        };
        Some(instante)
    };

    let store = match open_store(config) {
        Opened::Store(store, _) => *store,
        Opened::Absent(path) => {
            report_no_store(&path);
            return Ok(EXIT_OK);
        }
        Opened::Failed => return Ok(EXIT_IOERR),
    };

    if store.set_ttl(&id, prazo).context("failed to set ttl")? {
        match prazo {
            Some(t) if t <= Utc::now() => println!(
                "Entrada {id} vence em {} — ja no passado, entao ela sai do recall agora e \
                 a proxima compactacao a apaga.",
                t.format("%Y-%m-%d %H:%M:%SZ")
            ),
            Some(t) => println!("Entrada {id} vence em {}.", t.format("%Y-%m-%d %H:%M:%SZ")),
            None => println!("Prazo da entrada {id} removido: ela nao vence mais."),
        }
        Ok(EXIT_OK)
    } else {
        eprintln!("error: nenhuma entrada com o id {id}");
        Ok(EXIT_USAGE)
    }
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

    // ── #955: backup ─────────────────────────────────────────────────────

    #[test]
    fn nome_do_backup_e_ordenavel_e_utc() {
        let t = chrono::DateTime::parse_from_rfc3339("2026-09-06T05:45:00Z")
            .expect("data")
            .with_timezone(&Utc);
        assert_eq!(backup_file_name(t), "memory-20260906T054500000Z.db");
        // Ida e volta: a retencao le a idade de volta do nome.
        assert_eq!(backup_timestamp(&backup_file_name(t)), Some(t));
    }

    /// A retencao apaga arquivo, entao ela so pode reconhecer o que criou.
    #[test]
    fn so_reconhece_como_backup_o_que_tem_nosso_padrao() {
        assert!(is_our_backup("memory-20260906T054500000Z.db"));
        assert!(!is_our_backup("memory.db"));
        assert!(!is_our_backup("backup-do-operador.db"));
        assert!(!is_our_backup("memory-20260906T054500000Z.db.tmp"));
        assert!(!is_our_backup("notas.txt"));
    }

    #[test]
    fn timestamp_ilegivel_no_nome_vira_none() {
        assert_eq!(backup_timestamp("memory-nao-e-data.db"), None);
        assert_eq!(backup_timestamp("outro-20260906T054500000Z.db"), None);
    }

    /// Nome do formato antigo (segundo, sem milissegundo) continua legivel —
    /// a retencao precisa saber a idade dele para poder apaga-lo.
    #[test]
    fn ainda_le_o_nome_do_formato_antigo() {
        let t = chrono::DateTime::parse_from_rfc3339("2026-09-06T05:45:00Z")
            .expect("data")
            .with_timezone(&Utc);
        assert_eq!(backup_timestamp("memory-20260906T054500Z.db"), Some(t));
    }

    /// Dois backups seguidos nao podem colidir. Com precisao de segundo eles
    /// colidiam, e o segundo morria com erro em vez de escrever.
    #[tokio::test]
    async fn dois_backups_seguidos_nao_colidem() {
        let dir = tempfile::tempdir().expect("tempdir");
        let config = config_em(dir.path());
        MemoryStore::open(&config.memory_db_path()).expect("cria o banco");
        let copias = dir.path().join("copias");

        for _ in 0..3 {
            let code = run_backup(&config, Some(copias.clone()), None, false)
                .await
                .expect("backup");
            assert_eq!(code, EXIT_OK, "um dos backups seguidos falhou");
        }

        let nossos = std::fs::read_dir(&copias)
            .expect("le")
            .filter_map(|e| e.ok())
            .filter(|e| is_our_backup(&e.file_name().to_string_lossy()))
            .count();
        assert_eq!(nossos, 3, "os backups colidiram");
    }

    #[tokio::test]
    async fn backup_escreve_o_arquivo_e_ele_reabre() {
        let dir = tempfile::tempdir().expect("tempdir");
        let config = config_em(dir.path());
        {
            let store = MemoryStore::open(&config.memory_db_path()).expect("cria o banco");
            store.remember(entrada("preciosa")).await.expect("insere");
        }

        let destino = dir.path().join("copias");
        let code = run_backup(&config, Some(destino.clone()), None, false)
            .await
            .expect("backup");
        assert_eq!(code, EXIT_OK);

        let arquivos: Vec<_> = std::fs::read_dir(&destino)
            .expect("le dir")
            .filter_map(|e| e.ok())
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(arquivos.len(), 1, "{arquivos:?}");
        assert!(is_our_backup(&arquivos[0]), "{arquivos:?}");

        let copia = MemoryStore::open(&destino.join(&arquivos[0])).expect("a copia abre");
        assert_eq!(copia.integrity_report().expect("report").entries_total, 1);
    }

    /// A retencao nao pode encostar em arquivo que nao e nosso, nem no
    /// backup que acabou de ser criado.
    #[tokio::test]
    async fn retencao_apaga_so_backup_nosso_e_velho() {
        let dir = tempfile::tempdir().expect("tempdir");
        let config = config_em(dir.path());
        MemoryStore::open(&config.memory_db_path()).expect("cria o banco");

        let copias = dir.path().join("copias");
        std::fs::create_dir_all(&copias).expect("cria dir");

        // Velho e nosso: sai.
        let velho = copias.join("memory-20200101T000000Z.db");
        std::fs::write(&velho, b"antigo").expect("escreve");
        // Nosso mas recente: fica.
        let recente = copias.join(backup_file_name(Utc::now() - Duration::days(1)));
        std::fs::write(&recente, b"recente").expect("escreve");
        // Velho mas nao e nosso: fica.
        let alheio = copias.join("backup-do-operador.db");
        std::fs::write(&alheio, b"nao e meu").expect("escreve");
        // Nosso prefixo com data ilegivel: fica (na duvida sobre a idade,
        // manter).
        let ilegivel = copias.join("memory-sei-la-quando.db");
        std::fs::write(&ilegivel, b"?").expect("escreve");

        let code = run_backup(&config, Some(copias.clone()), Some(14), false)
            .await
            .expect("backup");
        assert_eq!(code, EXIT_OK);

        assert!(!velho.exists(), "o backup velho nosso devia ter saido");
        assert!(recente.exists(), "apagou um backup recente");
        assert!(alheio.exists(), "apagou arquivo que nao e nosso");
        assert!(ilegivel.exists(), "apagou arquivo de idade desconhecida");

        // E o recem-criado continua la.
        let nossos = std::fs::read_dir(&copias)
            .expect("le")
            .filter_map(|e| e.ok())
            .filter(|e| is_our_backup(&e.file_name().to_string_lossy()))
            .count();
        assert_eq!(nossos, 3, "recem-criado + recente + ilegivel");
    }

    /// Sem `--keep-days` nao se apaga nada — nunca por acidente.
    #[tokio::test]
    async fn sem_keep_days_nao_apaga_nada() {
        let dir = tempfile::tempdir().expect("tempdir");
        let config = config_em(dir.path());
        MemoryStore::open(&config.memory_db_path()).expect("cria o banco");

        let copias = dir.path().join("copias");
        std::fs::create_dir_all(&copias).expect("cria dir");
        let velho = copias.join("memory-20200101T000000Z.db");
        std::fs::write(&velho, b"antigo").expect("escreve");

        run_backup(&config, Some(copias), None, false)
            .await
            .expect("backup");
        assert!(velho.exists(), "apagou sem --keep-days");
    }

    #[tokio::test]
    async fn backup_sem_banco_nao_cria_nada() {
        let dir = tempfile::tempdir().expect("tempdir");
        let config = config_em(dir.path());
        let copias = dir.path().join("copias");

        let code = run_backup(&config, Some(copias.clone()), None, false)
            .await
            .expect("backup");

        assert_eq!(code, EXIT_OK);
        assert!(!copias.exists(), "criou diretorio de backup sem ter banco");
    }

    #[tokio::test]
    async fn pin_e_ttl_de_id_inexistente_sao_erro_de_uso() {
        let dir = tempfile::tempdir().expect("tempdir");
        let config = config_em(dir.path());
        MemoryStore::open(&config.memory_db_path()).expect("cria o banco");

        assert_eq!(
            run_pin(&config, "nao-existe".to_string(), false)
                .await
                .expect("pin"),
            EXIT_USAGE
        );
        assert_eq!(
            run_ttl(&config, "nao-existe".to_string(), Some(30), false)
                .await
                .expect("ttl"),
            EXIT_USAGE
        );
    }

    /// Fixar protege da compactacao: e o contrato inteiro do `pin`.
    ///
    /// A compactacao vem do store com um corte 5s no futuro, nao do
    /// `--older-than-days 0` da CLI: o `datetime()` do SQLite trunca no
    /// segundo, entao uma entrada criada no mesmo segundo do corte nao e
    /// "anterior" a ele e o teste passaria pelo motivo errado.
    #[tokio::test]
    async fn entrada_fixada_sobrevive_ao_compact() {
        let dir = tempfile::tempdir().expect("tempdir");
        let config = config_em(dir.path());
        let id = {
            let store = MemoryStore::open(&config.memory_db_path()).expect("cria o banco");
            store.remember(entrada("importante")).await.expect("insere")
        };

        assert_eq!(
            run_pin(&config, id.clone(), false).await.expect("pin"),
            EXIT_OK
        );
        {
            let store = MemoryStore::open(&config.memory_db_path()).expect("reabre");
            let report = store
                .compact(Utc::now() + Duration::seconds(5))
                .await
                .expect("compact");
            assert_eq!(report.deleted_entries, 0, "a fixada foi apagada");
            assert_eq!(store.integrity_report().expect("report").entries_pinned, 1);
        }

        // Solta, a mesma compactacao leva.
        assert_eq!(run_pin(&config, id, true).await.expect("unpin"), EXIT_OK);
        let store = MemoryStore::open(&config.memory_db_path()).expect("reabre");
        let report = store
            .compact(Utc::now() + Duration::seconds(5))
            .await
            .expect("compact");
        assert_eq!(report.deleted_entries, 1);
    }

    #[tokio::test]
    async fn ttl_com_clear_e_dias_juntos_e_erro_de_uso() {
        let dir = tempfile::tempdir().expect("tempdir");
        let code = run_ttl(&config_em(dir.path()), "x".to_string(), Some(1), true)
            .await
            .expect("ttl");
        assert_eq!(code, EXIT_USAGE);
    }

    #[tokio::test]
    async fn ttl_sem_dias_e_sem_clear_e_erro_de_uso() {
        let dir = tempfile::tempdir().expect("tempdir");
        let code = run_ttl(&config_em(dir.path()), "x".to_string(), None, false)
            .await
            .expect("ttl");
        assert_eq!(code, EXIT_USAGE);
    }

    /// Prazo no passado esconde do recall sem apagar — invisivel nao e
    /// apagada, e por isso o `--clear` reverte.
    #[tokio::test]
    async fn ttl_vencido_esconde_do_recall_e_clear_reverte() {
        let dir = tempfile::tempdir().expect("tempdir");
        let config = config_em(dir.path());
        let id = {
            let store = MemoryStore::open(&config.memory_db_path()).expect("cria o banco");
            store.remember(entrada("efemera")).await.expect("insere")
        };

        assert_eq!(
            run_ttl(&config, id.clone(), Some(-1), false)
                .await
                .expect("ttl"),
            EXIT_OK
        );
        {
            let store = MemoryStore::open(&config.memory_db_path()).expect("reabre");
            let report = store.integrity_report().expect("report");
            assert_eq!(report.entries_total, 1, "vencida ainda esta no banco");
            assert_eq!(report.entries_expired, 1);
            assert!(
                store.recent_entries(10).expect("lista").is_empty(),
                "vencida nao pode voltar no recall"
            );
        }

        assert_eq!(
            run_ttl(&config, id, None, true).await.expect("clear"),
            EXIT_OK
        );
        let store = MemoryStore::open(&config.memory_db_path()).expect("reabre");
        assert_eq!(store.integrity_report().expect("report").entries_expired, 0);
        assert_eq!(store.recent_entries(10).expect("lista").len(), 1);
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
