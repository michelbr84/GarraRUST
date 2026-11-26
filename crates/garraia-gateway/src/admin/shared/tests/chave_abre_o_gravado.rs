//! Testes do #2013: a chave do admin tem que abrir o que esta gravado.

use super::*;

// ── #2013: a chave resolvida tem que abrir o que esta gravado ────────

/// Frase-senha trocada numa instalacao ja re-chaveada. Antes da #2013 a linha
/// de `kdf_params` era valida, a chave saia derivada dela e o painel subia
/// com uma chave que nao abria nada: 500 em toda leitura, 500 no login de
/// quem tem 2FA, e os segredos novos cifrados sob a chave errada.
#[test]
fn a_changed_passphrase_over_stored_secrets_disables_the_panel() {
    let mut store = migrated_store_with_a_secret();
    let antes = store
        .get_secret_raw("default", "openai", "api_key")
        .expect("segredo presente");
    let linha = store.kdf_params().expect("readable");

    let err = resolve_with_passphrase(&mut store, "outra frase-senha").expect_err("must refuse");

    assert_eq!(
        err,
        AdminKeyError::KeyOpensNoStoredSecret {
            key_source: MasterKeySource::Passphrase,
            count: 1,
        }
    );
    let msg = err.to_string();
    assert!(msg.contains("passphrase"), "{msg}");
    assert!(msg.contains("changed"), "a causa provavel: {msg}");
    assert!(msg.contains("admin console stays disabled"), "{msg}");
    assert_eq!(
        store
            .get_secret_raw("default", "openai", "api_key")
            .expect("segredo presente"),
        antes,
        "a recusa nao reescreve o ciphertext"
    );
    assert_eq!(store.kdf_params().expect("readable"), linha);

    // A frase-senha certa de volta e o painel volta.
    let key = resolve_with_passphrase(&mut store, PASS).expect("frase-senha original");
    let (ct, nonce) = antes;
    assert_eq!(
        crate::admin::secrets::decrypt_value(&ct, &nonce, &key).expect("abre"),
        b"sk-live-do-not-leak"
    );
}

/// O segredo do 2FA sozinho tambem conta: e ele que trava o login do dono.
#[test]
fn a_changed_passphrase_over_a_totp_seed_alone_disables_the_panel() {
    let mut store = AdminStore::in_memory().expect("in-memory store");
    let key = resolve_with_passphrase(&mut store, PASS).expect("primeiro boot");
    let user = store
        .create_user("dono", "senha", crate::admin::rbac::Role::Admin)
        .expect("usuario");
    store
        .set_pending_totp_secret(&user.id, "JBSWY3DPEHPK3PXP", &key)
        .expect("segredo do 2FA");
    store.enable_totp(&user.id).expect("2fa ligado");

    let err = resolve_with_passphrase(&mut store, "outra").expect_err("must refuse");
    assert!(
        matches!(
            err,
            AdminKeyError::KeyOpensNoStoredSecret {
                key_source: MasterKeySource::Passphrase,
                count: 1
            }
        ),
        "{err:?}"
    );
}

/// Instalacao sem nenhum ciphertext continua subindo, com qualquer frase-senha:
/// nao ha o que conferir nem o que perder.
#[test]
fn a_store_without_ciphertexts_boots_with_any_passphrase() {
    let mut store = AdminStore::in_memory().expect("in-memory store");
    let first = resolve_with_passphrase(&mut store, PASS).expect("primeiro boot");
    assert!(store.kdf_params().expect("readable").is_some());

    let other = resolve_with_passphrase(&mut store, "outra").expect("sem segredo gravado");
    assert_ne!(first, other);
}

/// "Nenhum abre", e nao "algum nao abre": uma linha corrompida nao derruba o
/// painel inteiro quando as outras abrem com a chave.
#[test]
fn one_damaged_ciphertext_does_not_take_the_panel_down() {
    let mut store = migrated_store_with_a_secret();
    let key = resolve_with_passphrase(&mut store, PASS).expect("steady state");
    let (lixo, nonce) =
        crate::admin::secrets::encrypt_value(b"x", &[4u8; MASTER_KEY_LEN]).expect("encrypt");
    store
        .set_secret("default", "acme", "token", &lixo, &nonce, None)
        .expect("seed");

    assert_eq!(
        resolve_with_passphrase(&mut store, PASS).expect("um abre, basta"),
        key
    );
}

// ── #2013 item 2: master.key ─────────────────────────────────────────

fn store_with_a_secret_under(key: &[u8]) -> AdminStore {
    let store = AdminStore::in_memory().expect("in-memory store");
    let (ct, nonce) = crate::admin::secrets::encrypt_value(b"sk-live", key).expect("encrypt");
    store
        .set_secret("default", "openai", "api_key", &ct, &nonce, None)
        .expect("seed");
    store
}

#[test]
fn a_master_key_that_opens_the_stored_secrets_is_used() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("master.key");
    let key = vec![1u8; MASTER_KEY_LEN];
    std::fs::write(&path, &key).expect("write");
    let store = store_with_a_secret_under(&key);

    assert_eq!(master_key_file_key(&store, &path).expect("abre"), key);
}

/// `master.key` trocado por outro arquivo bem-formado: tamanho certo, chave
/// errada.
#[test]
fn a_swapped_master_key_disables_the_panel() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("master.key");
    std::fs::write(&path, [2u8; MASTER_KEY_LEN]).expect("write");
    let store = store_with_a_secret_under(&[1u8; MASTER_KEY_LEN]);

    let err = master_key_file_key(&store, &path).expect_err("must refuse");
    assert_eq!(
        err,
        AdminKeyError::KeyOpensNoStoredSecret {
            key_source: MasterKeySource::MasterKeyFile,
            count: 1,
        }
    );
    assert_eq!(
        std::fs::read(&path).expect("arquivo intacto"),
        vec![2u8; MASTER_KEY_LEN]
    );
}

/// Antes da #2013 um `master.key` ilegivel ou ausente virava uma chave
/// aleatoria nova, e tudo o que estava cifrado, 2FA incluido, deixava de abrir.
/// Com segredos gravados o painel vai a 503 e o arquivo fica como esta.
#[test]
fn a_lost_master_key_over_stored_secrets_disables_the_panel_and_touches_nothing() {
    let store = store_with_a_secret_under(&[1u8; MASTER_KEY_LEN]);

    // Tamanho errado: o arquivo continua la, byte a byte, sem `.unreadable`.
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("master.key");
    std::fs::write(&path, [1u8; 7]).expect("write");
    let err = master_key_file_key(&store, &path).expect_err("must refuse");
    assert_eq!(
        err,
        AdminKeyError::MasterKeyFileLost {
            state: "the wrong length",
            count: 1,
        }
    );
    assert_eq!(std::fs::read(&path).expect("intacto"), vec![1u8; 7]);
    assert!(!path.with_extension("key.unreadable").exists());

    // Ausente: nenhuma chave nova e criada.
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("master.key");
    let err = master_key_file_key(&store, &path).expect_err("must refuse");
    assert_eq!(
        err,
        AdminKeyError::MasterKeyFileLost {
            state: "missing",
            count: 1,
        }
    );
    assert!(!path.exists(), "nenhuma chave nova sobre segredos antigos");
    let msg = err.to_string();
    assert!(msg.contains("restore master.key"), "{msg}");
    assert!(msg.contains("admin console stays disabled"), "{msg}");
}

/// Sem nada gravado a regeneracao continua: nao ha o que perder.
#[test]
fn a_lost_master_key_over_an_empty_store_is_regenerated() {
    let store = AdminStore::in_memory().expect("in-memory store");

    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("master.key");
    let key = master_key_file_key(&store, &path).expect("instalacao nova");
    assert_eq!(key.len(), MASTER_KEY_LEN);
    assert_eq!(std::fs::read(&path).expect("gravado"), key);
    assert_eq!(
        master_key_file_key(&store, &path).expect("segundo boot"),
        key,
        "o proximo boot le a mesma chave"
    );

    std::fs::write(&path, [9u8; 5]).expect("corrompe");
    let nova = master_key_file_key(&store, &path).expect("regenerada");
    assert_ne!(nova, key);
    assert_eq!(
        std::fs::read(path.with_extension("key.unreadable")).expect("preservado"),
        vec![9u8; 5]
    );
}

/// As versoes arquivadas entram na varredura: aqui so a versao antiga abre com
/// a chave, e isso basta. Se `secret_versions` saisse de `stored_ciphertexts`,
/// a chave seria recusada (o segredo vivo esta sob outra).
#[test]
fn an_archived_version_alone_is_enough_to_prove_the_key() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("master.key");
    let key = vec![1u8; MASTER_KEY_LEN];
    std::fs::write(&path, &key).expect("write");

    let store = store_with_a_secret_under(&key);
    let (ct, nonce) =
        crate::admin::secrets::encrypt_value(b"sk-novo", &[6u8; MASTER_KEY_LEN]).expect("enc");
    store
        .set_secret("default", "openai", "api_key", &ct, &nonce, None)
        .expect("a versao antiga vai para secret_versions");
    assert_eq!(store.secret_version_ciphertexts().expect("le").len(), 1);

    assert_eq!(
        master_key_file_key(&store, &path).expect("a versao abre"),
        key
    );
}

/// Sem conseguir ler os ciphertexts, nada prova que a chave e a certa.
#[test]
fn ciphertexts_that_cannot_be_read_disable_the_panel() {
    let mut store = migrated_store_with_a_secret();
    store
        .execute_raw_for_test("DROP TABLE secret_versions")
        .expect("drop");
    let err = resolve_with_passphrase(&mut store, PASS).expect_err("must refuse");
    assert!(
        matches!(err, AdminKeyError::StoredSecretsUnreadable(_)),
        "{err:?}"
    );

    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("master.key");
    let err = master_key_file_key(&store, &path).expect_err("must refuse");
    assert!(
        matches!(err, AdminKeyError::StoredSecretsUnreadable(_)),
        "{err:?}"
    );
    assert!(
        !path.exists(),
        "nenhuma chave nova sem saber o que esta gravado"
    );
}

/// Um `master.key` que existe mas nao se le nao prova corrupcao: nunca e
/// trocado, nem com o store vazio.
#[test]
fn a_master_key_that_exists_but_cannot_be_read_is_never_replaced() {
    let store = AdminStore::in_memory().expect("in-memory store");
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("master.key");
    // Um diretorio no lugar do arquivo: `read` falha sem ser NotFound, mesmo
    // como root (que ignoraria um chmod 000).
    std::fs::create_dir(&path).expect("dir");

    let err = master_key_file_key(&store, &path).expect_err("must refuse");
    assert!(
        matches!(err, AdminKeyError::MasterKeyFileUnreadable(_)),
        "{err:?}"
    );
    assert!(path.is_dir(), "intocado");
    assert!(!path.with_extension("key.unreadable").exists());
}

/// Uma copia preservada antes nunca e sobrescrita.
#[test]
fn an_earlier_preserved_copy_is_never_overwritten() {
    let store = AdminStore::in_memory().expect("in-memory store");
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("master.key");
    std::fs::write(path.with_extension("key.unreadable"), b"primeira").expect("write");
    std::fs::write(&path, b"segunda").expect("write");

    master_key_file_key(&store, &path).expect("regenerada");

    assert_eq!(
        std::fs::read(path.with_extension("key.unreadable")).expect("le"),
        b"primeira"
    );
    assert_eq!(
        std::fs::read(path.with_extension("key.unreadable.1")).expect("le"),
        b"segunda"
    );
}

#[cfg(unix)]
#[test]
fn a_new_master_key_is_owner_only_from_creation() {
    use std::os::unix::fs::PermissionsExt;
    let store = AdminStore::in_memory().expect("in-memory store");
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("admin").join("master.key");

    master_key_file_key(&store, &path).expect("criada");

    let mode = std::fs::metadata(&path).expect("meta").permissions().mode();
    assert_eq!(mode & 0o777, 0o600, "{mode:o}");
}

/// Uma chave que so existiria em memoria cifraria os segredos desta sessao sob
/// algo que o proximo boot nao tem. Aqui um symlink pendurado ocupa o caminho:
/// `read` diz NotFound, mas `create_new` nao passa por cima dele.
#[cfg(unix)]
#[test]
fn a_master_key_that_cannot_be_written_disables_the_panel() {
    let store = AdminStore::in_memory().expect("in-memory store");
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("master.key");
    std::os::unix::fs::symlink(dir.path().join("nao-existe"), &path).expect("symlink");

    let err = master_key_file_key(&store, &path).expect_err("must refuse");
    assert!(
        matches!(err, AdminKeyError::MasterKeyFileNotPersisted(_)),
        "{err:?}"
    );
    assert!(
        !dir.path().join("nao-existe").exists(),
        "nada seguiu o symlink"
    );
}
