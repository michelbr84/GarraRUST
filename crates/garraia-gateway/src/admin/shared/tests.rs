//! Testes de `admin::shared` (derivacao e re-key da chave mestra do admin).

use super::*;

const PASS: &str = "correct horse battery staple";

fn params_of(keys: &AdminKeys) -> StoredKdfParams {
    let p = keys.pending.as_ref().expect("pending params");
    (p.version, p.salt.clone(), p.iterations.get())
}

#[test]
fn generated_salts_differ_between_installations() {
    let a = KdfParams::generate().expect("a");
    let b = KdfParams::generate().expect("b");
    assert_ne!(a.salt, b.salt, "two installations must not share a salt");
    assert_eq!(a.salt_bytes().expect("decode").len(), KDF_SALT_LEN);

    // The regression this whole change exists for: same passphrase, two
    // installations, two different master keys.
    let key_a = pbkdf2_key(&a.salt_bytes().unwrap(), a.iterations, PASS);
    let key_b = pbkdf2_key(&b.salt_bytes().unwrap(), b.iterations, PASS);
    assert_ne!(key_a, key_b);
}

#[test]
fn new_parameters_differ_from_legacy() {
    // Compile-time, so weakening them fails the build rather than a test.
    const _: () = assert!(KDF_ITERATIONS.get() >= 600_000);
    const _: () = assert!(KDF_ITERATIONS.get() != LEGACY_KDF_ITERATIONS.get());
    const _: () = assert!(KDF_SALT_LEN == 32);
    assert_ne!(KDF_ITERATIONS, LEGACY_KDF_ITERATIONS);
}

#[test]
fn stored_params_reproduce_the_same_key_every_boot() {
    // The bug the single-resolution-point design exists to prevent: with no
    // params recorded, each derivation mints a fresh salt. Once they ARE
    // recorded, every later boot must land on the exact same key.
    let first = derive_with_passphrase(None, PASS).expect("first boot");
    assert!(first.migration_pending());
    let recorded = params_of(&first);

    let second = derive_with_passphrase(Some(recorded.clone()), PASS).expect("second boot");
    let third = derive_with_passphrase(Some(recorded), PASS).expect("third boot");
    assert_eq!(first.current, second.current);
    assert_eq!(second.current, third.current);
    assert!(!second.migration_pending(), "nothing left to migrate");
}

// ── #1992: nothing but a never-re-keyed install derives from the legacy salt ──

/// A `kdf_params` row as this code records it, for tests that damage one
/// field at a time.
fn good_row() -> StoredKdfParams {
    (
        KDF_PARAMS_VERSION,
        BASE64.encode([7u8; KDF_SALT_LEN]),
        KDF_ITERATIONS.get(),
    )
}

/// Before #1992 every one of these rows made `current` the key derived from
/// `LEGACY_KDF_SALT` — the key that encrypts — with `legacy: None` and
/// `pending: None`, so no migration was on its way either. A recorded row
/// means the re-key committed and the secrets are under the NEW key, so the
/// legacy key opened none of them: the fallback bought nothing and cost a
/// KDF downgrade on every secret written afterwards.
#[test]
fn unusable_stored_params_refuse_instead_of_falling_back_to_the_legacy_key() {
    let (version, salt, iterations) = good_row();
    let cases = [
        (
            (version, "!!!not base64!!!".to_string(), iterations),
            "not valid base64",
        ),
        ((version, salt.clone(), 0), "iteration count is 0"),
        // Well-formed, and exactly what a KDF downgrade through a writable
        // admin.db looks like.
        ((version, salt.clone(), 1), "iteration count is 1"),
        (
            (version, salt.clone(), LEGACY_KDF_ITERATIONS.get()),
            "below the minimum",
        ),
        (
            (version, BASE64.encode([7u8; 8]), iterations),
            "8 bytes long",
        ),
        ((version, String::new(), iterations), "0 bytes long"),
        (
            (version + 1, salt.clone(), iterations),
            "unknown parameter version",
        ),
        ((0, salt.clone(), iterations), "unknown parameter version"),
    ];

    for (bad, reason) in cases {
        let err = match derive_with_passphrase(Some(bad.clone()), PASS) {
            Ok(_) => panic!("{bad:?} must not yield a key"),
            Err(e) => e,
        };
        assert!(
            matches!(err, AdminKeyError::StoredParamsUnusable(_)),
            "{bad:?} -> {err:?}"
        );
        let msg = err.to_string();
        assert!(msg.contains(reason), "{bad:?}: {msg}");
        // Actionable: names the table, says it refuses, says what to do.
        assert!(msg.contains("kdf_params"), "{msg}");
        assert!(msg.contains("Refusing to start"), "{msg}");
        assert!(msg.contains("restore admin.db from a backup"), "{msg}");
    }
}

/// The guard above must not reject what this very code records.
#[test]
fn a_row_as_recorded_by_the_rekey_is_usable() {
    const _: () = assert!(KDF_ITERATIONS.get() >= MIN_STORED_KDF_ITERATIONS);
    const _: () = assert!(LEGACY_KDF_ITERATIONS.get() < MIN_STORED_KDF_ITERATIONS);

    let keys = derive_with_passphrase(Some(good_row()), PASS).expect("usable");
    assert!(!keys.migration_pending());
    assert_ne!(
        keys.current,
        pbkdf2_key(LEGACY_KDF_SALT, LEGACY_KDF_ITERATIONS, PASS)
    );

    let fresh = params_of(&derive_with_passphrase(None, PASS).expect("fresh"));
    assert!(usable_stored_params(&fresh).is_ok(), "{fresh:?}");
}

/// The property the issue is about, stated directly: whatever the input,
/// the key that ENCRYPTS is never the one derived from the constant salt.
#[test]
fn the_encrypting_key_is_never_derived_from_the_legacy_salt() {
    let legacy = pbkdf2_key(LEGACY_KDF_SALT, LEGACY_KDF_ITERATIONS, PASS);
    let (version, salt, iterations) = good_row();

    let mut yielded_a_key = 0;
    for stored in [
        None,
        Some(good_row()),
        Some((version, "!!!".to_string(), iterations)),
        Some((version, salt, 0)),
    ] {
        if let Ok(keys) = derive_with_passphrase(stored, PASS) {
            assert_ne!(keys.current, legacy);
            yielded_a_key += 1;
        }
    }
    // Not vacuous: the two healthy inputs really do produce a key.
    assert_eq!(yielded_a_key, 2);
}

/// End to end through the boot entry point: a damaged row refuses, and the
/// refusal writes nothing — neither the row nor the secret it describes.
#[test]
fn resolve_refuses_a_damaged_row_and_leaves_the_store_untouched() {
    let mut store = AdminStore::in_memory().expect("in-memory store");

    // A healthy, re-keyed installation holding one secret.
    let key = resolve_with_passphrase(&mut store, PASS).expect("first boot");
    let recorded = store.kdf_params().expect("readable").expect("recorded");
    let (ciphertext, nonce) =
        super::super::secrets::encrypt_value(b"sk-live-do-not-leak", &key).expect("encrypt");
    store
        .set_secret("default", "openai", "api_key", &ciphertext, &nonce, None)
        .expect("seed secret");
    assert_eq!(
        resolve_with_passphrase(&mut store, PASS).expect("second boot"),
        key,
        "steady state must land on the same key"
    );

    // The row is damaged (or tampered with): same salt, one iteration.
    let damaged = (recorded.0, recorded.1.clone(), 1u32);
    store
        .apply_secret_rekey(&[], &[], &[], &damaged)
        .expect("write the damaged row");

    let err = resolve_with_passphrase(&mut store, PASS).expect_err("must refuse");
    assert!(
        matches!(err, AdminKeyError::StoredParamsUnusable(_)),
        "{err:?}"
    );
    assert_eq!(
        store.kdf_params().expect("readable"),
        Some(damaged),
        "a refusal must not rewrite the row it refused"
    );
    let (stored, stored_nonce) = store
        .get_secret_raw("default", "openai", "api_key")
        .expect("secret still present");
    assert_eq!(stored, ciphertext, "ciphertext must be byte-identical");
    assert_eq!(stored_nonce, nonce);

    // And putting the row back is all it takes.
    store
        .apply_secret_rekey(&[], &[], &[], &recorded)
        .expect("restore the row");
    assert_eq!(
        resolve_with_passphrase(&mut store, PASS).expect("boots again"),
        key
    );
}

// ── Forward-only re-key migration ────────────────────────────────────

fn pending_keys(passphrase: &str) -> AdminKeys {
    derive_with_passphrase(None, passphrase).expect("derive")
}

#[test]
fn migration_reencrypts_secrets_and_records_params_atomically() {
    let mut store = AdminStore::in_memory().expect("in-memory store");
    let keys = pending_keys(PASS);
    assert!(keys.migration_pending());
    assert!(
        store.kdf_params().expect("readable").is_none(),
        "nothing recorded yet"
    );

    // A secret as it exists on a legacy install: under the key derived from
    // the shared constant salt.
    let legacy = keys.legacy.clone().expect("legacy key");
    let (ciphertext, nonce) =
        super::super::secrets::encrypt_value(b"sk-live-do-not-leak", &legacy).expect("encrypt");
    store
        .set_secret("default", "openai", "api_key", &ciphertext, &nonce, None)
        .expect("seed secret");

    let rekeyed = migrate_admin_secrets_kdf(&mut store, &keys).expect("migration");
    assert_eq!(rekeyed, 1, "one live secret, no archived versions yet");

    let (stored, stored_nonce) = store
        .get_secret_raw("default", "openai", "api_key")
        .expect("secret still present");
    let plaintext = super::super::secrets::decrypt_value(&stored, &stored_nonce, &keys.current)
        .expect("must decrypt with the new key");
    assert_eq!(plaintext, b"sk-live-do-not-leak");
    assert!(
        super::super::secrets::decrypt_value(&stored, &stored_nonce, &legacy).is_err(),
        "the legacy key must no longer open the secret"
    );

    // Params landed in the SAME transaction, so the next boot rederives the
    // very key the ciphertexts are now under.
    let recorded = store
        .kdf_params()
        .expect("readable")
        .expect("params recorded");
    let next_boot = derive_with_passphrase(Some(recorded), PASS).expect("next boot");
    assert_eq!(next_boot.current, keys.current);
    assert!(!next_boot.migration_pending());
}

// ── #1141: o segredo do 2FA entra no re-key da chave mestra ──────────

/// O buraco que a revisao deste PR pegou: cifrar o segredo do 2FA criou
/// uma dependencia da chave mestra que a rotacao de KDF nao conhecia. Se
/// o re-key re-cifrasse so `secrets`/`secret_versions`, o segredo do
/// segundo fator ficaria sob a chave antiga enquanto o processo passa a
/// usar a nova — `get_totp_secret` viraria `Err` e o login do dono
/// responderia 500 para sempre, sem recovery code para sair.
#[test]
fn migration_reencrypts_the_admin_totp_secret_too() {
    let mut store = AdminStore::in_memory().expect("in-memory store");
    let keys = pending_keys(PASS);
    let legacy = keys.legacy.clone().expect("legacy key");
    let user = store
        .create_user("dono", "senha", super::super::rbac::Role::Admin)
        .expect("usuario");

    // Estado de uma instalacao legada: o segredo do 2FA cifrado sob a
    // chave derivada do salt compartilhado.
    store
        .set_pending_totp_secret(&user.id, "JBSWY3DPEHPK3PXP", &legacy)
        .expect("segredo sob a chave legada");
    store.enable_totp(&user.id).expect("2fa ligado");

    migrate_admin_secrets_kdf(&mut store, &keys).expect("migration");

    // Depois do re-key o segredo tem que abrir com a chave NOVA — que e a
    // que o processo passa a usar a partir daqui.
    assert_eq!(
        store
            .get_totp_secret(&user.id, &keys.current)
            .expect("segredo legivel sob a chave nova")
            .as_deref(),
        Some("JBSWY3DPEHPK3PXP"),
        "o re-key tem que levar o segredo do 2FA junto, senao o dono fica \
         trancado fora do painel"
    );
    assert!(
        store.get_totp_secret(&user.id, &legacy).is_err(),
        "a chave legada nao pode mais abrir o segredo"
    );
}

/// Fail-closed tambem no lado do 2FA: um segredo TOTP que nao abre com a
/// chave legada aborta o re-key inteiro, sem gravar `kdf_params`. Gravar
/// parametros de uma chave que nao abre esse segredo transformaria uma
/// inconsistencia recuperavel (ainda da para rodar na legada) em perda
/// definitiva.
#[test]
fn a_totp_secret_that_does_not_decrypt_aborts_the_rekey() {
    let mut store = AdminStore::in_memory().expect("in-memory store");
    let keys = pending_keys(PASS);
    let user = store
        .create_user("dono", "senha", super::super::rbac::Role::Admin)
        .expect("usuario");

    let unrelated = vec![5u8; MASTER_KEY_LEN];
    store
        .set_pending_totp_secret(&user.id, "JBSWY3DPEHPK3PXP", &unrelated)
        .expect("segredo sob uma chave alheia");

    let err = migrate_admin_secrets_kdf(&mut store, &keys).expect_err("must refuse");
    assert!(err.contains("totp secret"), "{err}");
    assert!(
        store.kdf_params().expect("readable").is_none(),
        "params nao podem ser gravados quando o re-key falhou"
    );
    assert_eq!(
        store
            .get_totp_secret(&user.id, &unrelated)
            .expect("segredo intacto")
            .as_deref(),
        Some("JBSWY3DPEHPK3PXP"),
        "o ciphertext tem que ficar byte-identico"
    );
}

#[test]
fn migration_is_a_no_op_without_pending_params() {
    let mut store = AdminStore::in_memory().expect("in-memory store");
    let keys = AdminKeys {
        current: vec![7u8; MASTER_KEY_LEN],
        legacy: None,
        pending: None,
    };
    assert!(!keys.migration_pending());
    assert_eq!(migrate_admin_secrets_kdf(&mut store, &keys).unwrap(), 0);
    assert!(store.kdf_params().expect("readable").is_none());
}

#[test]
fn failed_migration_writes_nothing_at_all() {
    // Fail-closed: a ciphertext that does not open with the legacy key means
    // our assumption about this installation is wrong. Neither the rows nor
    // the params may be written — recording params for a key that opens
    // nothing is exactly the data-loss window this design removes.
    let mut store = AdminStore::in_memory().expect("in-memory store");
    let keys = pending_keys("passphrase");

    let unrelated = vec![3u8; MASTER_KEY_LEN];
    let (ciphertext, nonce) =
        super::super::secrets::encrypt_value(b"opaque", &unrelated).expect("encrypt");
    store
        .set_secret("default", "acme", "token", &ciphertext, &nonce, None)
        .expect("seed");

    let err = migrate_admin_secrets_kdf(&mut store, &keys).expect_err("must refuse");
    assert!(err.contains("does not decrypt"), "{err}");

    let (stored, stored_nonce) = store
        .get_secret_raw("default", "acme", "token")
        .expect("still present");
    assert_eq!(stored, ciphertext, "ciphertext must be byte-identical");
    assert_eq!(stored_nonce, nonce);
    assert!(
        store.kdf_params().expect("readable").is_none(),
        "params must not be recorded when the re-key failed"
    );
}

/// The one fallback that stays (#1992 keeps it on purpose): no parameters
/// recorded, and a ciphertext the legacy key does not open. The re-key is
/// deferred and the process runs on the legacy key — the key the data of a
/// never-re-keyed install is under — until a later boot can migrate.
#[test]
fn resolve_falls_back_to_the_legacy_key_when_the_migration_fails() {
    let mut store = AdminStore::in_memory().expect("in-memory store");
    let legacy = pbkdf2_key(LEGACY_KDF_SALT, LEGACY_KDF_ITERATIONS, PASS);

    let unrelated = vec![9u8; MASTER_KEY_LEN];
    let (ciphertext, nonce) =
        super::super::secrets::encrypt_value(b"x", &unrelated).expect("encrypt");
    store
        .set_secret("default", "acme", "token", &ciphertext, &nonce, None)
        .expect("seed");

    let key = resolve_with_passphrase(&mut store, PASS).expect("deferred, not refused");
    assert_eq!(key, legacy, "must hand back the key the data is under");
    assert!(
        store.kdf_params().expect("readable").is_none(),
        "a deferred re-key records nothing, so the next boot retries"
    );
}
