//! Latencia de `GET /v1/me/export` num cenario reproduzivel (#1801).
//!
//! Mede, nao reimplementa: a requisicao passa pelo router de producao
//! (`build_router_for_test`), com JWT verificado, extractor `Principal`, RLS
//! do role `garraia_app` e o handler `export_me` inteiro, contra um Postgres
//! real (pgvector/pg16 em testcontainer). O corpo e consumido ate o ultimo
//! byte dentro da janela cronometrada; o parse e a validacao de conteudo e de
//! isolamento ficam FORA dela.
//!
//! ## O que NAO esta na medida
//!
//! Rede, TLS e o listener TCP: o router e chamado em processo, por
//! `tower::ServiceExt::oneshot`, como no resto da suite. O numero e o custo
//! do servidor (auth + banco + serializacao), nao o tempo de download.
//!
//! ## Carga (tudo sintetico, nenhum dado real)
//!
//! Titular (Alice), em DOIS grupos — o handler reaponta a GUC de tenant por
//! grupo, e um grupo so nao exercitaria esse laco:
//!
//! - `MESSAGES` mensagens proprias (padrao 10 000): 75 % no grupo dela, em
//!   tres chats, e 25 % num chat do grupo da Carol;
//! - 200 mencoes recebidas, 100 arquivos (metadado), 100 memorias pessoais e
//!   100 tarefas.
//!
//! Terceiros, que NAO podem aparecer no export:
//!
//! - Carol, no mesmo chat que a Alice: 1 000 mensagens, das quais 200
//!   mencionam a Alice (essas saem so como trecho, em `mentions_received`);
//! - Bob, em outro tenant: 1 000 mensagens.
//!
//! ## Teto do endpoint
//!
//! `export_me` corta cada secao em 5 000 linhas e declara o corte em
//! `truncated_sections`. Com 10 000 mensagens o export traz 5 000 e diz que
//! cortou: a latencia medida e a de um export TRUNCADO, e o teste verifica
//! exatamente isso em vez de fingir que os 10 000 sairam. Para medir o export
//! completo, rode com `GARRAIA_EXPORT_PERF_MESSAGES=5000`.
//!
//! ## Como rodar
//!
//! `benches/data-export/run.sh` (registra maquina, versoes e SHA), ou direto:
//!
//! ```text
//! cargo test -p garraia-gateway --features test-helpers \
//!     --test rest_v1_me_export_perf -- --nocapture
//! ```
//!
//! Knobs (env): `GARRAIA_EXPORT_PERF_MESSAGES` (padrao 10000),
//! `GARRAIA_EXPORT_PERF_RUNS` (repeticoes quentes, padrao 30) e
//! `GARRAIA_EXPORT_PERF_OUT` (caminho de um JSON com as amostras cruas).
//!
//! ## O que o teste afirma sobre latencia
//!
//! So a referencia historica do ROADMAP ("export em < 30s"): toda amostra,
//! fria ou quente, tem de caber nela. E um teto frouxo de proposito — pega
//! regressao de ordem de grandeza sem virar teste instavel em runner lento.
//! Os percentis sao REPORTADOS, nao afirmados.

mod common;

use std::net::SocketAddr;
use std::time::{Duration, Instant};

use axum::extract::ConnectInfo;
use axum::http::{HeaderName, HeaderValue, Request, StatusCode};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use tower::ServiceExt;
use uuid::Uuid;

use common::Harness;
use common::fixtures::seed_user_with_group;

/// Referencia historica (ROADMAP: "Export LGPD ... em < 30s").
const REFERENCIA: Duration = Duration::from_secs(30);

/// Teto de linhas por secao em `export_me` (`EXPORT_MAX_MESSAGES`). Repetido
/// aqui de proposito: se o handler mudar o teto, este teste tem de falhar e
/// a medicao ser refeita, nao acompanhar em silencio.
const TETO_DE_MENSAGENS: usize = 5_000;

const MENSAGENS_PADRAO: usize = 10_000;
const REPETICOES_PADRAO: usize = 30;

const MENSAGENS_DA_CAROL: i32 = 1_000;
const MENCOES_A_ALICE: i32 = 200;
const MENSAGENS_DO_BOB: i32 = 1_000;
const ARQUIVOS: i32 = 100;
const MEMORIAS: i32 = 100;
const TAREFAS: i32 = 100;

/// Marcas que identificam de quem e cada linha sintetica.
const MARCA_ALICE: &str = "PERF-ALICE-";
const MARCA_CAROL_PRIVADA: &str = "PERF-CAROL-PRIVADA-";
const MARCA_CAROL_MENCAO: &str = "PERF-CAROL-MENCAO-";
const MARCA_BOB: &str = "PERF-BOB-";

fn env_usize(nome: &str, padrao: usize) -> usize {
    match std::env::var(nome) {
        Ok(v) => v
            .trim()
            .parse()
            .unwrap_or_else(|_| panic!("{nome} deve ser um inteiro positivo, veio {v:?}")),
        Err(_) => padrao,
    }
}

/// `GET /v1/me/export` autenticado. Cada requisicao sai de um IP de par
/// diferente: o rate limit da cadeia de producao e por IP, e o objeto da
/// medida e o handler, nao o balde do governor.
fn requisicao(token: &str, n: usize) -> Request<axum::body::Body> {
    let mut req = Request::builder()
        .method("GET")
        .uri("/v1/me/export")
        .body(axum::body::Body::empty())
        .expect("request builder");
    let ip = std::net::Ipv4Addr::new(10, 77, (n / 250) as u8, (n % 250) as u8 + 1);
    req.extensions_mut()
        .insert(ConnectInfo(SocketAddr::from((ip, 1))));
    req.headers_mut().insert(
        HeaderName::from_static("authorization"),
        HeaderValue::from_str(&format!("Bearer {token}")).expect("valid bearer header"),
    );
    req
}

/// Uma exportacao completa: status, corpo inteiro e o tempo ate o ultimo byte.
async fn exportar(h: &Harness, token: &str, n: usize) -> (Duration, bytes::Bytes) {
    let req = requisicao(token, n);
    let inicio = Instant::now();
    let resp = h.router.clone().oneshot(req).await.expect("oneshot export");
    let status = resp.status();
    let corpo = resp
        .into_body()
        .collect()
        .await
        .expect("collect export body")
        .to_bytes();
    let decorrido = inicio.elapsed();
    assert_eq!(
        status,
        StatusCode::OK,
        "export #{n} deve responder 200; corpo: {}",
        String::from_utf8_lossy(&corpo[..corpo.len().min(300)])
    );
    (decorrido, corpo)
}

async fn seed_chat(h: &Harness, group_id: Uuid, creator: Uuid, name: &str) -> Uuid {
    let chat_id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO chats (id, group_id, type, name, created_by) \
         VALUES ($1, $2, 'channel', $3, $4)",
    )
    .bind(chat_id)
    .bind(group_id)
    .bind(name)
    .bind(creator)
    .execute(&h.admin_pool)
    .await
    .expect("seed chat");
    chat_id
}

/// `quantas` mensagens de `sender` num chat, numa unica instrucao. O corpo
/// varia de ~30 a ~270 caracteres e os instantes sao distintos (um segundo
/// de passo, a partir de `offset`), para a ordenacao do export ser estavel.
async fn seed_mensagens(
    h: &Harness,
    chat_id: Uuid,
    group_id: Uuid,
    sender: Uuid,
    marca: &str,
    offset: i32,
    quantas: i32,
) {
    let n = sqlx::query(
        "INSERT INTO messages (chat_id, group_id, sender_user_id, sender_label, body, created_at) \
         SELECT $1, $2, $3, 'Perf Seed', \
                $4 || i::text || ' ' || repeat('lorem ipsum ', 1 + (i % 20)), \
                now() - make_interval(secs => i) \
         FROM generate_series($5::int + 1, $5::int + $6::int) AS s(i)",
    )
    .bind(chat_id)
    .bind(group_id)
    .bind(sender)
    .bind(marca)
    .bind(offset)
    .bind(quantas)
    .execute(&h.admin_pool)
    .await
    .expect("seed messages")
    .rows_affected();
    assert_eq!(n, quantas as u64, "seed de mensagens ({marca}) incompleto");
}

/// Percentil por posto mais proximo (nearest-rank) sobre amostras ORDENADAS.
fn percentil(ordenadas: &[Duration], p: f64) -> Duration {
    assert!(!ordenadas.is_empty(), "percentil de amostra vazia");
    let posto = ((p / 100.0) * ordenadas.len() as f64).ceil() as usize;
    ordenadas[posto.clamp(1, ordenadas.len()) - 1]
}

fn ms(d: Duration) -> f64 {
    (d.as_secs_f64() * 1_000_000.0).round() / 1_000.0
}

#[tokio::test]
async fn export_de_10_mil_mensagens_cabe_na_referencia_e_respeita_o_isolamento() {
    let total = env_usize("GARRAIA_EXPORT_PERF_MESSAGES", MENSAGENS_PADRAO);
    let repeticoes = env_usize("GARRAIA_EXPORT_PERF_RUNS", REPETICOES_PADRAO);
    assert!(
        (4..=100_000).contains(&total),
        "GARRAIA_EXPORT_PERF_MESSAGES fora da faixa 4..=100000: {total}"
    );
    assert!(
        (1..=10_000).contains(&repeticoes),
        "GARRAIA_EXPORT_PERF_RUNS fora da faixa 1..=10000: {repeticoes}"
    );

    let h = Harness::get().await;

    // ── Preparacao da carga ────────────────────────────────────────────
    let preparo = Instant::now();

    let (alice, alice_group, alice_token) = seed_user_with_group(&h, "alice@export-perf.test")
        .await
        .expect("seed alice");
    let (carol, carol_group, _) = seed_user_with_group(&h, "carol@export-perf.test")
        .await
        .expect("seed carol");
    let (bob, bob_group, _) = seed_user_with_group(&h, "bob@export-perf.test")
        .await
        .expect("seed bob");

    // Alice tambem e membro do grupo da Carol: segundo grupo do laco.
    sqlx::query(
        "INSERT INTO group_members (group_id, user_id, role, status) \
         VALUES ($1, $2, 'member', 'active')",
    )
    .bind(carol_group)
    .bind(alice)
    .execute(&h.admin_pool)
    .await
    .expect("alice entra no grupo da carol");

    // Mensagens da Alice: quatro fatias, tres chats no grupo dela e um no da
    // Carol. A ultima fatia absorve o resto da divisao.
    let fatia = (total / 4) as i32;
    let ultima = total as i32 - 3 * fatia;
    let chat_compartilhado = seed_chat(&h, carol_group, carol, "perf-compartilhado").await;
    for (i, quantas) in [fatia, fatia, fatia].into_iter().enumerate() {
        let chat = seed_chat(&h, alice_group, alice, &format!("perf-alice-{i}")).await;
        seed_mensagens(
            &h,
            chat,
            alice_group,
            alice,
            MARCA_ALICE,
            i as i32 * fatia,
            quantas,
        )
        .await;
    }
    seed_mensagens(
        &h,
        chat_compartilhado,
        carol_group,
        alice,
        MARCA_ALICE,
        3 * fatia,
        ultima,
    )
    .await;

    // Carol, no MESMO chat: mensagens que nao sao da Alice. As primeiras
    // `MENCOES_A_ALICE` a mencionam.
    seed_mensagens(
        &h,
        chat_compartilhado,
        carol_group,
        carol,
        MARCA_CAROL_MENCAO,
        0,
        MENCOES_A_ALICE,
    )
    .await;
    seed_mensagens(
        &h,
        chat_compartilhado,
        carol_group,
        carol,
        MARCA_CAROL_PRIVADA,
        MENCOES_A_ALICE,
        MENSAGENS_DA_CAROL - MENCOES_A_ALICE,
    )
    .await;
    let mencoes = sqlx::query(
        "INSERT INTO message_mentions (message_id, mentioned_user_id, group_id) \
         SELECT id, $1, group_id FROM messages \
         WHERE sender_user_id = $2 AND body LIKE $3",
    )
    .bind(alice)
    .bind(carol)
    .bind(format!("{MARCA_CAROL_MENCAO}%"))
    .execute(&h.admin_pool)
    .await
    .expect("seed mentions")
    .rows_affected();
    assert_eq!(mencoes, MENCOES_A_ALICE as u64, "seed de mencoes incompleto");

    // Bob: outro tenant.
    let bob_chat = seed_chat(&h, bob_group, bob, "perf-bob").await;
    seed_mensagens(
        &h,
        bob_chat,
        bob_group,
        bob,
        MARCA_BOB,
        0,
        MENSAGENS_DO_BOB,
    )
    .await;

    // Arquivos (so metadado), memorias pessoais e tarefas da Alice.
    sqlx::query(
        "INSERT INTO files (group_id, name, size_bytes, mime_type, created_by, created_by_label) \
         SELECT $1, 'perf-' || i::text || '.txt', 11, 'text/plain', $2, 'Perf Seed' \
         FROM generate_series(1, $3::int) AS s(i)",
    )
    .bind(alice_group)
    .bind(alice)
    .bind(ARQUIVOS)
    .execute(&h.admin_pool)
    .await
    .expect("seed files");
    sqlx::query(
        "INSERT INTO memory_items \
             (scope_type, scope_id, group_id, created_by, created_by_label, kind, content) \
         SELECT 'user', $1, NULL, $1, 'Perf Seed', 'note', 'PERF-MEMORIA-' || i::text \
         FROM generate_series(1, $2::int) AS s(i)",
    )
    .bind(alice)
    .bind(MEMORIAS)
    .execute(&h.admin_pool)
    .await
    .expect("seed memory");
    let lista = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO task_lists (id, group_id, name, type, created_by, created_by_label) \
         VALUES ($1, $2, 'Perf list', 'list', $3, 'Perf Seed')",
    )
    .bind(lista)
    .bind(alice_group)
    .bind(alice)
    .execute(&h.admin_pool)
    .await
    .expect("seed task list");
    sqlx::query(
        "INSERT INTO tasks (list_id, group_id, title, created_by, created_by_label) \
         SELECT $1, $2, 'PERF-TAREFA-' || i::text, $3, 'Perf Seed' \
         FROM generate_series(1, $4::int) AS s(i)",
    )
    .bind(lista)
    .bind(alice_group)
    .bind(alice)
    .bind(TAREFAS)
    .execute(&h.admin_pool)
    .await
    .expect("seed tasks");

    // Estatisticas em dia, como um banco vivo com autovacuum: sem isto o
    // planner trabalharia com a contagem de ANTES da carga em massa.
    sqlx::query("ANALYZE")
        .execute(&h.admin_pool)
        .await
        .expect("analyze");

    let preparo = preparo.elapsed();

    // ── Medicao ────────────────────────────────────────────────────────
    //
    // Fria: a primeira requisicao depois da carga (conexao do pool de
    // handlers e planos ainda nao aquecidos; as paginas recem-escritas ja
    // estao no cache do Postgres — nao e um "cold start" de disco).
    let (fria, corpo_frio) = exportar(&h, &alice_token, 0).await;

    let mut quentes: Vec<Duration> = Vec::with_capacity(repeticoes);
    let mut ultimo_corpo = corpo_frio.clone();
    for n in 1..=repeticoes {
        let (d, corpo) = exportar(&h, &alice_token, n).await;
        quentes.push(d);
        ultimo_corpo = corpo;
    }

    // ── Validacao de conteudo e isolamento (fora da janela medida) ─────
    let esperadas = total.min(TETO_DE_MENSAGENS);
    let truncado = total > TETO_DE_MENSAGENS;
    for (rotulo, corpo) in [("frio", &corpo_frio), ("ultimo quente", &ultimo_corpo)] {
        let cru = std::str::from_utf8(corpo).expect("export e UTF-8");
        let v: Value = serde_json::from_str(cru).expect("export e JSON");

        assert_eq!(v["schema_version"], "2", "[{rotulo}] schema do export");
        assert_eq!(
            v["profile"]["user_id"],
            alice.to_string(),
            "[{rotulo}] o export e do titular autenticado"
        );

        let mensagens = v["messages"].as_array().expect("messages array");
        assert_eq!(
            mensagens.len(),
            esperadas,
            "[{rotulo}] {total} mensagens semeadas, teto {TETO_DE_MENSAGENS}"
        );
        let mut ids = std::collections::HashSet::new();
        for m in mensagens {
            let corpo_msg = m["body"].as_str().expect("message body");
            assert!(
                corpo_msg.starts_with(MARCA_ALICE),
                "[{rotulo}] mensagem que nao e da titular no export: {corpo_msg:.40}"
            );
            let grupo = m["group_id"].as_str().expect("message group_id");
            assert!(
                grupo == alice_group.to_string() || grupo == carol_group.to_string(),
                "[{rotulo}] mensagem de grupo fora das memberships da titular: {grupo}"
            );
            assert!(
                ids.insert(m["message_id"].as_str().expect("message_id")),
                "[{rotulo}] mensagem repetida no export"
            );
        }

        let truncadas: Vec<&str> = v["truncated_sections"]
            .as_array()
            .expect("truncated_sections array")
            .iter()
            .filter_map(Value::as_str)
            .collect();
        assert_eq!(
            truncadas.contains(&"messages"),
            truncado,
            "[{rotulo}] corte de mensagens tem de ser declarado se e so se houve; veio {truncadas:?}"
        );
        assert_eq!(
            truncadas.iter().filter(|s| **s != "messages").count(),
            0,
            "[{rotulo}] nenhuma outra secao chega ao teto nesta carga; veio {truncadas:?}"
        );

        let mencoes = v["mentions_received"].as_array().expect("mentions array");
        assert_eq!(
            mencoes.len(),
            MENCOES_A_ALICE as usize,
            "[{rotulo}] mencoes recebidas"
        );
        assert!(
            mencoes.iter().all(|m| m["body_excerpt"]
                .as_str()
                .is_some_and(|t| t.starts_with(MARCA_CAROL_MENCAO))),
            "[{rotulo}] toda mencao e o trecho de uma mensagem que mencionou a titular"
        );
        assert_eq!(
            v["files"].as_array().expect("files array").len(),
            ARQUIVOS as usize,
            "[{rotulo}] arquivos"
        );
        assert_eq!(
            v["memory_items"].as_array().expect("memory array").len(),
            MEMORIAS as usize,
            "[{rotulo}] memorias"
        );
        assert_eq!(
            v["tasks"].as_array().expect("tasks array").len(),
            TAREFAS as usize,
            "[{rotulo}] tarefas"
        );

        // Isolamento: nada do outro tenant, e do chat compartilhado so o
        // trecho das mensagens que mencionaram a titular.
        assert!(
            !cru.contains(MARCA_BOB),
            "[{rotulo}] conteudo de outro tenant no export"
        );
        assert!(
            !cru.contains(MARCA_CAROL_PRIVADA),
            "[{rotulo}] mensagem de terceiro (sem mencao) no export"
        );
        assert!(
            !cru.contains(&bob_group.to_string()) && !cru.contains(&bob.to_string()),
            "[{rotulo}] identificador de outro tenant no export"
        );
    }

    // ── Relatorio ──────────────────────────────────────────────────────
    let mut ordenadas = quentes.clone();
    ordenadas.sort();
    let soma: Duration = quentes.iter().sum();
    let media = soma / quentes.len() as u32;
    let p50 = percentil(&ordenadas, 50.0);
    let p95 = percentil(&ordenadas, 95.0);
    let minimo = ordenadas[0];
    let maximo = ordenadas[ordenadas.len() - 1];
    let perfil = if cfg!(debug_assertions) {
        "debug"
    } else {
        "release"
    };

    let relatorio = json!({
        "issue": 1801,
        "endpoint": "GET /v1/me/export",
        "transporte": "em processo (tower oneshot), sem rede nem TLS",
        "perfil_de_build": perfil,
        "carga": {
            "mensagens_da_titular": total,
            "grupos_da_titular": 2,
            "chats_da_titular": 4,
            "mencoes_recebidas": MENCOES_A_ALICE,
            "arquivos": ARQUIVOS,
            "memorias_pessoais": MEMORIAS,
            "tarefas": TAREFAS,
            "mensagens_de_terceiro_no_mesmo_chat": MENSAGENS_DA_CAROL,
            "mensagens_de_outro_tenant": MENSAGENS_DO_BOB,
        },
        "export": {
            "mensagens_exportadas": esperadas,
            "teto_de_mensagens": TETO_DE_MENSAGENS,
            "truncado": truncado,
            "bytes": ultimo_corpo.len(),
        },
        "preparo_da_carga_ms": ms(preparo),
        "fria_ms": ms(fria),
        "quentes": {
            "repeticoes": repeticoes,
            "min_ms": ms(minimo),
            "p50_ms": ms(p50),
            "p95_ms": ms(p95),
            "max_ms": ms(maximo),
            "media_ms": ms(media),
            "percentil": "nearest-rank",
            "amostras_ms": quentes.iter().map(|d| ms(*d)).collect::<Vec<_>>(),
        },
        "referencia_ms": ms(REFERENCIA),
    });

    println!("=== export-perf (#1801) ===");
    println!(
        "{}",
        serde_json::to_string_pretty(&relatorio).expect("serialize report")
    );
    println!("=== fim export-perf ===");

    if let Ok(destino) = std::env::var("GARRAIA_EXPORT_PERF_OUT") {
        let texto = serde_json::to_string_pretty(&relatorio).expect("serialize report");
        std::fs::write(&destino, texto + "\n")
            .unwrap_or_else(|e| panic!("nao consegui gravar {destino}: {e}"));
    }

    // ── A unica afirmacao de latencia: a referencia historica ──────────
    let pior = maximo.max(fria);
    assert!(
        pior < REFERENCIA,
        "export levou {} ms na pior amostra; a referencia historica e {} ms",
        ms(pior),
        ms(REFERENCIA)
    );
}
