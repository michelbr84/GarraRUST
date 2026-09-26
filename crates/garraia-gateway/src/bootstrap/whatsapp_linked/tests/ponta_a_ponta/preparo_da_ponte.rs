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
    Asset, BridgeAssets, DepsPlan, EmbeddedAssets, assets_digest, deps_digest, deps_installed,
    install_deps, prepare,
};
use std::path::Path;

/// O carimbo de dependencias da ponte (`.garraia-deps-sha256`).
const CARIMBO_DE_DEPS: &str = ".garraia-deps-sha256";

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

/// O `node_modules/.package-lock.json` que o npm grava depois de instalar
/// `lock`: as entradas de `packages`, sem a raiz `""` e sem as `optional` —
/// a forma que o `fake_npm.py` grava, e a de uma instalacao real da ponte.
fn registro_do_npm(lock: &str) -> serde_json::Value {
    let mut v: serde_json::Value = serde_json::from_str(lock).expect("lock");
    v.get_mut("packages")
        .and_then(serde_json::Value::as_object_mut)
        .expect("packages")
        .retain(|nome, entrada| {
            !nome.is_empty() && entrada.get("optional") != Some(&serde_json::Value::Bool(true))
        });
    v
}

fn registro_em_disco(dir: &Path) -> serde_json::Value {
    serde_json::from_str(
        &std::fs::read_to_string(dir.join("node_modules/.package-lock.json")).expect("registro"),
    )
    .expect("json")
}

/// O `package-lock.json` de uma versao anterior cujo Baileys difere do
/// embutido — o update por CVE que obriga o `npm ci`.
fn lock_da_versao_anterior() -> String {
    let mut v: serde_json::Value =
        serde_json::from_str(embutido("package-lock.json")).expect("lock");
    let baileys = &mut v["packages"]["node_modules/@whiskeysockets/baileys"];
    assert!(
        baileys.is_object(),
        "premissa: o lock embutido tem o Baileys"
    );
    baileys["version"] = serde_json::json!("7.0.0-rc9");
    baileys["integrity"] = serde_json::json!("sha512-versao-anterior");
    serde_json::to_string_pretty(&v).expect("json")
}

/// O disco de uma instalacao da versao ANTERIOR: o lock dela em disco, a
/// arvore que o `npm ci` dela instalou (com o registro do npm para ESSE
/// lock) e o carimbo dos manifestos dela.
fn instalacao_da_versao_anterior(dir: &Path) {
    let lock = lock_da_versao_anterior();
    std::fs::write(dir.join("package-lock.json"), &lock).expect("lock antigo");
    let modules = dir.join("node_modules");
    let _ = std::fs::remove_dir_all(&modules);
    std::fs::create_dir_all(modules.join("arvore-antiga")).expect("arvore antiga");
    std::fs::write(
        modules.join(".package-lock.json"),
        registro_do_npm(&lock).to_string(),
    )
    .expect("registro antigo");
    std::fs::write(dir.join(CARIMBO_DE_DEPS), "0".repeat(64)).expect("carimbo antigo");
}

/// O usuario, no terminal dele: `cd <dir da ponte> && npm ci` — o passo que o
/// `status` e o `/api/diagnostics` mandam dar. Aqui, com o `npm` falso em
/// modo ok, que imita a instalacao (e o registro) do npm de verdade.
fn npm_ci_a_mao(dir: &Path) {
    std::fs::write(dir.join(".fake-npm-ok"), "").expect("npm do usuario funciona");
    let status = std::process::Command::new(fake_npm())
        .arg("ci")
        .current_dir(dir)
        .status()
        .expect("npm a mao");
    assert!(status.success());
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
            access: Default::default(),
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
/// MESMOS manifestos (entre as duas versoes so o `bridge.mjs` mudou) — com o
/// registro que o npm grava ao terminar — e nenhum carimbo de dependencias,
/// que a 0.4.4 nao gravava. O boot tem de subir a ponte embutida — e sem
/// `npm`, que nao tem nada para fazer.
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
    std::fs::write(
        bridge.join("node_modules/.package-lock.json"),
        registro_do_npm(embutido("package-lock.json")).to_string(),
    )
    .expect("o registro que o npm ci da 0.4.4 gravou");
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
    instalacao_da_versao_anterior(&bridge);

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
        registro_em_disco(&boot.bridge),
        registro_do_npm(embutido("package-lock.json")),
        "o npm ci instalou a partir do lockfile novo"
    );
    assert_eq!(
        std::fs::read_to_string(boot.bridge.join(CARIMBO_DE_DEPS)).expect("carimbo"),
        deps_digest(&EmbeddedAssets)
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
    instalacao_da_versao_anterior(&bridge);
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
        passo.contains("npm ci")
            && passo.contains(&boot.bridge.display().to_string())
            && passo.contains("reinicie o gateway"),
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

/// Arvore de outra versao e nenhum `npm` na PATH do gateway: a ponte nao
/// sobe, e o disco diz "sem dependencias" — mas a arvore FICA. Nao foi este
/// gateway que tentou instala-la, e apagar o que ele nao instalou e o que
/// fazia o passo do `status` (`npm ci` a mao) virar um `rm -r` no boot
/// seguinte.
#[tokio::test]
async fn sem_npm_na_path_e_manifesto_mudado_a_ponte_nao_sobe_e_nada_e_apagado() {
    let (dir, state, store, key) = estado_com_sessao();
    let mut preparo = ponte_ja_pronta(&state).await;
    preparo.npm = None;
    instalacao_da_versao_anterior(&preparo.dir);

    let boot = sobe(dir, state, store, key, preparo);
    assert!(ate(|| boot.state.whatsapp_linked.bridge() == BridgeView::Down).await);
    assert!(boot.lancamentos().is_empty());
    assert_eq!(npm_calls(&boot.bridge), 1, "so o do link");
    assert_eq!(
        health(&boot.state.config, &boot.state.whatsapp_linked).0,
        LinkHealth::MissingDependencies
    );
    assert!(
        boot.bridge.join("node_modules/arvore-antiga").is_dir(),
        "o gateway sem npm nao apaga uma arvore que ele nao tentou instalar"
    );
    assert_eq!(
        registro_em_disco(&boot.bridge),
        registro_do_npm(&lock_da_versao_anterior()),
        "nem mexe nela"
    );
    let _ = boot.state.whatsapp_linked.cancelar();
}

/// **O passo que o `status` manda dar, funcionando (item 1 da verificacao da
/// W1).** O `npm ci` do gateway falha; o `status` diz `rode npm ci em <dir> e
/// reinicie o gateway`; o usuario roda, no shell dele (onde o `npm` existe),
/// e reinicia — com um gateway que nem tem `npm` na PATH, como um servico do
/// systemd com PATH curta. O boot seguinte adota a arvore pelo registro do
/// npm, sem rodar `npm`, sem apagar nada, e a ponte sobe. Antes o carimbo
/// ficava em `pending` para sempre, o boot pedia outro `npm ci` e, sem `npm`,
/// apagava a instalacao do usuario.
#[tokio::test]
async fn npm_ci_a_mao_depois_da_falha_e_adotado_no_boot_seguinte_mesmo_sem_npm() {
    let (dir, state, store, key) = estado_com_sessao();
    let bridge = bridge_dir(&state);

    // Boot 1: a versao anterior no disco, o `npm` do gateway falhando.
    let primeiro = ponte_ja_pronta(&state).await;
    std::fs::remove_file(bridge.join(".fake-npm-ok")).expect("npm do gateway falha");
    instalacao_da_versao_anterior(&bridge);
    let (_tx, mut cancel) = watch::channel(false);
    assert!(matches!(
        preparar_ponte(&primeiro, &mut cancel).await,
        Err(PonteNaoPronta::Npm { .. })
    ));
    assert_eq!(npm_calls(&bridge), 2, "o do link e o do boot 1");
    let (saude, dir_da_ponte) = health(&state.config, &state.whatsapp_linked);
    assert_eq!(saude, LinkHealth::MissingDependencies);
    let passo = saude.next_step(&dir_da_ponte, "garraia").expect("passo");
    assert!(
        passo.contains("npm ci") && passo.contains("reinicie o gateway"),
        "{passo}"
    );

    // O usuario segue o passo.
    npm_ci_a_mao(&bridge);
    assert_eq!(npm_calls(&bridge), 3);

    // Boot 2, sem `npm` na PATH do gateway.
    let boot = sobe(
        dir,
        state,
        store,
        key,
        PreparoDaPonte {
            dir: bridge.clone(),
            assets: Arc::new(EmbeddedAssets),
            npm: None,
        },
    );
    assert!(
        ate(|| boot.state.whatsapp_linked.bridge() == BridgeView::Connected).await,
        "a arvore que o usuario instalou e adotada e a ponte sobe"
    );
    assert_eq!(
        boot.lancamentos(),
        vec![3],
        "sem npm do gateway antes do lancamento"
    );
    assert_eq!(npm_calls(&boot.bridge), 3);
    assert_eq!(
        registro_em_disco(&boot.bridge),
        registro_do_npm(embutido("package-lock.json")),
        "a arvore do usuario continua la"
    );
    assert_eq!(
        std::fs::read_to_string(boot.bridge.join(CARIMBO_DE_DEPS)).expect("carimbo"),
        deps_digest(&EmbeddedAssets),
        "adotada e carimbada: o boot seguinte nem le o registro"
    );
    assert!(
        deps_installed(&boot.bridge),
        "o status volta a dizer dependencias instaladas"
    );
    assert!(boot.state.whatsapp_linked.cancelar());
}

/// Uma arvore que e a do lock embutido — pelo registro do npm — com o
/// carimbo em `pending` (um `npm ci` do gateway que foi interrompido depois
/// de o usuario ja ter instalado, por exemplo) e nenhum `npm` na PATH: o
/// gateway sobe a ponte e nao apaga nada.
#[tokio::test]
async fn sem_npm_na_path_uma_arvore_do_lock_embutido_nunca_e_apagada() {
    let (dir, state, store, key) = estado_com_sessao();
    let mut preparo = ponte_ja_pronta(&state).await;
    preparo.npm = None;
    std::fs::write(preparo.dir.join(CARIMBO_DE_DEPS), "pending").expect("pending");
    // Dentro de um pacote, e nao na raiz do `node_modules`: um arquivo novo
    // na raiz mudaria o mtime do diretorio, e uma arvore mexida depois do
    // registro do npm nao e adotada (ver `tree_matches_lock`).
    let marca = preparo
        .dir
        .join("node_modules/@whiskeysockets/baileys/marca-do-usuario");
    std::fs::write(&marca, "fica").expect("marca");

    let boot = sobe(dir, state, store, key, preparo);
    assert!(ate(|| boot.state.whatsapp_linked.bridge() == BridgeView::Connected).await);
    assert_eq!(boot.lancamentos(), vec![1]);
    assert!(
        marca_existe(&boot.bridge),
        "a arvore que casa com o lock embutido nao sai do disco"
    );
    assert!(boot.state.whatsapp_linked.cancelar());
}

fn marca_existe(bridge: &Path) -> bool {
    bridge
        .join("node_modules/@whiskeysockets/baileys/marca-do-usuario")
        .is_file()
}

/// Assets que pedem o cancelamento do supervisor no meio do `prepare` — o
/// gateway encerrando exatamente entre o preparo (manifestos ja reescritos,
/// resposta `Install`) e o primeiro poll do `npm ci`, que o `select!` com
/// `biased` nunca chega a fazer.
struct CancelaNoPreparo(watch::Sender<bool>);

impl BridgeAssets for CancelaNoPreparo {
    fn files(&self) -> &[Asset] {
        self.0.send_replace(true);
        EmbeddedAssets.files()
    }
}

/// **A janela de antes do carimbo (item 2 da verificacao da W1).** Instalacao
/// da 0.4.4 (sem carimbo de dependencias) cujo lock difere do embutido; o
/// primeiro boot reescreve os manifestos e e cancelado antes do `npm ci`. O
/// carimbo ja diz `pending` (gravado antes de o lock mudar), o `status` diz
/// "sem dependencias", e o boot seguinte roda o `npm ci` ANTES de lancar —
/// nunca adota a arvore antiga com os manifestos novos.
#[tokio::test]
async fn cancelado_entre_o_preparo_e_o_npm_o_boot_seguinte_nao_adota_a_arvore_antiga() {
    let (dir, state, store, key) = estado_com_sessao();
    let bridge = bridge_dir(&state);
    let _ = ponte_ja_pronta(&state).await;
    instalacao_da_versao_anterior(&bridge);
    std::fs::remove_file(bridge.join(CARIMBO_DE_DEPS)).expect("a 0.4.4 nao tinha carimbo");
    assert!(deps_installed(&bridge), "premissa: a 0.4.4 instalada");
    let chamadas_antes = npm_calls(&bridge);

    let (tx, mut cancel) = watch::channel(false);
    let cancelado = PreparoDaPonte {
        dir: bridge.clone(),
        assets: Arc::new(CancelaNoPreparo(tx)),
        npm: Some(fake_npm()),
    };
    assert!(matches!(
        preparar_ponte(&cancelado, &mut cancel).await,
        Err(PonteNaoPronta::Cancelado)
    ));
    assert_eq!(
        npm_calls(&bridge),
        chamadas_antes,
        "o npm ci nao chegou a rodar"
    );
    assert_eq!(
        std::fs::read_to_string(bridge.join("package-lock.json")).expect("lock"),
        embutido("package-lock.json"),
        "premissa: os manifestos ja sao os novos"
    );
    assert_eq!(
        std::fs::read_to_string(bridge.join(CARIMBO_DE_DEPS)).expect("carimbo"),
        "pending"
    );
    assert!(!deps_installed(&bridge), "o status ja diz sem dependencias");
    assert_eq!(
        prepare(&bridge, &EmbeddedAssets).expect("prepare").deps,
        DepsPlan::Install,
        "a arvore antiga nao passa por atual"
    );

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
    assert!(ate(|| boot.state.whatsapp_linked.bridge() == BridgeView::Connected).await);
    assert_eq!(
        boot.lancamentos(),
        vec![chamadas_antes + 1],
        "o npm ci do boot seguinte rodou ANTES do lancamento"
    );
    assert!(!boot.bridge.join("node_modules/arvore-antiga").exists());
    assert!(boot.state.whatsapp_linked.cancelar());
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

/// Assets num "disco lento": cada `files()` segura a thread que o chamou.
struct AssetsLentos;

impl BridgeAssets for AssetsLentos {
    fn files(&self) -> &[Asset] {
        std::thread::sleep(std::time::Duration::from_millis(40));
        EmbeddedAssets.files()
    }
}

/// **Item 4 da verificacao da W1.** O `prepare` e I/O sincrono de disco
/// (comparar os assets, ler o registro do npm, gravar o carimbo). Rodado
/// direto no `async fn`, ele segurava a thread do runtime: no runtime de uma
/// thread so, nenhuma outra tarefa do gateway andava enquanto ele durava.
/// Em `spawn_blocking`, as outras tarefas seguem.
#[tokio::test(flavor = "current_thread")]
async fn o_preparo_nao_segura_a_thread_do_runtime() {
    use std::sync::atomic::{AtomicUsize, Ordering};

    let dir = tempfile::tempdir().expect("tempdir");
    let preparo = PreparoDaPonte {
        dir: dir.path().join("bridge"),
        assets: Arc::new(AssetsLentos),
        npm: None,
    };
    let voltas = Arc::new(AtomicUsize::new(0));
    let contador = Arc::clone(&voltas);
    let outra_tarefa = tokio::spawn(async move {
        loop {
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
            contador.fetch_add(1, Ordering::SeqCst);
        }
    });
    tokio::task::yield_now().await;

    let (_tx, mut cancel) = watch::channel(false);
    let desfecho = preparar_ponte(&preparo, &mut cancel).await;
    let durante = voltas.load(Ordering::SeqCst);
    outra_tarefa.abort();

    assert!(
        matches!(desfecho, Err(PonteNaoPronta::SemNpm { .. })),
        "premissa: sem node_modules e sem npm, {desfecho:?}"
    );
    assert!(
        durante >= 5,
        "a outra tarefa do runtime andou {durante} vez(es) durante ~200 ms de preparo"
    );
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
