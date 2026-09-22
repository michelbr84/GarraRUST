//! W1 da v0.4.5: o boot do canal re-materializa a ponte embutida.
//!
//! O defeito: depois de um `garraia update`, o gateway seguia lancando o
//! `bridge.mjs` que o ultimo `whatsapp link` tinha deixado no disco — o da
//! versao anterior, sem o conserto do `@lid` da 0.4.5 — porque nada no
//! gateway materializava os assets. Estes testes sobem o supervisor pela
//! fiacao do boot ([`supervisionar`]), com a ponte falsa no lugar do `node` e
//! o `npm` falso (`fake_npm.py`) no lugar do `npm`, e olham o disco.

use super::*;
use garraia_channels::whatsapp_linked::bridge::{
    Asset, BridgeAssets, EmbeddedAssets, assets_digest, deps_installed, install_deps, prepare,
};
use std::path::Path;

/// O `npm` falso da suite de `garraia-channels`. Sem `.fake-npm-ok` no
/// diretorio da ponte ele falha; com, imita um `npm ci` que da certo. Cada
/// chamada vira uma linha em `.fake-npm-calls`.
fn fake_npm() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("crates/")
        .join("garraia-channels/tests/fixtures/fake_npm.py")
}

fn npm_calls(dir: &Path) -> usize {
    std::fs::read_to_string(dir.join(".fake-npm-calls"))
        .map(|s| s.lines().count())
        .unwrap_or(0)
}

fn embutido(nome: &str) -> &'static str {
    EmbeddedAssets
        .files()
        .iter()
        .find(|a| a.name == nome)
        .map(|a: &Asset| a.contents)
        .expect("asset embutido")
}

fn bridge_dir(state: &SharedState) -> PathBuf {
    LinkedPaths::from_config(&state.config)
        .expect("DEFAULT_ACCOUNT e valido")
        .bridge_dir
}

/// O que um `whatsapp link` bem-sucedido desta versao deixa no disco: os
/// assets embutidos, `node_modules` e o carimbo de dependencias — pelo mesmo
/// caminho da CLI (`prepare` + `install_deps`). Devolve o preparo que o boot
/// usaria, com o `npm` falso.
pub(super) async fn ponte_ja_pronta(state: &SharedState) -> PreparoDaPonte {
    let dir = bridge_dir(state);
    prepare(&dir, &EmbeddedAssets).expect("materializa");
    std::fs::write(dir.join(".fake-npm-ok"), "").expect("npm falso em modo ok");
    install_deps(&fake_npm(), &dir, &EmbeddedAssets)
        .await
        .expect("npm falso instala");
    PreparoDaPonte {
        dir,
        assets: Arc::new(EmbeddedAssets),
        npm: Some(fake_npm()),
    }
}

/// Poe o mtime no passado: "nao foi reescrito" precisa ser observavel, e uma
/// reescrita no mesmo milissegundo (ou pelo mesmo inode) nao muda nada.
fn envelhece(path: &Path) -> std::time::SystemTime {
    let antigo = std::time::SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1_700_000_000);
    std::fs::File::options()
        .write(true)
        .open(path)
        .expect("abre")
        .set_modified(antigo)
        .expect("set_modified");
    antigo
}

fn mtime(path: &Path) -> std::time::SystemTime {
    std::fs::metadata(path)
        .expect("stat")
        .modified()
        .expect("mtime")
}

/// Todo arquivo do diretorio da sessao, por nome. E o que "a sessao nao foi
/// tocada" quer dizer.
fn retrato_da_sessao(store: &SessionStore) -> Vec<(String, Vec<u8>)> {
    let mut arquivos: Vec<(String, Vec<u8>)> = std::fs::read_dir(store.dir())
        .expect("dir da sessao")
        .filter_map(Result::ok)
        .filter(|e| e.path().is_file())
        .map(|e| {
            (
                e.file_name().to_string_lossy().into_owned(),
                std::fs::read(e.path()).expect("le"),
            )
        })
        .collect();
    arquivos.sort();
    arquivos
}

/// A ponte falsa, contando quantas vezes foi lancada e quantos `npm` ja
/// tinham rodado no momento de cada lancamento.
struct LauncherContado {
    interno: FixtureLauncher,
    npm_no_lancamento: Arc<Mutex<Vec<usize>>>,
}

impl BridgeLauncher for LauncherContado {
    fn command(&self) -> Result<tokio::process::Command, BridgeError> {
        if let Ok(mut v) = self.npm_no_lancamento.lock() {
            v.push(npm_calls(&self.interno.dir));
        }
        self.interno.command()
    }
    fn describe(&self) -> String {
        self.interno.describe()
    }
    fn dir(&self) -> PathBuf {
        self.interno.dir()
    }
}

struct Boot {
    _dir: tempfile::TempDir,
    state: SharedState,
    store: SessionStore,
    bridge: PathBuf,
    lancamentos: Arc<Mutex<Vec<usize>>>,
}

impl Boot {
    fn lancamentos(&self) -> Vec<usize> {
        self.lancamentos.lock().expect("lock").clone()
    }
}

/// Estado com sessao gravada. Cada teste arma o diretorio da ponte depois.
fn estado_com_sessao() -> (tempfile::TempDir, SharedState, SessionStore, SessionKey) {
    let dir = tempfile::tempdir().expect("tempdir");
    let (state, _provider) = monta_estado(&dir);
    let (store, key) = grava_sessao(&state);
    (dir, state, store, key)
}

fn sobe(
    dir: tempfile::TempDir,
    state: SharedState,
    store: SessionStore,
    key: SessionKey,
    preparo: PreparoDaPonte,
) -> Boot {
    let bridge = bridge_dir(&state);
    let lancamentos = Arc::new(Mutex::new(Vec::new()));
    let launcher: Arc<dyn BridgeLauncher> = Arc::new(LauncherContado {
        interno: FixtureLauncher {
            dir: bridge.clone(),
            roteiro: Roteiro::eco(),
        },
        npm_no_lancamento: Arc::clone(&lancamentos),
    });
    supervisionar(
        &state,
        LinkedSettings {
            enabled: true,
            ..LinkedSettings::default()
        },
        store.clone(),
        key,
        launcher,
        preparo,
    );
    Boot {
        _dir: dir,
        state,
        store,
        bridge,
        lancamentos,
    }
}

/// **O defeito da W1, na forma exata do update 0.4.4 -> 0.4.5.** No disco, a
/// instalacao que o `whatsapp link` da 0.4.4 deixou: `bridge.mjs` antigo, o
/// carimbo de assets da versao anterior, `node_modules` instalado para os
/// MESMOS manifestos (entre as duas versoes so o `bridge.mjs` mudou) e nenhum
/// carimbo de dependencias, que a 0.4.4 nao gravava. O boot tem de subir a
/// ponte embutida — e sem `npm`, que nao tem nada para fazer.
#[tokio::test]
async fn o_boot_troca_a_ponte_da_versao_anterior_pela_embutida_sem_rodar_npm() {
    let (dir, state, store, key) = estado_com_sessao();
    let bridge = bridge_dir(&state);
    garraia_common::fs_perms::create_secret_dir(&bridge).expect("dir");
    std::fs::write(bridge.join("bridge.mjs"), "// ponte da 0.4.4\n").expect("bridge antigo");
    for nome in ["package.json", "package-lock.json"] {
        std::fs::write(bridge.join(nome), embutido(nome)).expect("manifesto");
    }
    std::fs::write(bridge.join(".garraia-bridge-sha256"), "0".repeat(64)).expect("carimbo");
    std::fs::create_dir_all(bridge.join("node_modules")).expect("node_modules");
    std::fs::write(bridge.join(".fake-npm-ok"), "").expect("npm em modo ok");
    let pkg = envelhece(&bridge.join("package.json"));
    let lock = envelhece(&bridge.join("package-lock.json"));

    let boot = sobe(
        dir,
        state,
        store,
        key,
        PreparoDaPonte {
            dir: bridge.clone(),
            assets: Arc::new(EmbeddedAssets),
            npm: Some(fake_npm()),
        },
    );

    assert!(
        ate(|| boot.state.whatsapp_linked.bridge() == BridgeView::Connected).await,
        "a ponte tem de subir depois de atualizada"
    );
    assert_eq!(
        std::fs::read_to_string(boot.bridge.join("bridge.mjs")).expect("le"),
        embutido("bridge.mjs"),
        "o bridge.mjs em disco tem de ser o deste binario, e nao o da versao anterior"
    );
    assert_eq!(
        boot.lancamentos(),
        vec![0],
        "lancada uma vez, sem npm antes"
    );
    assert_eq!(
        npm_calls(&boot.bridge),
        0,
        "so o bridge.mjs mudou: nada de npm"
    );
    assert_eq!(mtime(&boot.bridge.join("package.json")), pkg);
    assert_eq!(
        mtime(&boot.bridge.join("package-lock.json")),
        lock,
        "manifesto igual nao e reescrito"
    );
    assert!(boot.state.whatsapp_linked.cancelar());
}

/// Nada mudou desde o `link`: nenhum arquivo reescrito, nenhum `npm`.
#[tokio::test]
async fn o_boot_sem_mudanca_nao_reescreve_nada_nem_roda_npm() {
    let (dir, state, store, key) = estado_com_sessao();
    let preparo = ponte_ja_pronta(&state).await;
    let bridge = preparo.dir.clone();
    assert_eq!(
        npm_calls(&bridge),
        1,
        "premissa: o link rodou o npm uma vez"
    );
    let antigos: Vec<(&str, std::time::SystemTime)> =
        ["bridge.mjs", "package.json", "package-lock.json"]
            .into_iter()
            .map(|n| (n, envelhece(&bridge.join(n))))
            .collect();

    let boot = sobe(dir, state, store, key, preparo);
    assert!(ate(|| boot.state.whatsapp_linked.bridge() == BridgeView::Connected).await);
    assert_eq!(npm_calls(&boot.bridge), 1, "o boot nao roda npm sem motivo");
    assert_eq!(boot.lancamentos(), vec![1]);
    for (nome, antigo) in antigos {
        assert_eq!(
            mtime(&boot.bridge.join(nome)),
            antigo,
            "{nome} foi reescrito"
        );
    }
    assert!(boot.state.whatsapp_linked.cancelar());
}

/// O `package-lock.json` embutido mudou (um bump do Baileys): o boot grava o
/// novo e roda o `npm ci` ANTES de lancar a ponte — e o `npm` le o lockfile
/// novo, nao o antigo.
#[tokio::test]
async fn lockfile_mudado_roda_npm_ci_antes_de_lancar_a_ponte() {
    let (dir, state, store, key) = estado_com_sessao();
    let preparo = ponte_ja_pronta(&state).await;
    let bridge = preparo.dir.clone();
    std::fs::write(
        bridge.join("package-lock.json"),
        "{\"lockfileVersion\":3,\"da\":\"versao anterior\"}\n",
    )
    .expect("lock antigo");
    std::fs::write(bridge.join("node_modules").join("arvore-antiga"), "x").expect("arvore");

    let boot = sobe(dir, state, store, key, preparo);
    assert!(ate(|| boot.state.whatsapp_linked.bridge() == BridgeView::Connected).await);

    assert_eq!(
        boot.lancamentos(),
        vec![2],
        "o npm do boot rodou, e rodou ANTES do lancamento"
    );
    assert_eq!(
        std::fs::read_to_string(boot.bridge.join("package-lock.json")).expect("le"),
        embutido("package-lock.json")
    );
    assert_eq!(
        std::fs::read_to_string(boot.bridge.join("node_modules/.package-lock.json"))
            .expect("o npm falso copia o lockfile que leu"),
        embutido("package-lock.json"),
        "o npm ci instalou a partir do lockfile novo"
    );
    assert!(
        !boot.bridge.join("node_modules/arvore-antiga").exists(),
        "a arvore antiga foi trocada"
    );
    assert!(boot.state.whatsapp_linked.cancelar());
}

/// O `npm ci` falha: a ponte nao e lancada (nunca contra dependencias de outra
/// versao), o canal fica fora com o proximo passo de sempre — `rode npm ci em
/// <dir>` —, e a sessao vinculada continua byte a byte onde estava.
#[tokio::test]
async fn npm_falhando_deixa_o_canal_fora_com_o_passo_certo_e_a_sessao_intacta() {
    let (dir, state, store, key) = estado_com_sessao();
    let preparo = ponte_ja_pronta(&state).await;
    let bridge = preparo.dir.clone();
    std::fs::remove_file(bridge.join(".fake-npm-ok")).expect("npm em modo falha");
    std::fs::write(
        bridge.join("package-lock.json"),
        "{\"lockfileVersion\":3,\"da\":\"versao anterior\"}\n",
    )
    .expect("lock antigo");
    let sessao_antes = retrato_da_sessao(&store);
    assert!(!sessao_antes.is_empty(), "premissa: ha sessao em disco");

    let boot = sobe(dir, state, store, key, preparo);
    assert!(
        ate(|| boot.state.whatsapp_linked.bridge() == BridgeView::Down).await,
        "o supervisor desiste e diz que a ponte esta fora"
    );

    assert_eq!(npm_calls(&boot.bridge), 2, "o boot tentou o npm ci");
    assert!(
        boot.lancamentos().is_empty(),
        "a ponte nao pode subir contra um node_modules de outra versao"
    );
    assert!(
        !deps_installed(&boot.bridge),
        "a arvore que nao e dos manifestos atuais sai do disco"
    );

    let (saude, dir_da_ponte) = health(&boot.state.config, &boot.state.whatsapp_linked);
    assert_eq!(saude, LinkHealth::MissingDependencies);
    let passo = saude
        .next_step(&dir_da_ponte, "garraia")
        .expect("ha proximo passo");
    assert!(
        passo.contains("npm ci") && passo.contains(&boot.bridge.display().to_string()),
        "{passo}"
    );

    assert_eq!(
        retrato_da_sessao(&boot.store),
        sessao_antes,
        "a sessao vinculada nao pode ser tocada"
    );
    // A tarefa do supervisor ja terminou: nao ha o que cancelar. Quem tenta
    // de novo e o proximo boot.
    let _ = boot.state.whatsapp_linked.cancelar();
}

/// Manifestos mudados e nenhum `npm` na PATH do gateway: mesmo desfecho da
/// falha do `npm` — nada sobe, e o disco diz "sem dependencias".
#[tokio::test]
async fn sem_npm_na_path_e_manifesto_mudado_a_ponte_nao_sobe() {
    let (dir, state, store, key) = estado_com_sessao();
    let mut preparo = ponte_ja_pronta(&state).await;
    preparo.npm = None;
    std::fs::write(
        preparo.dir.join("package-lock.json"),
        "{\"lockfileVersion\":3,\"da\":\"versao anterior\"}\n",
    )
    .expect("lock antigo");

    let boot = sobe(dir, state, store, key, preparo);
    assert!(ate(|| boot.state.whatsapp_linked.bridge() == BridgeView::Down).await);
    assert!(boot.lancamentos().is_empty());
    assert_eq!(npm_calls(&boot.bridge), 1, "so o do link");
    assert_eq!(
        health(&boot.state.config, &boot.state.whatsapp_linked).0,
        LinkHealth::MissingDependencies
    );
    let _ = boot.state.whatsapp_linked.cancelar();
}

/// Cancelado antes de a tarefa rodar (o que os testes de boot com `node`
/// real fazem): o preparo nao toca o disco nem roda `npm`.
#[tokio::test]
async fn cancelado_antes_do_preparo_nao_toca_o_disco() {
    let (dir, state, store, key) = estado_com_sessao();
    let bridge = bridge_dir(&state);
    let boot = sobe(
        dir,
        state,
        store,
        key,
        PreparoDaPonte {
            dir: bridge.clone(),
            assets: Arc::new(EmbeddedAssets),
            npm: Some(fake_npm()),
        },
    );
    // Sem `.await` entre subir e cancelar: a tarefa so roda depois.
    assert!(boot.state.whatsapp_linked.cancelar());
    assert!(ate(|| boot.state.whatsapp_linked.bridge() == BridgeView::Down).await);
    assert!(!bridge.exists(), "o diretorio da ponte nem nasce");
    assert!(boot.lancamentos().is_empty());
}

/// O preparo do boot usa os assets DESTE binario e o `npm` da PATH.
#[test]
fn o_preparo_do_boot_e_o_embutido() {
    let dir = tempfile::tempdir().expect("tempdir");
    let preparo = PreparoDaPonte::embutido(dir.path().join("bridge"));
    assert_eq!(
        assets_digest(preparo.assets.as_ref()),
        assets_digest(&EmbeddedAssets)
    );
    assert_eq!(
        preparo.npm,
        garraia_channels::whatsapp_linked::bridge::find_executable("npm")
    );

    let fonte = include_str!("../../../whatsapp_linked.rs");
    let boot = fonte
        .split("pub fn spawn_whatsapp_linked(")
        .nth(1)
        .and_then(|resto| resto.split("\n}\n").next())
        .expect("corpo de spawn_whatsapp_linked");
    assert!(
        boot.contains("PreparoDaPonte::embutido(paths.bridge_dir"),
        "o boot tem de preparar a ponte embutida no diretorio da ponte"
    );
}
