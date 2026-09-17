//! Store cifrado da sessao do WhatsApp vinculado.
//!
//! # O que esta em jogo
//!
//! O blob de sessao do Baileys **e** a conta: quem o tem fala como o usuario,
//! le o historico e nao precisa do telefone. No Hermes ele fica em claro, num
//! diretorio sem `chmod` (a doc pede que o usuario rode `chmod 700` a mao).
//! Aqui ele e cifrado em repouso com a mesma pilha do `CredentialVault`
//! (`ring`, AES-256-GCM, PBKDF2-HMAC-SHA256 600k) e os arquivos nascem 0600
//! dentro de um diretorio 0700.
//!
//! # As duas origens de chave, e por que existem duas
//!
//! | Origem | Quando | O que protege |
//! |---|---|---|
//! | [`KeyOrigin::VaultPassphrase`] | `GARRAIA_VAULT_PASSPHRASE` definida | Backup, disco roubado, snapshot de container: o blob nao abre sem a passphrase, que nunca toca o disco |
//! | [`KeyOrigin::RandomKeyFile`] | caso contrario | So o vizinho de mesma maquina com outro UID, e so enquanto o modo 0600 valer |
//!
//! A segunda e deliberadamente honesta sobre seu limite: uma chave ao lado do
//! ciphertext nao protege contra quem le o diretorio. Ela existe porque a
//! alternativa era o texto claro do Hermes, e porque exigir passphrase faria o
//! caminho feliz de `garra whatsapp` pedir um segredo que o usuario ainda nao
//! tem. `garra whatsapp status` **avisa** quando esta nesse modo — o aviso e
//! parte do contrato, nao enfeite.
//!
//! # Formato em disco
//!
//! ```text
//! <data_dir>/whatsapp/<conta>/          0700
//!   session.enc        0600  JSON {v, nonce, ciphertext} — AES-256-GCM
//!   session.enc.prev   0600  blob anterior, arquivado ate o novo link dar certo
//!   session.key        0600  chave aleatoria de 32 B (so no modo sem passphrase)
//!   session.salt       0600  salt do PBKDF2 (so no modo com passphrase)
//! ```
//!
//! O AAD e a string de versao do formato: um arquivo de outra versao nao abre
//! por acidente, ele falha a autenticacao.
//!
//! # Escrita atomica
//!
//! `tmp` no **mesmo diretorio** (rename cross-device falha), apertado para 0600
//! *antes* de receber conteudo, `sync_all` no arquivo, `rename`, e um `fsync`
//! best-effort no diretorio. O blob e reescrito a cada `session_update` durante
//! o pareamento; uma escrita interrompida no meio nao pode deixar meio arquivo
//! no lugar do anterior.

use std::fmt;
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64;
use garraia_common::fs_perms;
use ring::aead::{AES_256_GCM, Aad, LessSafeKey, NONCE_LEN, Nonce, UnboundKey};
use ring::pbkdf2;
use serde::{Deserialize, Serialize};
use zeroize::{Zeroize, Zeroizing};

/// Mesma contagem do `CredentialVault` (`garraia-security`). Nao ha motivo para
/// este material ser derivado mais barato que o do cofre.
const PBKDF2_ITERATIONS: u32 = 600_000;
const SALT_LEN: usize = 32;
const KEY_LEN: usize = 32;

/// AAD do AES-GCM. Liga o ciphertext ao formato: trocar a versao invalida o
/// arquivo antigo de forma detectavel, em vez de decifra-lo como lixo.
const AEAD_CONTEXT: &[u8] = b"garraia-whatsapp-linked-session-v1";

const BLOB_FILE: &str = "session.enc";
const ARCHIVE_FILE: &str = "session.enc.prev";
const KEY_FILE: &str = "session.key";
const SALT_FILE: &str = "session.salt";

/// Conta default. O caminho ja e parametrizado por conta para o dia em que
/// houver mais de um numero vinculado; hoje so existe `default`.
pub const DEFAULT_ACCOUNT: &str = "default";

/// Teto do identificador de conta. Nao ha razao para um segmento de diretorio
/// mais longo que isto, e um teto explicito evita depender do limite do
/// sistema de arquivos para recusar entrada absurda.
const MAX_ACCOUNT_LEN: usize = 64;

/// A conta e **um unico segmento** de caminho, e e esta funcao que garante.
///
/// Sem ela, [`SessionStore::for_data_dir`] aceitava qualquer string:
/// `join("../../..")` saia do data dir, e o store passava a cifrar — e a
/// apagar, no `purge` — arquivos escolhidos por quem controlasse a conta. O
/// unico chamador de hoje passa uma constante, entao o traversal estava
/// fechado **por acidente**; esta regra o fecha por construcao, para o dia em
/// que a fatia do gateway passar um valor vindo de config ou de request.
///
/// A regra e uma allowlist, nao uma lista de proibidos: so
/// `[A-Za-z0-9_-]{1,64}`. Isso recusa de uma vez `/`, `\`, `..`, `.`, a
/// string vazia, o NUL, o espaco e qualquer forma Unicode que o sistema de
/// arquivos possa dobrar em separador — sem precisar enumerar nenhuma delas.
fn validate_account(account: &str) -> Result<(), SessionError> {
    let ok = !account.is_empty()
        && account.len() <= MAX_ACCOUNT_LEN
        && account
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-');
    if ok {
        Ok(())
    } else {
        Err(SessionError::InvalidAccount(account.to_string()))
    }
}

/// Blob de sessao serializado pelo bridge (base64 de `{creds, keys}`).
///
/// `Debug` e `Display` imprimem `<redacted>`. Isto nao e cosmetico: o
/// `RedactingWriter` de `garraia-security` redige por **prefixo conhecido**
/// (`sk-`, `xoxb-`, …) e nao teria como reconhecer um base64 generico, entao a
/// unica defesa contra um `tracing::debug!(?evento)` e o tipo nunca saber se
/// imprimir.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct SessionBlob(String);

impl SessionBlob {
    pub fn new(raw: impl Into<String>) -> Self {
        Self(raw.into())
    }

    /// Valor cru. So o bridge e o store devem chamar isto.
    pub fn expose(&self) -> &str {
        &self.0
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }
}

impl fmt::Debug for SessionBlob {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("SessionBlob(<redacted>)")
    }
}

impl fmt::Display for SessionBlob {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("<redacted>")
    }
}

impl Drop for SessionBlob {
    fn drop(&mut self) {
        self.0.zeroize();
    }
}

/// De onde veio a chave de 32 B.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyOrigin {
    /// Derivada de `GARRAIA_VAULT_PASSPHRASE` via PBKDF2 com salt em disco.
    VaultPassphrase,
    /// Aleatoria, persistida em `session.key` ao lado do ciphertext.
    RandomKeyFile,
}

impl KeyOrigin {
    /// Aviso a exibir, ou `None` quando nao ha o que avisar.
    ///
    /// **F5 da auditoria R4**: o texto anterior nomeava a correcao
    /// (`defina GARRAIA_VAULT_PASSPHRASE`) sem nomear a **exposicao**, e quem
    /// nao sabe o que a passphrase protege nao tem como decidir. Agora diz o
    /// risco concreto — a chave fica no mesmo diretorio do arquivo cifrado,
    /// entao qualquer copia do diretorio leva a sessao junto.
    pub fn warning(self) -> Option<&'static str> {
        match self {
            KeyOrigin::VaultPassphrase => None,
            KeyOrigin::RandomKeyFile => Some(
                "a chave da sessao esta em session.key, no MESMO diretorio do arquivo \
cifrado: um backup do seu home, um snapshot do container ou um disco roubado \
levam a sessao junto. Defina GARRAIA_VAULT_PASSPHRASE para a chave deixar de \
tocar o disco.",
            ),
        }
    }

    /// Mesma advertencia em ingles, para a tela de consentimento.
    pub fn warning_en(self) -> Option<&'static str> {
        match self {
            KeyOrigin::VaultPassphrase => None,
            KeyOrigin::RandomKeyFile => Some(
                "the session key lives in session.key, in the SAME directory as the \
encrypted file: a backup of your home, a container snapshot or a stolen disk \
takes the session with it. Set GARRAIA_VAULT_PASSPHRASE so the key never \
touches disk.",
            ),
        }
    }
}

/// Chave simetrica de 32 B. Zerada no drop.
pub struct SessionKey {
    bytes: Zeroizing<[u8; KEY_LEN]>,
    origin: KeyOrigin,
}

impl fmt::Debug for SessionKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SessionKey")
            .field("origin", &self.origin)
            .field("bytes", &"<redacted>")
            .finish()
    }
}

impl SessionKey {
    pub fn origin(&self) -> KeyOrigin {
        self.origin
    }

    /// Resolve a chave do diretorio `dir`, criando o que faltar.
    ///
    /// `passphrase` e injetada pelo chamador (a CLI passa
    /// `garraia_security::vault_passphrase_from_env()`). Ler o ambiente aqui
    /// dentro tornaria o store impossivel de testar em paralelo e amarraria
    /// `garraia-channels` a uma politica que e da CLI.
    pub fn resolve(dir: &Path, passphrase: Option<&str>) -> Result<Self, SessionError> {
        fs_perms::create_secret_dir(dir).map_err(|e| SessionError::io(dir, e))?;

        match passphrase {
            Some(pass) if !pass.is_empty() => {
                let salt = load_or_create_salt(dir)?;
                let mut bytes = Zeroizing::new([0u8; KEY_LEN]);
                // `NonZeroU32::new(...).expect(...)` seria um panic em producao;
                // a constante e literal e diferente de zero, mas o tipo nao
                // sabe disso, entao tratamos como erro em vez de desdobrar.
                let iterations = std::num::NonZeroU32::new(PBKDF2_ITERATIONS)
                    .ok_or_else(|| SessionError::Crypto("iteracoes PBKDF2 invalidas".into()))?;
                pbkdf2::derive(
                    pbkdf2::PBKDF2_HMAC_SHA256,
                    iterations,
                    &salt,
                    pass.as_bytes(),
                    bytes.as_mut_slice(),
                );
                Ok(Self {
                    bytes,
                    origin: KeyOrigin::VaultPassphrase,
                })
            }
            _ => {
                let bytes = load_or_create_key_file(dir)?;
                Ok(Self {
                    bytes,
                    origin: KeyOrigin::RandomKeyFile,
                })
            }
        }
    }

    fn aead(&self) -> Result<LessSafeKey, SessionError> {
        let unbound = UnboundKey::new(&AES_256_GCM, self.bytes.as_slice())
            .map_err(|_| SessionError::Crypto("chave AES invalida".into()))?;
        Ok(LessSafeKey::new(unbound))
    }
}

/// Representacao em disco do blob cifrado.
#[derive(Serialize, Deserialize)]
struct EncryptedFile {
    v: u32,
    nonce: String,
    ciphertext: String,
}

/// Store de sessao de uma conta.
#[derive(Debug, Clone)]
pub struct SessionStore {
    dir: PathBuf,
}

impl SessionStore {
    /// `dir` e o diretorio da conta, tipicamente
    /// `<resolved_data_dir()>/whatsapp/default`.
    ///
    /// # Por que `pub(crate)`, e nao `pub`
    ///
    /// Porque ela nao valida nada — recebe o diretorio ja resolvido — e um
    /// **segundo construtor sem guarda torna a guarda do primeiro invisivel**:
    /// a supressao CodeQL 173 afirmava que a contencao no data dir "fecha por
    /// construcao, sem depender de call site", e enquanto `new` fosse `pub`
    /// isso era falso, porque qualquer crate podia montar o store com uma
    /// conta de runtime e o scan da CLI, que chaveia em `for_data_dir`, nao
    /// via nada.
    ///
    /// Estreitar a visibilidade nao custou call site nenhum: fora desta crate
    /// **ninguem** chamava `new` — a CLI, o smoke e os testes de integracao
    /// entram todos por [`SessionStore::for_data_dir`]. Ou seja, para quem
    /// esta de fora hoje existe um unico construtor, e ele valida. Quem esta
    /// dentro do modulo continua podendo montar um store sobre um `tempdir`.
    pub(crate) fn new(dir: impl Into<PathBuf>) -> Self {
        Self { dir: dir.into() }
    }

    /// Caminho canonico a partir do data dir da aplicacao — e, de fora desta
    /// crate, o **unico** jeito de construir um [`SessionStore`].
    ///
    /// Recusa qualquer `account` que nao seja um unico segmento
    /// `[A-Za-z0-9_-]{1,64}` — ver [`validate_account`]. E por isso que esta
    /// funcao devolve `Result` e a irma [`SessionStore::new`] nao: `new`
    /// recebe o diretorio ja resolvido de quem sabe o que esta fazendo, e por
    /// isso mesmo ela e `pub(crate)`.
    pub fn for_data_dir(data_dir: &Path, account: &str) -> Result<Self, SessionError> {
        validate_account(account)?;
        Ok(Self::new(data_dir.join("whatsapp").join(account)))
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    pub fn blob_path(&self) -> PathBuf {
        self.dir.join(BLOB_FILE)
    }

    pub fn archive_path(&self) -> PathBuf {
        self.dir.join(ARCHIVE_FILE)
    }

    pub fn key_path(&self) -> PathBuf {
        self.dir.join(KEY_FILE)
    }

    pub fn salt_path(&self) -> PathBuf {
        self.dir.join(SALT_FILE)
    }

    /// Ha um blob em disco? Nao diz nada sobre ele ser valido no servidor —
    /// so o bridge responde isso, e e por isso que existe a fase
    /// `validating` da maquina de estados.
    pub fn exists(&self) -> bool {
        self.blob_path().is_file()
    }

    /// Cifra e grava atomicamente.
    pub fn save(&self, blob: &SessionBlob, key: &SessionKey) -> Result<(), SessionError> {
        fs_perms::create_secret_dir(&self.dir).map_err(|e| SessionError::io(&self.dir, e))?;

        let nonce_bytes = random_bytes::<NONCE_LEN>()?;
        let aead = key.aead()?;
        let mut in_out = blob.expose().as_bytes().to_vec();
        aead.seal_in_place_append_tag(
            Nonce::assume_unique_for_key(*nonce_bytes),
            Aad::from(AEAD_CONTEXT),
            &mut in_out,
        )
        .map_err(|_| SessionError::Crypto("falha ao cifrar a sessao".into()))?;

        let file = EncryptedFile {
            v: 1,
            nonce: BASE64.encode(nonce_bytes),
            ciphertext: BASE64.encode(&in_out),
        };
        let json = serde_json::to_vec(&file)
            .map_err(|e| SessionError::Format(format!("falha ao serializar a sessao: {e}")))?;

        write_atomic(&self.blob_path(), &json)
    }

    /// Le e decifra. Erro de autenticacao vira [`SessionError::Undecryptable`],
    /// que o chamador trata como "precisa parear de novo" — e nunca como
    /// "arquivo corrompido, apaga tudo".
    pub fn load(&self, key: &SessionKey) -> Result<SessionBlob, SessionError> {
        let path = self.blob_path();
        let raw = std::fs::read(&path).map_err(|e| SessionError::io(&path, e))?;
        let file: EncryptedFile = serde_json::from_slice(&raw)
            .map_err(|e| SessionError::Format(format!("formato de sessao invalido: {e}")))?;
        if file.v != 1 {
            return Err(SessionError::Format(format!(
                "sessao gravada na versao {} — esta versao le apenas a 1",
                file.v
            )));
        }

        let nonce_bytes = BASE64
            .decode(&file.nonce)
            .map_err(|e| SessionError::Format(format!("nonce invalido: {e}")))?;
        let mut ciphertext = BASE64
            .decode(&file.ciphertext)
            .map_err(|e| SessionError::Format(format!("ciphertext invalido: {e}")))?;

        let nonce = Nonce::try_assume_unique_for_key(&nonce_bytes)
            .map_err(|_| SessionError::Format("nonce com tamanho errado".into()))?;
        let aead = key.aead()?;
        let plaintext = aead
            .open_in_place(nonce, Aad::from(AEAD_CONTEXT), &mut ciphertext)
            .map_err(|_| SessionError::Undecryptable)?;

        let text = std::str::from_utf8(plaintext)
            .map_err(|_| SessionError::Format("sessao decifrada nao e UTF-8".into()))?;
        let blob = SessionBlob::new(text);
        ciphertext.zeroize();
        Ok(blob)
    }

    /// Move o blob atual para `session.enc.prev`. Devolve `false` quando nao
    /// havia nada para arquivar.
    ///
    /// Arquivar em vez de apagar e a decisao 3 do ADR 0023: uma validacao que
    /// falha por rede instavel nao pode destruir a unica copia de uma sessao
    /// que talvez ainda sirva.
    pub fn archive(&self) -> Result<bool, SessionError> {
        let from = self.blob_path();
        if !from.is_file() {
            return Ok(false);
        }
        let to = self.archive_path();
        std::fs::rename(&from, &to).map_err(|e| SessionError::io(&from, e))?;
        // `rename` preserva o modo do arquivo de origem (0600); o aperto e
        // cinto e suspensorio para o caso de um `session.enc` legado frouxo.
        let _ = fs_perms::harden_secret_file(&to);
        Ok(true)
    }

    /// Inverso de [`SessionStore::archive`]: traz `session.enc.prev` de volta
    /// para `session.enc`. Devolve `false` quando nao havia nada a restaurar.
    ///
    /// # Por que ela precisa existir
    ///
    /// O docstring de `archive` diz que arquivar existe para que "uma
    /// validacao que falha por rede instavel nao possa destruir a unica copia
    /// de uma sessao que talvez ainda sirva" — e ate agora **nada** cumpria
    /// essa promessa: o `.prev` so era lido por `discard_archive` (shred) e
    /// por `purge` (shred). Um Ctrl+C na tela do QR, ou cinco QRs expirando,
    /// deixavam o usuario sem sessao viva e com a boa num arquivo que nenhum
    /// caminho de codigo reabria.
    ///
    /// # A guarda do blob novo
    ///
    /// Restaurar **nunca** sobrescreve um `session.enc` existente: se ha blob
    /// novo em disco, o link seguiu em frente e o arquivado ja nao vale. Hoje
    /// isso nao chega a ser uma corrida — o `pair` segura o blob em memoria e
    /// so grava depois de conectar, entao nos desfechos cancelado/QR expirado
    /// nao existe blob parcial nenhum —, mas a guarda fica porque a ordem de
    /// gravacao do `pair` e do `pair`, e nao um contrato deste store.
    pub fn restore_archive(&self) -> Result<bool, SessionError> {
        let from = self.archive_path();
        if !from.is_file() {
            return Ok(false);
        }
        let to = self.blob_path();
        if to.exists() {
            // Ha sessao viva: o arquivado perdeu a vez. Nao e erro — e o
            // caminho feliz, em que `discard_archive` ja passou ou vai passar.
            return Ok(false);
        }
        std::fs::rename(&from, &to).map_err(|e| SessionError::io(&from, e))?;
        // Mesmo motivo do gemeo em `archive`: o `rename` preserva o modo da
        // origem, e este aperto e para o caso de um `.prev` que chegou frouxo
        // (restaurado de backup, copiado com `cp`).
        let _ = fs_perms::harden_secret_file(&to);
        Ok(true)
    }

    /// Apaga o arquivo `session.enc.prev`, se houver.
    ///
    /// A sessao anterior fica arquivada **ate o novo link dar certo** (decisao
    /// 3 do ADR 0023). "Dar certo" e ter um blob novo em disco; a partir dai o
    /// arquivado e so material de autenticacao vivo esquecido num arquivo que
    /// ninguem mais vai abrir — exatamente o que nao se quer deixar para tras.
    pub fn discard_archive(&self) -> Result<(), SessionError> {
        shred(&self.archive_path())
    }

    /// Descarte da sessao que o SERVIDOR recusou, no meio de um pareamento.
    ///
    /// # Por que nao e o [`SessionStore::purge`]
    ///
    /// Porque `purge` tritura tambem o `session.enc.prev`, e num re-vinculo o
    /// `.prev` e a sessao BOA do usuario — foi o `link` quem a arquivou,
    /// segundos antes, justamente para poder devolve-la. A sequencia real era:
    /// `archive()` → pareamento novo → a Meta recusa (401/403/419) →
    /// `purge()` → o guard tenta restaurar e ja nao ha o que restaurar. O
    /// vinculo anterior sumia, e sumia em silencio.
    ///
    /// # A regra, e o invariante que ela protege
    ///
    /// O blob vivo vai embora sempre: foi ele que o servidor recusou. A chave
    /// e o salt so vao junto quando **nao ha arquivado** — porque `.prev`,
    /// `session.key` e `session.salt` vivem ou morrem juntos. Um `.prev` sem a
    /// chave que o decifra nao recupera nada e seria pior do que nao ter
    /// guardado: daria a impressao de recuperacao.
    ///
    /// Isto e uma regra do store, e nao uma decisao de quem chama, de
    /// proposito: `pair` e API publica desta crate, e um call site novo que
    /// esquecesse de limpar deixaria blob morto em disco. Aqui nenhum pode
    /// esquecer, e nenhum pode triturar o que nao foi ele quem criou.
    ///
    /// Devolve `true` quando havia arquivado e ele foi PRESERVADO — e o que o
    /// chamador precisa saber para dizer ao usuario que a sessao anterior
    /// ainda da para recuperar.
    pub fn discard_dead_session(&self) -> Result<bool, SessionError> {
        shred(&self.blob_path())?;
        if self.archive_path().is_file() {
            return Ok(true);
        }
        shred(&self.key_path())?;
        shred(&self.salt_path())?;
        Ok(false)
    }

    /// Logout: sobrescreve e remove blob, arquivo e chave.
    ///
    /// A sobrescrita e best-effort e esta documentada como tal: em SSD com
    /// wear leveling e em qualquer FS com copy-on-write ela nao garante que os
    /// bytes antigos sumiram do meio fisico. O que ela garante e que o arquivo
    /// que ficou no cache de pagina e no backup incremental de hoje nao carrega
    /// mais o material.
    pub fn purge(&self) -> Result<(), SessionError> {
        for path in [self.blob_path(), self.archive_path(), self.key_path()] {
            shred(&path)?;
        }
        // O salt nao e segredo (ele so existe para o PBKDF2 nao ser
        // pre-computavel), mas deixa-lo para tras faria a proxima sessao
        // reutilizar a derivacao de uma conta que ja foi embora.
        shred(&self.salt_path())?;
        Ok(())
    }
}

fn shred(path: &Path) -> Result<(), SessionError> {
    let Ok(meta) = std::fs::metadata(path) else {
        return Ok(()); // nao existe: nada a fazer
    };
    if meta.is_file() {
        if let Ok(mut f) = OpenOptions::new().write(true).open(path) {
            let zeros = vec![0u8; meta.len() as usize];
            let _ = f.write_all(&zeros);
            let _ = f.sync_all();
        }
        std::fs::remove_file(path).map_err(|e| SessionError::io(path, e))?;
    }
    Ok(())
}

fn load_or_create_salt(dir: &Path) -> Result<Vec<u8>, SessionError> {
    let path = dir.join(SALT_FILE);
    if path.is_file() {
        let salt = std::fs::read(&path).map_err(|e| SessionError::io(&path, e))?;
        if salt.len() == SALT_LEN {
            return Ok(salt);
        }
        return Err(SessionError::Format(format!(
            "salt em {} tem {} bytes, esperado {SALT_LEN}",
            path.display(),
            salt.len()
        )));
    }
    let salt = random_bytes::<SALT_LEN>()?;
    write_atomic(&path, &salt[..])?;
    Ok(salt.to_vec())
}

fn load_or_create_key_file(dir: &Path) -> Result<Zeroizing<[u8; KEY_LEN]>, SessionError> {
    let path = dir.join(KEY_FILE);
    if path.is_file() {
        // `Zeroizing` desde a leitura: este `Vec` **e** a chave. Sem ele, o
        // buffer do `fs::read` ficava no heap depois do `copy_from_slice`,
        // desfazendo boa parte do cuidado que o `Zeroizing` do destino tem.
        let raw = Zeroizing::new(std::fs::read(&path).map_err(|e| SessionError::io(&path, e))?);
        let mut bytes = Zeroizing::new([0u8; KEY_LEN]);
        if raw.len() != KEY_LEN {
            return Err(SessionError::Format(format!(
                "chave em {} tem {} bytes, esperado {KEY_LEN}",
                path.display(),
                raw.len()
            )));
        }
        bytes.copy_from_slice(&raw);
        // Uma chave que chegou frouxa (restaurada de backup, copiada com `cp`)
        // e apertada agora: o store nunca deixa um segredo em 0644 depois de
        // te-lo tocado.
        let _ = fs_perms::harden_secret_file(&path);
        return Ok(bytes);
    }
    let fresh = random_bytes::<KEY_LEN>()?;
    write_atomic(&path, &fresh[..])?;
    let mut bytes = Zeroizing::new([0u8; KEY_LEN]);
    bytes.copy_from_slice(&fresh[..]);
    Ok(bytes)
}

/// Bytes do RNG do sistema, ja embrulhados para zerar na queda.
///
/// `Zeroizing` e nao `[u8; N]` cru porque um dos usos e a **chave** de
/// `session.key`: a versao anterior devolvia o array por valor, o chamador o
/// copiava para dentro de um `Zeroizing` e o original ficava na pilha ate ser
/// sobrescrito por acaso. O nonce e o salt nao sao segredo e nao precisavam
/// disto — mas o tipo certo na fonte e o que impede o proximo uso de tamanho de
/// chave de esquecer.
fn random_bytes<const N: usize>() -> Result<Zeroizing<[u8; N]>, SessionError> {
    use rand::TryRngCore;
    let mut buf = Zeroizing::new([0u8; N]);
    rand::rngs::OsRng
        .try_fill_bytes(buf.as_mut())
        .map_err(|e| SessionError::Crypto(format!("RNG do sistema indisponivel: {e}")))?;
    Ok(buf)
}

/// O temporario do [`write_atomic`]: criado do zero, 0600 desde o `open`,
/// escrito e sincronizado.
fn write_tmp_file(tmp: &Path, bytes: &[u8]) -> Result<(), SessionError> {
    let mut opts = OpenOptions::new();
    // `create_new`: se o caminho ja existe — arquivo ou symlink — isto falha
    // em vez de escrever por cima. Ver o docstring de [`write_atomic`].
    opts.write(true).create_new(true);
    // Nasce 0600: o `chmod` depois da escrita deixaria uma janela em que o
    // segredo ja esta no disco com o modo do umask.
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(fs_perms::SECRET_FILE_MODE);
    }
    let mut f = opts.open(tmp).map_err(|e| SessionError::io(tmp, e))?;
    f.write_all(bytes).map_err(|e| SessionError::io(tmp, e))?;
    f.sync_all().map_err(|e| SessionError::io(tmp, e))?;
    drop(f);
    // Fora de Unix o `mode` acima nao existe; aperta pelo caminho generico.
    let _ = fs_perms::harden_secret_file(tmp);
    Ok(())
}

/// tmp no mesmo diretorio, ja 0600, fsync, rename, fsync do diretorio.
///
/// # Por que o nome do temporario e aleatorio, e por que `create_new`
///
/// O nome era deterministico (`.session.enc.tmp`) e o arquivo era aberto com
/// `create(true).truncate(true)`. Juntas, as duas coisas desmentiam o "nasce
/// 0600" do comentario abaixo: `mode()` so vale na **criacao**, entao sobre um
/// `.tmp` preexistente o modo antigo ficava, e o segredo passava a existir em
/// disco frouxo ate o `harden_secret_file`, que so roda depois do `write_all`.
///
/// E com nome previsivel o preexistente nao precisa ser um arquivo: um symlink
/// plantado por outro usuario no mesmo diretorio seria SEGUIDO por
/// `create(true)`, e o alvo dele e que receberia a escrita e o `chmod`.
/// `create_new(true)` recusa abrir qualquer coisa que ja exista — symlink
/// inclusive — e o sufixo aleatorio faz o caminho nao ser adivinhavel. As duas
/// juntas, porque cada uma sozinha ainda deixa metade do problema.
fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), SessionError> {
    let dir = path
        .parent()
        .ok_or_else(|| SessionError::Format(format!("{} nao tem diretorio pai", path.display())))?;
    fs_perms::create_secret_dir(dir).map_err(|e| SessionError::io(dir, e))?;

    let nonce = u64::from_ne_bytes(*random_bytes::<8>()?);
    let tmp = dir.join(format!(
        ".{}.{nonce:016x}.tmp",
        path.file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "session".into())
    ));

    // Qualquer falha daqui para baixo deixaria material de sessao num
    // temporario orfao com nome que ninguem mais conhece — o pior dos dois
    // mundos. Por isso o desfecho de erro tritura o `tmp` antes de propagar.
    let written = write_tmp_file(&tmp, bytes)
        .and_then(|()| std::fs::rename(&tmp, path).map_err(|e| SessionError::io(path, e)));
    if let Err(e) = written {
        let _ = shred(&tmp);
        return Err(e);
    }

    // Best-effort: sem isto o rename pode nao estar durável depois de uma queda
    // de energia. Falha aqui nao invalida a escrita, entao nao vira erro.
    if let Ok(d) = File::open(dir) {
        let _ = d.sync_all();
    }
    Ok(())
}

/// Falhas do store.
#[derive(Debug, thiserror::Error)]
pub enum SessionError {
    #[error("{path}: {source}")]
    Io {
        path: String,
        #[source]
        source: std::io::Error,
    },
    #[error("sessao ilegivel: {0}")]
    Format(String),
    /// A chave nao abre este blob: passphrase trocada, `session.key` perdida ou
    /// arquivo de outra instalacao.
    #[error("a sessao existe mas nao abre com a chave atual")]
    Undecryptable,
    /// A conta nao e um segmento de caminho aceitavel. O valor recusado entra
    /// na mensagem de proposito: ele e um identificador de conta escolhido
    /// pelo operador, nao segredo, e sem ele o erro nao orienta ninguem.
    #[error("conta invalida: {0:?} — use apenas [A-Za-z0-9_-], ate 64 caracteres")]
    InvalidAccount(String),
    #[error("erro criptografico: {0}")]
    Crypto(String),
}

impl SessionError {
    fn io(path: &Path, source: std::io::Error) -> Self {
        Self::Io {
            path: path.display().to_string(),
            source,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn blob() -> SessionBlob {
        SessionBlob::new("eyJjcmVkcyI6eyJtZSI6IjU1MTE5OTk5OTg4ODgifX0=")
    }

    #[test]
    fn round_trip_with_a_random_key_file() {
        let dir = tempdir().expect("tempdir");
        let store = SessionStore::new(dir.path().join("whatsapp/default"));
        let key = SessionKey::resolve(store.dir(), None).expect("key");
        assert_eq!(key.origin(), KeyOrigin::RandomKeyFile);

        store.save(&blob(), &key).expect("save");
        assert!(store.exists());

        let key2 = SessionKey::resolve(store.dir(), None).expect("reopen key");
        let back = store.load(&key2).expect("load");
        assert_eq!(back.expose(), blob().expose());
    }

    #[test]
    fn round_trip_with_a_vault_passphrase_leaves_no_key_file() {
        let dir = tempdir().expect("tempdir");
        let store = SessionStore::new(dir.path().join("whatsapp/default"));
        let key = SessionKey::resolve(store.dir(), Some("correct horse")).expect("key");
        assert_eq!(key.origin(), KeyOrigin::VaultPassphrase);

        store.save(&blob(), &key).expect("save");

        assert!(!store.key_path().exists(), "com passphrase nao ha key file");
        assert!(store.salt_path().is_file(), "o salt precisa persistir");

        let key2 = SessionKey::resolve(store.dir(), Some("correct horse")).expect("key again");
        assert_eq!(store.load(&key2).expect("load").expose(), blob().expose());
    }

    #[test]
    fn a_wrong_passphrase_does_not_decrypt() {
        let dir = tempdir().expect("tempdir");
        let store = SessionStore::new(dir.path());
        let key = SessionKey::resolve(store.dir(), Some("right")).expect("key");
        store.save(&blob(), &key).expect("save");

        let wrong = SessionKey::resolve(store.dir(), Some("wrong")).expect("key");
        assert!(matches!(
            store.load(&wrong),
            Err(SessionError::Undecryptable)
        ));
    }

    #[test]
    fn tampering_with_the_ciphertext_is_detected() {
        let dir = tempdir().expect("tempdir");
        let store = SessionStore::new(dir.path());
        let key = SessionKey::resolve(store.dir(), None).expect("key");
        store.save(&blob(), &key).expect("save");

        let raw = std::fs::read_to_string(store.blob_path()).expect("read");
        let mut file: serde_json::Value = serde_json::from_str(&raw).expect("json");
        let ct = file["ciphertext"].as_str().expect("ct").to_string();
        let mut bytes = BASE64.decode(&ct).expect("b64");
        bytes[0] ^= 0xff;
        file["ciphertext"] = serde_json::Value::String(BASE64.encode(&bytes));
        std::fs::write(store.blob_path(), file.to_string()).expect("write");

        assert!(matches!(store.load(&key), Err(SessionError::Undecryptable)));
    }

    #[test]
    fn archive_moves_the_blob_and_is_a_noop_when_empty() {
        let dir = tempdir().expect("tempdir");
        let store = SessionStore::new(dir.path());
        assert!(!store.archive().expect("archive empty"));

        let key = SessionKey::resolve(store.dir(), None).expect("key");
        store.save(&blob(), &key).expect("save");
        assert!(store.archive().expect("archive"));
        assert!(!store.blob_path().exists());
        assert!(store.archive_path().is_file());
    }

    #[test]
    fn purge_removes_every_piece_of_material() {
        let dir = tempdir().expect("tempdir");
        let store = SessionStore::new(dir.path());
        let key = SessionKey::resolve(store.dir(), None).expect("key");
        store.save(&blob(), &key).expect("save");
        store.archive().expect("archive");
        store.save(&blob(), &key).expect("save again");

        store.purge().expect("purge");

        for path in [
            store.blob_path(),
            store.archive_path(),
            store.key_path(),
            store.salt_path(),
        ] {
            assert!(!path.exists(), "{} sobreviveu ao purge", path.display());
        }
        assert!(!store.exists());
    }

    #[test]
    fn discarding_the_archive_leaves_the_live_session_alone() {
        let dir = tempdir().expect("tempdir");
        let store = SessionStore::new(dir.path());
        let key = SessionKey::resolve(store.dir(), None).expect("key");
        store.save(&blob(), &key).expect("save");
        store.archive().expect("archive");
        store.save(&blob(), &key).expect("novo link");

        store.discard_archive().expect("discard");

        assert!(!store.archive_path().exists(), "o arquivado some");
        assert!(store.exists(), "a sessao viva fica");
        // Idempotente: descartar duas vezes nao e erro.
        store.discard_archive().expect("discard de novo");
    }

    #[test]
    fn a_dead_session_takes_the_key_with_it_when_there_is_nothing_archived() {
        let dir = tempfile::tempdir().expect("tempdir");
        let store = SessionStore::new(dir.path().join("wa"));
        let key = SessionKey::resolve(store.dir(), None).expect("key");
        store.save(&blob(), &key).expect("save");

        let preservou = store.discard_dead_session().expect("discard");

        assert!(!preservou, "nao havia arquivado a preservar");
        assert!(!store.exists(), "o blob que o servidor recusou vai embora");
        assert!(!store.key_path().exists(), "e a chave vai junto");
        assert!(!store.salt_path().exists(), "o salt tambem");
    }

    /// O invariante do F2: com arquivado em disco, `.prev`, `session.key` e
    /// `session.salt` sobrevivem juntos. Preservar o `.prev` sem a chave seria
    /// pior que nao preservar — daria a impressao de recuperacao.
    #[test]
    fn a_dead_session_never_takes_the_archived_one_nor_its_key() {
        let dir = tempfile::tempdir().expect("tempdir");
        let store = SessionStore::new(dir.path().join("wa"));
        let key = SessionKey::resolve(store.dir(), None).expect("key");
        store.save(&blob(), &key).expect("save");
        assert!(store.archive().expect("archive"));
        // Um blob novo em disco, como o de um pareamento que o servidor
        // acabou de recusar.
        store
            .save(&SessionBlob::new("eyJub3ZvIjoxfQ=="), &key)
            .expect("save novo");

        let preservou = store.discard_dead_session().expect("discard");

        assert!(preservou, "havia arquivado, e ele foi preservado");
        assert!(!store.exists(), "o blob recusado some");
        assert!(store.archive_path().is_file(), "o arquivado fica");
        assert!(store.key_path().exists(), "e a chave que o abre tambem");
        assert!(store.restore_archive().expect("restore"));
        assert_eq!(
            store.load(&key).expect("abre").expose(),
            blob().expose(),
            "o que volta e a sessao anterior, legivel"
        );
    }

    #[test]
    fn purge_on_an_empty_store_succeeds() {
        let dir = tempdir().expect("tempdir");
        SessionStore::new(dir.path().join("never-used"))
            .purge()
            .expect("purge vazio nao e erro");
    }

    /// O store **aperta** o que encontra frouxo: cria-se o diretorio e a chave
    /// em 0777/0666 de proposito e prova-se que depois de uma passagem do
    /// codigo os modos sao 0700/0600.
    #[cfg(unix)]
    #[test]
    fn loose_modes_are_tightened_by_the_store() {
        use std::os::unix::fs::PermissionsExt;

        let dir = tempdir().expect("tempdir");
        let account = dir.path().join("whatsapp").join("default");
        std::fs::create_dir_all(&account).expect("mkdir");
        std::fs::set_permissions(&account, std::fs::Permissions::from_mode(0o777))
            .expect("loosen dir");
        std::fs::write(account.join(KEY_FILE), [7u8; KEY_LEN]).expect("write key");
        std::fs::set_permissions(
            account.join(KEY_FILE),
            std::fs::Permissions::from_mode(0o666),
        )
        .expect("loosen key");

        let store = SessionStore::new(&account);
        let key = SessionKey::resolve(store.dir(), None).expect("key");
        store.save(&blob(), &key).expect("save");
        store.archive().expect("archive");
        store.save(&blob(), &key).expect("save again");

        let mode = |p: PathBuf| {
            std::fs::metadata(&p)
                .unwrap_or_else(|e| panic!("stat {}: {e}", p.display()))
                .permissions()
                .mode()
                & 0o777
        };
        assert_eq!(mode(account.clone()), 0o700, "diretorio da conta");
        assert_eq!(mode(store.blob_path()), 0o600, "session.enc");
        assert_eq!(mode(store.archive_path()), 0o600, "session.enc.prev");
        assert_eq!(mode(store.key_path()), 0o600, "session.key");
    }

    #[test]
    fn blob_debug_and_display_are_redacted() {
        let b = blob();
        assert_eq!(format!("{b:?}"), "SessionBlob(<redacted>)");
        assert_eq!(format!("{b}"), "<redacted>");
        assert!(!format!("{b:?}{b}").contains("eyJjcmVkcyI"));
    }

    #[test]
    fn session_key_debug_is_redacted_but_keeps_the_origin() {
        let dir = tempdir().expect("tempdir");
        let key = SessionKey::resolve(dir.path(), Some("p")).expect("key");
        let shown = format!("{key:?}");
        assert!(shown.contains("VaultPassphrase"));
        assert!(shown.contains("<redacted>"));
    }

    #[test]
    fn only_the_keyless_mode_warns() {
        assert!(KeyOrigin::VaultPassphrase.warning().is_none());
        assert!(
            KeyOrigin::RandomKeyFile
                .warning()
                .is_some_and(|w| w.contains("GARRAIA_VAULT_PASSPHRASE"))
        );
    }

    #[test]
    fn for_data_dir_uses_the_documented_layout() {
        let store =
            SessionStore::for_data_dir(Path::new("/data"), DEFAULT_ACCOUNT).expect("conta valida");
        assert!(store.blob_path().ends_with("whatsapp/default/session.enc"));
    }

    /// Contas que um dia podem existir de verdade (mais de um numero
    /// vinculado) continuam passando: a regra fecha o traversal, nao o
    /// recurso.
    #[test]
    fn plausible_account_names_are_accepted() {
        for account in [
            "default",
            "pessoal",
            "conta_2",
            "linha-b",
            "A1",
            &"x".repeat(64),
        ] {
            let store = SessionStore::for_data_dir(Path::new("/data"), account)
                .unwrap_or_else(|e| panic!("conta {account:?} deveria valer: {e}"));
            assert!(store.dir().ends_with(account));
        }
    }

    /// **Este e o teste do bloqueador.** Cada conta abaixo e uma forma de sair
    /// do data dir ou de nomear algo que nao e um segmento; o `for_data_dir`
    /// tem de recusar todas.
    ///
    /// E ele vai ate o disco de proposito: no dia em que alguem apagar o
    /// `validate_account`, o `Ok` cai no braco que **de fato grava** e o
    /// teste reporta o caminho real que a sessao cifrada acabou de ocupar
    /// fora do data dir. Uma asserção so sobre o `Result` provaria bem menos.
    #[test]
    fn an_account_that_escapes_the_data_dir_is_refused_before_any_write() {
        let tmp = tempdir().expect("tempdir");
        let data = tmp.path().join("data");

        // A ordem importa para o diagnostico: a primeira conta da lista e uma
        // que de fato SAI do data dir, entao, com a validacao arrancada, quem
        // falha e a asserção de contencao — e a mensagem mostra o caminho real
        // que a sessao cifrada acabou de ocupar la fora.
        for account in [
            "../../fuga",
            "..",
            "../..",
            "sub/dir",
            "sub\\dir",
            "/absoluta",
            ".",
            "",
            "conta com espaco",
            "acentuada\u{e7}",
        ] {
            match SessionStore::for_data_dir(&data, account) {
                Err(SessionError::InvalidAccount(recusada)) => assert_eq!(recusada, account),
                Err(outro) => panic!("conta {account:?} recusada pelo motivo errado: {outro}"),
                Ok(store) => {
                    // Sem a validacao, este e o caminho que o codigo antigo
                    // tomava. Grava-se de verdade para mostrar onde para.
                    let key = SessionKey::resolve(store.dir(), None).expect("chave");
                    store.save(&blob(), &key).expect("save");
                    let escrito = store.blob_path().canonicalize().expect("canonicalize");
                    let raiz = data.canonicalize().expect("canonicalize data dir");
                    assert!(
                        escrito.starts_with(&raiz),
                        "a conta {account:?} escapou do data dir: {} nao esta sob {}",
                        escrito.display(),
                        raiz.display()
                    );
                    panic!("a conta {account:?} deveria ter sido recusada");
                }
            }
        }
    }

    #[test]
    fn restoring_the_archive_brings_the_previous_session_back_readable() {
        let dir = tempdir().expect("tempdir");
        let store = SessionStore::new(dir.path());
        let key = SessionKey::resolve(store.dir(), None).expect("key");
        store.save(&blob(), &key).expect("save");

        assert!(store.archive().expect("archive"));
        assert!(!store.exists(), "arquivar tira a sessao de cena");

        assert!(store.restore_archive().expect("restore"));
        assert!(store.exists(), "restaurar traz a sessao de volta");
        assert!(!store.archive_path().exists(), "o arquivado saiu de la");
        // Nao basta o arquivo existir: ele tem de continuar abrindo.
        assert_eq!(store.load(&key).expect("load").expose(), blob().expose());
    }

    #[test]
    fn restoring_is_a_noop_without_an_archive_and_never_clobbers_a_live_session() {
        let dir = tempdir().expect("tempdir");
        let store = SessionStore::new(dir.path());
        let key = SessionKey::resolve(store.dir(), None).expect("key");

        assert!(!store.restore_archive().expect("sem arquivado"));

        store.save(&blob(), &key).expect("save");
        store.archive().expect("archive");
        let novo = SessionBlob::new("bm92bw==");
        store.save(&novo, &key).expect("link novo concluiu");

        assert!(
            !store.restore_archive().expect("restore"),
            "com sessao viva em disco o arquivado perdeu a vez"
        );
        assert_eq!(
            store.load(&key).expect("load").expose(),
            novo.expose(),
            "a sessao viva nao pode ser sobrescrita pelo arquivado"
        );
        assert!(store.archive_path().is_file(), "o arquivado fica onde esta");
    }

    /// O temporario do `write_atomic` nao reaproveita um arquivo que ja
    /// estivesse no caminho previsivel de antes.
    ///
    /// Com o nome deterministico (`.session.key.tmp`) e
    /// `create(true).truncate(true)`, o store abria o preexistente e o
    /// renomeava por cima do destino: o `mode(0600)` do `OpenOptions` so vale
    /// na CRIACAO, entao o segredo passava a existir em disco com o modo do
    /// arquivo alheio ate o `harden_secret_file` de depois do `write_all`.
    #[test]
    fn a_planted_temporary_file_is_never_reused() {
        let dir = tempdir().expect("tempdir");
        let wa = dir.path().join("wa");
        std::fs::create_dir_all(&wa).expect("mkdir");
        let plantado = wa.join(".session.key.tmp");
        std::fs::write(&plantado, b"nao sou do store").expect("plantar");

        let store = SessionStore::new(&wa);
        let key = SessionKey::resolve(store.dir(), None).expect("key");
        store.save(&blob(), &key).expect("save");

        assert!(
            store.key_path().is_file(),
            "a chave foi escrita assim mesmo"
        );
        assert_eq!(
            std::fs::read(&plantado).expect("o plantado continua la"),
            b"nao sou do store",
            "o store nao pode ter reaproveitado um temporario que nao e dele"
        );
    }

    /// E nao SEGUE um symlink plantado nesse caminho.
    ///
    /// Era o desfecho pior do nome previsivel: `create(true)` segue o link, e
    /// quem levava a escrita (e o `chmod` 0600) era o alvo, um arquivo de
    /// outra pessoa. `create_new(true)` recusa abrir o que ja existe — symlink
    /// inclusive — e o sufixo aleatorio tira o alvo da mira.
    #[cfg(unix)]
    #[test]
    fn a_planted_symlink_is_never_followed() {
        let dir = tempdir().expect("tempdir");
        let wa = dir.path().join("wa");
        std::fs::create_dir_all(&wa).expect("mkdir");
        let vitima = dir.path().join("arquivo-da-vitima");
        std::fs::write(&vitima, b"conteudo da vitima").expect("vitima");
        std::os::unix::fs::symlink(&vitima, wa.join(".session.key.tmp")).expect("symlink");

        let store = SessionStore::new(&wa);
        let key = SessionKey::resolve(store.dir(), None).expect("key");
        store.save(&blob(), &key).expect("save");

        assert_eq!(
            std::fs::read(&vitima).expect("a vitima continua la"),
            b"conteudo da vitima",
            "um symlink com nome previsivel nao pode redirecionar a escrita do store"
        );
    }

    #[test]
    fn no_temporary_file_survives_a_successful_write() {
        let dir = tempdir().expect("tempdir");
        let store = SessionStore::new(dir.path());
        let key = SessionKey::resolve(store.dir(), None).expect("key");
        store.save(&blob(), &key).expect("save");

        let leftovers: Vec<_> = std::fs::read_dir(dir.path())
            .expect("readdir")
            .filter_map(|e| e.ok())
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .filter(|n| n.ends_with(".tmp"))
            .collect();
        assert!(leftovers.is_empty(), "sobrou temporario: {leftovers:?}");
    }
}
