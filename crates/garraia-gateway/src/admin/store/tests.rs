//! Testes de `admin::store` (usuarios, sessoes, segredos, recuperacao e abertura no boot).

use super::*;

fn test_store() -> AdminStore {
    AdminStore::in_memory().expect("in-memory store should work")
}

#[test]
fn create_and_verify_user() {
    let store = test_store();
    let user = store
        .create_user("admin", "secret123", Role::Admin)
        .unwrap();
    assert_eq!(user.username, "admin");
    assert_eq!(user.role, Role::Admin);

    let verified = store.verify_password("admin", "secret123");
    assert!(verified.is_some());

    let bad = store.verify_password("admin", "wrong");
    assert!(bad.is_none());
}

#[test]
fn session_lifecycle() {
    let store = test_store();
    let user = store.create_user("op", "pass", Role::Operator).unwrap();

    let session = store
        .create_session(&user.id, Some("127.0.0.1"), None)
        .unwrap();
    assert!(!session.token.is_empty());
    assert!(!session.csrf_token.is_empty());

    let validated = store.validate_session(&session.token);
    assert!(validated.is_some());
    assert_eq!(validated.unwrap().user_id, user.id);

    store.delete_session(&session.token).unwrap();
    assert!(store.validate_session(&session.token).is_none());
}

#[test]
fn audit_log_round_trip() {
    let store = test_store();
    store
        .append_audit(
            Some("user1"),
            Some("admin"),
            "create",
            "secret",
            Some("OPENAI_KEY"),
            None,
            Some("127.0.0.1"),
            "success",
        )
        .unwrap();

    let entries = store.list_audit_log(10, 0, None, None);
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].action, "create");
    assert_eq!(entries[0].resource_type, "secret");
}

#[test]
fn try_user_count_matches_user_count() {
    let store = test_store();
    assert_eq!(store.try_user_count(), Ok(0));
    store.create_user("a", "p", Role::Viewer).unwrap();
    assert_eq!(store.try_user_count(), Ok(1));
}

#[test]
fn try_user_count_reports_read_failure_instead_of_zero() {
    let store = test_store();
    store.conn.execute_batch("DROP TABLE admin_users;").unwrap();
    assert_eq!(store.user_count(), 0, "display helper keeps fail-soft");
    assert!(store.try_user_count().is_err());
}

#[test]
fn open_for_boot_creates_missing_db() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("admin.db");
    let store = AdminStore::open_for_boot(&path).expect("fresh install opens");
    assert_eq!(store.try_user_count(), Ok(0));
    assert!(path.exists(), "fresh install persists to disk, not memory");
}

#[test]
fn open_for_boot_keeps_existing_users() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("admin.db");
    {
        let store = AdminStore::open(&path).unwrap();
        store
            .create_user("admin", "password123", Role::Admin)
            .unwrap();
    }
    let store = AdminStore::open_for_boot(&path).unwrap();
    assert_eq!(store.try_user_count(), Ok(1));
}

#[test]
fn open_for_boot_refuses_existing_unreadable_db() {
    // #2000: um admin.db que existe e nao abre nao pode virar um console
    // vazio em memoria, cujo setup aceitaria um admin novo.
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("admin.db");
    std::fs::write(&path, vec![0xA5u8; 8192]).unwrap();
    let err = match AdminStore::open_for_boot(&path) {
        Ok(_) => panic!("corrupt admin.db must refuse the boot"),
        Err(e) => e,
    };
    assert!(err.contains("admin.db"), "names the file: {err}");
    assert!(err.contains("backup"), "names the way out: {err}");
}

#[test]
fn open_for_boot_falls_back_only_when_file_is_absent() {
    // Diretorio inexistente: o arquivo nao existe e nao da para cria-lo.
    // Nao ha admin a sequestrar, entao fica o comportamento antigo.
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("nao-existe").join("admin.db");
    let store = AdminStore::open_for_boot(&path).expect("in-memory fallback");
    assert_eq!(store.try_user_count(), Ok(0));
    assert!(!path.exists());
}

#[test]
fn open_for_boot_does_not_refuse_a_file_it_created_itself() {
    // Primeira instalacao: o `open` cria o arquivo e a migracao falha.
    // Simulado com um diretorio no lugar do `-wal`, o que faz o
    // `journal_mode=WAL` falhar depois de o arquivo principal existir.
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("admin.db");
    std::fs::create_dir(dir.path().join("admin.db-wal")).unwrap();
    assert!(!path.exists());
    match AdminStore::open(&path) {
        Ok(_) => return, // plataforma em que o WAL nao falha: nada a provar
        Err(_) => {
            let _ = std::fs::remove_file(&path);
        }
    }
    let store = AdminStore::open_for_boot(&path).expect("fresh install falls back");
    assert_eq!(store.try_user_count(), Ok(0));
}

#[test]
fn user_count() {
    let store = test_store();
    assert_eq!(store.user_count(), 0);
    store.create_user("a", "p", Role::Viewer).unwrap();
    assert_eq!(store.user_count(), 1);
}

#[test]
fn duplicate_username_fails() {
    let store = test_store();
    store.create_user("admin", "pass", Role::Admin).unwrap();
    let result = store.create_user("admin", "pass2", Role::Viewer);
    assert!(result.is_err());
}

#[test]
fn delete_user_cascades_sessions() {
    let store = test_store();
    let user = store.create_user("temp", "pass", Role::Viewer).unwrap();
    let session = store.create_session(&user.id, None, None).unwrap();

    store.delete_user(&user.id).unwrap();
    assert!(store.validate_session(&session.token).is_none());
}

// ── #1122: recovery tokens ───────────────────────────────────────

#[test]
fn recovery_token_is_single_use() {
    let mut store = test_store();
    let user = store.create_user("admin", "pass", Role::Admin).unwrap();

    let (id, code) = store.create_recovery_token(&user.id, 600).unwrap();
    assert!(!code.is_empty());

    let live = store.live_recovery_secrets(&user.id);
    assert_eq!(live.len(), 1);
    assert!(AdminStore::recovery_code_matches(
        &code, &live[0].1, &live[0].2
    ));
    assert!(!AdminStore::recovery_code_matches(
        "wrong", &live[0].1, &live[0].2
    ));

    // Exactly one winner: the second claim is indistinguishable from a
    // bad code, so concurrent /complete calls cannot both reset.
    assert!(store.claim_recovery_token(id).unwrap());
    assert!(!store.claim_recovery_token(id).unwrap());
    assert!(store.live_recovery_secrets(&user.id).is_empty());
}

#[test]
fn recovery_token_expires() {
    let mut store = test_store();
    let user = store.create_user("admin", "pass", Role::Admin).unwrap();

    let (_id, _code) = store.create_recovery_token(&user.id, -1).unwrap();
    assert!(store.live_recovery_secrets(&user.id).is_empty());
}

#[test]
fn recovery_token_reissue_supersedes_the_previous_one() {
    let mut store = test_store();
    let user = store.create_user("admin", "pass", Role::Admin).unwrap();

    let (_old_id, old_code) = store.create_recovery_token(&user.id, 600).unwrap();
    let (_new_id, new_code) = store.create_recovery_token(&user.id, 600).unwrap();
    assert_ne!(old_code, new_code);

    let live = store.live_recovery_secrets(&user.id);
    assert_eq!(live.len(), 1);
    assert!(AdminStore::recovery_code_matches(
        &new_code, &live[0].1, &live[0].2
    ));
    assert!(!AdminStore::recovery_code_matches(
        &old_code, &live[0].1, &live[0].2
    ));
}

#[test]
fn recovery_token_discard_and_purge() {
    let mut store = test_store();
    let user = store.create_user("admin", "pass", Role::Admin).unwrap();

    let (id, _code) = store.create_recovery_token(&user.id, 600).unwrap();
    store.discard_recovery_token(id).unwrap();
    assert!(store.live_recovery_secrets(&user.id).is_empty());

    store.create_recovery_token(&user.id, -1).unwrap();
    assert_eq!(store.purge_recovery_tokens(), 1);
}

#[test]
fn config_version_round_trip() {
    let store = test_store();
    let v1 = store
        .save_config_version("key: value1", Some("admin"), Some("initial"))
        .unwrap();
    assert_eq!(v1, 1);

    let v2 = store
        .save_config_version("key: value2", Some("admin"), Some("update"))
        .unwrap();
    assert_eq!(v2, 2);

    let versions = store.list_config_versions(10);
    assert_eq!(versions.len(), 2);
    assert_eq!(versions[0].version, 2);

    let yaml = store.get_config_version(1).unwrap();
    assert_eq!(yaml, "key: value1");
}

// ── TOTP (#1121) ──────────────────────────────────────────────────

fn usuario_totp(store: &AdminStore) -> String {
    store.create_user("cofre", "senha", Role::Admin).unwrap().id
}

/// Chave AES-256 dos testes. Nao e credencial: um padrao fixo basta para
/// exercitar cifra e decifra — mesma escolha de `secrets.rs`.
fn chave_de_teste() -> [u8; 32] {
    [7u8; 32]
}

#[test]
fn segredo_pendente_nao_liga_o_segundo_fator() {
    let store = test_store();
    let id = usuario_totp(&store);

    store
        .set_pending_totp_secret(&id, "JBSWY3DPEHPK3PXP", &chave_de_teste())
        .unwrap();

    assert_eq!(
        store
            .get_totp_secret(&id, &chave_de_teste())
            .expect("segredo legivel")
            .as_deref(),
        Some("JBSWY3DPEHPK3PXP")
    );
    assert!(
        !store.is_totp_enabled(&id).expect("estado 2fa legivel"),
        "guardar o segredo nao pode exigir o codigo; so `enable_totp` liga"
    );
}

#[test]
fn desligar_apaga_o_segredo_junto() {
    let store = test_store();
    let id = usuario_totp(&store);

    store
        .set_pending_totp_secret(&id, "JBSWY3DPEHPK3PXP", &chave_de_teste())
        .unwrap();
    store.enable_totp(&id).unwrap();
    assert!(store.is_totp_enabled(&id).expect("estado 2fa legivel"));

    store.disable_totp(&id).unwrap();

    assert!(!store.is_totp_enabled(&id).expect("estado 2fa legivel"));
    assert_eq!(
        store
            .get_totp_secret(&id, &chave_de_teste())
            .expect("segredo legivel"),
        None,
        "segredo sobrevivente seria uma reativacao sem novo enrollment"
    );
}

/// #1121 (regressao dos vereditos de seguranca): a mudanca de estado e o
/// evento de auditoria confirmam na mesma transacao. Se a trilha nao pode
/// ser gravada (tabela apagada, disco cheio, arquivo corrompido), a
/// operacao inteira falha — desligar o segundo fator sem deixar rastro e
/// exatamente o que um invasor com a sessao tentaria.
#[test]
fn desligar_sem_trilha_de_auditoria_nao_confirma() {
    let store = test_store();
    let id = usuario_totp(&store);
    store
        .set_pending_totp_secret(&id, "JBSWY3DPEHPK3PXP", &chave_de_teste())
        .unwrap();
    store.enable_totp(&id).unwrap();

    store
        .conn
        .execute("DROP TABLE audit_log", [])
        .expect("derrubar audit_log");

    assert!(
        store
            .disable_totp_audited(&id, "cofre", Some("127.0.0.1"))
            .is_err(),
        "sem trilha gravada a operacao nao pode confirmar"
    );
    assert!(
        store.is_totp_enabled(&id).expect("estado 2fa legivel"),
        "a recusa nao pode deixar o 2FA desligado por baixo da transacao"
    );
}

#[test]
fn usuario_novo_nasce_sem_segundo_fator() {
    let store = test_store();
    let id = usuario_totp(&store);

    assert!(
        !store
            .is_totp_enabled(&id)
            .expect("usuario novo tem estado legivel")
    );
    assert_eq!(
        store
            .get_totp_secret(&id, &chave_de_teste())
            .expect("segredo legivel"),
        None
    );
}

/// #1121 (pass-3): erro de leitura do segredo nao e "segredo ausente" —
/// sao estados diferentes, e quem chama precisa distinguir para recusar
/// com 500 em vez de responder "2FA nao configurado".
#[test]
fn leitura_do_segredo_ilegivel_e_erro_nao_ausencia() {
    let store = test_store();
    let id = usuario_totp(&store);

    assert_eq!(
        store
            .get_totp_secret(&id, &chave_de_teste())
            .expect("usuario novo e legivel"),
        None,
        "sem segredo guardado e um estado legitimo, nao erro"
    );

    store
        .conn
        .execute("ALTER TABLE admin_users DROP COLUMN totp_secret_enc", [])
        .expect("derrubar a coluna do segredo");

    assert!(
        store.get_totp_secret(&id, &chave_de_teste()).is_err(),
        "coluna sumida e erro de leitura e tem que chegar como Err"
    );
}

#[test]
fn errar_muito_trava_e_um_acerto_zera_a_contagem() {
    let store = test_store();
    let id = usuario_totp(&store);

    for _ in 0..TOTP_MAX_ATTEMPTS - 1 {
        store.record_totp_attempt(&id, false).unwrap();
    }
    assert!(
        !store
            .totp_attempts_exhausted(&id)
            .expect("contagem legivel"),
        "travar antes da ultima tentativa valida"
    );

    store.record_totp_attempt(&id, false).unwrap();
    assert!(
        store
            .totp_attempts_exhausted(&id)
            .expect("contagem legivel")
    );

    // Acertar nao pode deixar o dono travado.
    store.record_totp_attempt(&id, true).unwrap();
    assert!(
        !store
            .totp_attempts_exhausted(&id)
            .expect("contagem legivel")
    );
}

#[test]
fn a_contagem_e_por_usuario() {
    let store = test_store();
    let a = usuario_totp(&store);
    let b = store.create_user("outro", "senha", Role::Admin).unwrap().id;

    for _ in 0..TOTP_MAX_ATTEMPTS {
        store.record_totp_attempt(&a, false).unwrap();
    }

    assert!(store.totp_attempts_exhausted(&a).expect("contagem legivel"));
    assert!(
        !store.totp_attempts_exhausted(&b).expect("contagem legivel"),
        "um usuario errando nao pode travar os outros"
    );
}

// ── #1140: o lockout sobrevive ao processo ───────────────────────

/// O motivo da issue: com a contagem em `HashMap` de processo, quem tinha
/// a senha reiniciava o gateway e voltava a forcar o codigo de 6 digitos.
/// Reabrir o mesmo arquivo e o que um restart faz.
#[test]
fn o_lockout_sobrevive_a_reabertura_do_banco() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("admin.db");

    let id = {
        let store = AdminStore::open(&path).expect("abrir banco");
        let id = usuario_totp(&store);
        for _ in 0..TOTP_MAX_ATTEMPTS {
            store.record_totp_attempt(&id, false).unwrap();
        }
        assert!(
            store
                .totp_attempts_exhausted(&id)
                .expect("contagem legivel")
        );
        id
    };

    let reaberto = AdminStore::open(&path).expect("reabrir banco");
    assert!(
        reaberto
            .totp_attempts_exhausted(&id)
            .expect("contagem legivel"),
        "restart nao pode zerar o lockout do segundo fator (#1140)"
    );
}

/// Duas conexoes sobre o mesmo arquivo sao a aproximacao de duas
/// instancias do gateway: a contagem e compartilhada, entao distribuir as
/// tentativas entre instancias nao multiplica o orcamento de chutes.
#[test]
fn duas_instancias_compartilham_a_contagem() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("admin.db");

    let uma = AdminStore::open(&path).expect("abrir banco");
    let outra = AdminStore::open(&path).expect("abrir o mesmo banco");
    let id = usuario_totp(&uma);

    for _ in 0..TOTP_MAX_ATTEMPTS - 1 {
        uma.record_totp_attempt(&id, false).unwrap();
    }
    outra.record_totp_attempt(&id, false).unwrap();

    assert!(
        uma.totp_attempts_exhausted(&id).expect("contagem legivel"),
        "tentativa feita na outra instancia tem que contar aqui (#1140)"
    );
}

/// A tabela nao pode crescer sem limite: tentativa fora da janela sai na
/// mesma transacao que conta.
#[test]
fn tentativa_expirada_e_apagada_e_nao_conta() {
    let store = test_store();
    let id = usuario_totp(&store);

    for _ in 0..TOTP_MAX_ATTEMPTS {
        store.record_totp_attempt(&id, false).unwrap();
    }
    assert!(
        store
            .totp_attempts_exhausted(&id)
            .expect("contagem legivel")
    );

    // Envelhece as tentativas para fora da janela de 15 minutos.
    store
        .conn
        .execute(
            "UPDATE totp_attempts SET attempted_at = datetime('now', '-1 hour')",
            [],
        )
        .expect("envelhecer as tentativas");

    assert!(
        !store
            .totp_attempts_exhausted(&id)
            .expect("contagem legivel"),
        "tentativa fora da janela nao pode travar"
    );
    let restantes: i64 = store
        .conn
        .query_row("SELECT COUNT(*) FROM totp_attempts", [], |r| r.get(0))
        .expect("contar linhas");
    assert_eq!(restantes, 0, "a contagem tambem faz a limpeza");
}

/// Contagem ilegivel tem que chegar como `Err` — o chamador recusa
/// fechado. Tratar como "nao esgotou" devolveria o brute force de graca.
#[test]
fn contagem_ilegivel_e_erro_nao_zero() {
    let store = test_store();
    let id = usuario_totp(&store);

    store
        .conn
        .execute("DROP TABLE totp_attempts", [])
        .expect("derrubar a tabela de tentativas");

    assert!(store.totp_attempts_exhausted(&id).is_err());
    assert!(store.record_totp_attempt(&id, false).is_err());
}

// ── #1141: o segredo nao fica em claro no arquivo ────────────────

/// O nucleo da issue: quem le o `admin.db` nao pode achar o base32 do
/// segundo fator la dentro.
#[test]
fn o_segredo_nao_fica_em_claro_na_coluna() {
    let store = test_store();
    let id = usuario_totp(&store);
    store
        .set_pending_totp_secret(&id, "JBSWY3DPEHPK3PXP", &chave_de_teste())
        .unwrap();

    let (claro, cifrado, nonce): (Option<String>, Option<Vec<u8>>, Option<Vec<u8>>) = store
        .conn
        .query_row(
            "SELECT totp_secret, totp_secret_enc, totp_secret_nonce
             FROM admin_users WHERE id = ?1",
            params![id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .expect("ler as colunas cruas");

    assert_eq!(claro, None, "a coluna em claro tem que ficar vazia (#1141)");
    let cifrado = cifrado.expect("segredo cifrado presente");
    assert!(nonce.is_some(), "nonce gravado junto com o ciphertext");
    assert!(
        !cifrado.windows(16).any(|w| w == b"JBSWY3DPEHPK3PXP"),
        "o base32 nao pode aparecer dentro do ciphertext"
    );

    // E ainda assim o segredo volta legivel para quem tem a chave.
    assert_eq!(
        store
            .get_totp_secret(&id, &chave_de_teste())
            .expect("segredo legivel")
            .as_deref(),
        Some("JBSWY3DPEHPK3PXP")
    );
}

/// Chave errada e indisponibilidade, nao "2FA desligado": decifrar
/// falhando tem que chegar como `Err`, senao trocar a chave mestra sem
/// re-cifrar abriria o painel so com a senha.
#[test]
fn chave_errada_nao_vira_segredo_ausente() {
    let store = test_store();
    let id = usuario_totp(&store);
    store
        .set_pending_totp_secret(&id, "JBSWY3DPEHPK3PXP", &chave_de_teste())
        .unwrap();

    assert!(store.get_totp_secret(&id, &[9u8; 32]).is_err());
}

/// Lazy upgrade forward-only: banco vindo do #1137 tem o segredo em claro.
/// A primeira leitura devolve o valor **e** migra para as colunas
/// cifradas, zerando a antiga.
#[test]
fn segredo_herdado_em_claro_migra_na_primeira_leitura() {
    let store = test_store();
    let id = usuario_totp(&store);

    // Estado de um banco anterior ao #1141, escrito na mao.
    store
        .conn
        .execute(
            "UPDATE admin_users SET totp_secret = ?1 WHERE id = ?2",
            params!["JBSWY3DPEHPK3PXP", id],
        )
        .expect("plantar o segredo em claro");

    assert_eq!(
        store
            .get_totp_secret(&id, &chave_de_teste())
            .expect("segredo legivel")
            .as_deref(),
        Some("JBSWY3DPEHPK3PXP"),
        "a migracao nao pode quebrar o login de quem ja tinha 2FA"
    );

    let claro: Option<String> = store
        .conn
        .query_row(
            "SELECT totp_secret FROM admin_users WHERE id = ?1",
            params![id],
            |r| r.get(0),
        )
        .expect("ler a coluna em claro");
    assert_eq!(claro, None, "a leitura tem que zerar a coluna em claro");

    // E segue legivel depois da migracao, agora pelo caminho cifrado.
    assert_eq!(
        store
            .get_totp_secret(&id, &chave_de_teste())
            .expect("segredo legivel")
            .as_deref(),
        Some("JBSWY3DPEHPK3PXP")
    );
}

/// Desligar apaga o ciphertext e o nonce junto — um dos dois sobrevivendo
/// seria segredo reaproveitavel depois do desligamento.
#[test]
fn desligar_apaga_ciphertext_e_nonce() {
    let store = test_store();
    let id = usuario_totp(&store);
    store
        .set_pending_totp_secret(&id, "JBSWY3DPEHPK3PXP", &chave_de_teste())
        .unwrap();
    store.enable_totp(&id).unwrap();

    store.disable_totp(&id).unwrap();

    let (enc, nonce): (Option<Vec<u8>>, Option<Vec<u8>>) = store
        .conn
        .query_row(
            "SELECT totp_secret_enc, totp_secret_nonce FROM admin_users WHERE id = ?1",
            params![id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .expect("ler as colunas cruas");
    assert_eq!(enc, None);
    assert_eq!(nonce, None);
}

/// `admin_users` criada **antes** do #1121, sem as colunas de 2FA. E o
/// banco de toda instalacao existente: `CREATE TABLE IF NOT EXISTS` nao
/// toca nela, entao so o `ALTER TABLE` condicional salva o boot.
fn banco_antigo_sem_2fa(path: &std::path::Path) {
    let conn = Connection::open(path).expect("db temporario");
    conn.execute_batch(
        "PRAGMA foreign_keys=ON;
         CREATE TABLE admin_users (
             id TEXT PRIMARY KEY,
             username TEXT UNIQUE NOT NULL,
             password_hash TEXT NOT NULL,
             password_salt TEXT NOT NULL,
             role TEXT NOT NULL DEFAULT 'viewer',
             created_at TEXT NOT NULL DEFAULT (datetime('now')),
             updated_at TEXT NOT NULL DEFAULT (datetime('now')),
             last_login TEXT
         );",
    )
    .expect("schema antigo");
    conn.execute(
        "INSERT INTO admin_users (id, username, password_hash, password_salt, role)
         VALUES ('u1', 'dono', 'hash', 'salt', 'admin')",
        [],
    )
    .expect("usuario antigo");
}

#[test]
fn instalacao_antiga_ganha_as_colunas_de_2fa_no_boot() {
    let path = std::env::temp_dir().join(format!("garra-2fa-migr-{}.db", uuid::Uuid::new_v4()));
    banco_antigo_sem_2fa(&path);

    let store = AdminStore::open(&path).expect("abrir banco antigo nao pode falhar");

    assert!(
        !store.is_totp_enabled("u1").expect("estado 2fa legivel"),
        "usuario antigo nasce com 2FA desligado"
    );
    store
        .set_pending_totp_secret("u1", "JBSWY3DPEHPK3PXP", &chave_de_teste())
        .expect("escrever segredo");
    assert_eq!(
        store
            .get_totp_secret("u1", &chave_de_teste())
            .expect("segredo legivel")
            .as_deref(),
        Some("JBSWY3DPEHPK3PXP")
    );

    let _ = std::fs::remove_file(&path);
}

/// O `PRAGMA table_info` e o `ALTER TABLE` nao sao atomicos: dois processos
/// podem ler o schema antes de qualquer um escrever. O perdedor tem que
/// receber sucesso, nao um `Err` que faz o `open_for_boot` recusar o boot
/// de uma instalacao saudavel (#2000).
#[test]
fn rodar_a_migracao_duas_vezes_e_sucesso() {
    let store = test_store();
    store.ensure_totp_columns().expect("segunda passada");
    store.ensure_totp_columns().expect("terceira passada");
}

// ── #1992: `kdf_params` distingue "sem linha" de "linha ilegivel" ────

#[test]
fn kdf_params_is_none_only_when_there_is_no_row() {
    let mut store = AdminStore::in_memory().expect("in-memory store");
    assert_eq!(store.kdf_params(), Ok(None));

    let recorded = (1u32, "c2FsdA==".to_string(), 600_000u32);
    store
        .apply_secret_rekey(&[], &[], &[], &recorded)
        .expect("record");
    assert_eq!(store.kdf_params(), Ok(Some(recorded)));
}

/// Antes do #1992 este leitor devolvia `None` para salt vazio, contagem
/// zero e qualquer erro de leitura. `None` e a resposta de "instalacao que
/// ainda nao foi re-chaveada", e o boot respondia a ela derivando a chave
/// do salt constante. Julgar a linha e trabalho de quem deriva
/// (`shared::usable_stored_params`); aqui ela so nao pode sumir.
#[test]
fn kdf_params_never_reports_a_damaged_row_as_absent() {
    let store = AdminStore::in_memory().expect("in-memory store");

    // Contagem zero e salt vazio: chegam ao chamador como estao.
    store
        .conn
        .execute(
            "INSERT INTO kdf_params (id, version, salt, iterations) VALUES (1, 1, '', 0)",
            [],
        )
        .expect("insert");
    assert_eq!(store.kdf_params(), Ok(Some((1, String::new(), 0))));

    // Negativo: `as u32` transformava -1 em ~4 bilhoes de iteracoes.
    store
        .conn
        .execute("UPDATE kdf_params SET iterations = -1 WHERE id = 1", [])
        .expect("update");
    let err = store.kdf_params().expect_err("fora da faixa");
    assert!(err.contains("iteration count -1"), "{err}");

    store
        .conn
        .execute(
            "UPDATE kdf_params SET iterations = 600000, version = -3 WHERE id = 1",
            [],
        )
        .expect("update");
    let err = store.kdf_params().expect_err("fora da faixa");
    assert!(err.contains("parameter version -3"), "{err}");

    // Tipo errado na coluna (SQLite nao impoe): erro de leitura, nao `None`.
    store
        .conn
        .execute(
            "UPDATE kdf_params SET version = 1, salt = x'00ff' WHERE id = 1",
            [],
        )
        .expect("update");
    let err = store.kdf_params().expect_err("blob no lugar de texto");
    assert!(err.contains("cannot be read"), "{err}");
}
