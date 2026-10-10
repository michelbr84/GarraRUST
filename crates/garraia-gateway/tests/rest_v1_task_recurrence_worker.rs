//! Regressao do #1611 para o `tasks_recurrence_worker`.
//!
//! O mesmo defeito de `uploads_worker`: o tick adquiria o advisory lock
//! `hashtext('task_recurrence_sweep')` numa conexao do pool e tentava
//! liberta-lo noutra, via `tokio::spawn` no `Drop` de um guard. Advisory lock
//! no Postgres e **por sessao**, entao o unlock noutra conexao e um no-op, e
//! como o sqlx devolve a conexao ao pool sem resetar a sessao (sem
//! `DISCARD ALL`, e o `ping()` do `test_before_acquire` e so `write_sync`),
//! o lock sobrevivia a todo o tick seguinte.
//!
//! Este arquivo cobre so a invariante que o defeito quebrava: **depois que
//! `run_recurrence_tick` retorna, nenhuma sessao do servidor pode segurar o
//! lock.** Nao cobre a logica de recorrencia em si (que tem suite propria em
//! `garraia-workspace`) — a varredura com tabela vazia ja exercita o caminho
//! acquire -> sweep -> unlock, que e onde o lock vazava.
//!
//! O detector de holders e auto-testado: ele primeiro prova que DETECTA um
//! lock segurado, para que uma refactor futura nao possa deixar a query
//! devolvendo sempre 0 e deixar estas assercoes verdes sem provar nada.

mod common;

use common::Harness;
use garraia_gateway::tasks_recurrence_worker::run_recurrence_tick;

/// Chave de lock deste worker, para a auto-verificacao do detector.
const LOCK_KEY: &str = "task_recurrence_sweep";

/// True quando o daemon Docker esta acessivel. Sem Docker o teste faz skip
/// limpo em vez de panica no boot do container — mesmo padrao de
/// `rest_v1_uploads_delete_worker.rs`.
fn docker_available() -> bool {
    std::process::Command::new("docker")
        .arg("version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// Quantas sessoes, em TODO o servidor, seguram hoje o advisory lock deste
/// worker. Decodificamos o par `(classid, objid)` do `pg_locks` a partir do
/// mesmo `hashtext(...)` que o worker usa: o Postgres guarda a chave de 64
/// bits do `pg_try_advisory_lock(bigint)` dividida nos dois campos, e
/// `objsubid = 1` distingue o formato de inteiro do de par.
async fn advisory_lock_holders(h: &Harness, lock_key: &str) -> i64 {
    sqlx::query_scalar(
        "WITH k AS (SELECT hashtext($1)::bigint AS key) \
         SELECT count(*)::bigint \
         FROM pg_locks l, k \
         WHERE l.locktype = 'advisory' \
           AND l.objsubid = 1 \
           AND l.classid = ((k.key >> 32) & 4294967295)::oid \
           AND l.objid   = (k.key & 4294967295)::oid",
    )
    .bind(lock_key)
    .fetch_one(&h.admin_pool)
    .await
    .expect("query pg_locks for the recurrence advisory lock")
}

#[tokio::test]
async fn recurrence_worker_releases_advisory_lock() {
    if !docker_available() {
        eprintln!("docker not available; skipping recurrence_worker_releases_advisory_lock");
        return;
    }

    let h = Harness::get().await;

    // ─── Self-test do detector ─────────────────────────────────────
    // Uma assercao que nunca falha nao prova nada. Prova que o detector
    // DETECTA um lock segurado antes de confiar nele nas assercoes abaixo.
    {
        let mut probe = h
            .admin_pool
            .acquire()
            .await
            .expect("acquire probe connection");
        let got: bool = sqlx::query_scalar("SELECT pg_try_advisory_lock(hashtext($1))")
            .bind(LOCK_KEY)
            .fetch_one(&mut *probe)
            .await
            .expect("probe try_advisory_lock");
        assert!(got, "probe acquired the recurrence advisory lock");

        assert_eq!(
            advisory_lock_holders(&h, LOCK_KEY).await,
            1,
            "detector sees exactly one holder while the probe session holds the lock"
        );

        let released: bool = sqlx::query_scalar("SELECT pg_advisory_unlock(hashtext($1))")
            .bind(LOCK_KEY)
            .fetch_one(&mut *probe)
            .await
            .expect("probe advisory_unlock");
        assert!(released, "probe released its own lock on the same session");
    }
    assert_eq!(
        advisory_lock_holders(&h, LOCK_KEY).await,
        0,
        "detector returns to zero once the probe session releases"
    );

    // ─── Ticks consecutivos nao acumulam lock (#1611) ──────────────
    // Com o defeito, CADA tick deixava uma sessao do pool segurando o lock:
    // a contagem ia 1, depois 2, depois 3 — e nunca voltava a zero, porque o
    // pool mantem as sessoes vivas. E exatamente esse acumulo que parava a
    // varredura de recorrencia em silencio.
    for tick in 1..=3 {
        run_recurrence_tick(h.app_pool.clone(), 16)
            .await
            .unwrap_or_else(|e| panic!("tick {tick} failed: {e}"));

        assert_eq!(
            advisory_lock_holders(&h, LOCK_KEY).await,
            0,
            "no session still holds the recurrence advisory lock after tick {tick} (#1611)"
        );
    }

    // ─── E o lock segue liberado, ou seja: o proximo tick consegue pegar ──
    // Prova que o lock NAO ficou preso de um jeito que impediria replicas
    // futuras de varrer — o modo de falha real do defeito.
    let got_again: bool = {
        let mut probe = h
            .admin_pool
            .acquire()
            .await
            .expect("acquire post-tick probe");
        let got: bool = sqlx::query_scalar("SELECT pg_try_advisory_lock(hashtext($1))")
            .bind(LOCK_KEY)
            .fetch_one(&mut *probe)
            .await
            .expect("post-tick try_advisory_lock");
        let _: bool = sqlx::query_scalar("SELECT pg_advisory_unlock(hashtext($1))")
            .bind(LOCK_KEY)
            .fetch_one(&mut *probe)
            .await
            .expect("post-tick advisory_unlock");
        got
    };
    assert!(
        got_again,
        "the recurrence lock is acquirable again after the ticks — no replica is locked out"
    );
}
