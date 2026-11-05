//! A configuracao de outra instalacao (#1824).
//!
//! Quando ja ha um gateway GarraIA rodando fora do app (#1816, `Origem::Externo`
//! na casca), o `config.yml` da pasta padrao e o DELE: um servico systemd, a CLI
//! de outra versao. Os passos do assistente gravam nessa pasta (provedor,
//! persona, politica de acesso e a conexao do WhatsApp), e no teste da
//! v0.5.2-rc2 isso trocou em silencio o prompt de sistema de um bot do Telegram
//! 0.5.1 ja instalado.
//!
//! A regra mora aqui, pura e testada no CI (a casca Tauri esta fora dos gates):
//!
//! - gateway do proprio app: grava como sempre, sem perguntar nem copiar;
//! - gateway de fora e sem confirmacao explicita: recusa ANTES de tocar em
//!   qualquer arquivo, dizendo qual instalacao e o que mudaria;
//! - gateway de fora e confirmado: copia o `config.yml` para `backups/` e so
//!   entao deixa o passo gravar. Sem backup, nao grava (fail-closed);
//! - o pareamento com gateway de fora e recusado sempre: o `link` abriria uma
//!   segunda ponte com a sessao viva do outro, e o backup do `config.yml` nao
//!   protege a sessao.
//!
//! A guarda nao e so convencao: os construtores de argumentos que gravam
//! ([`crate::onboarding::args_pareamento`], [`crate::onboarding::args_aplicar_modo`],
//! [`crate::onboarding::args_autorizar_numero`],
//! [`crate::onboarding::montar_system_prompt`] e
//! [`crate::provedor::Escolha::args_set_routing`]) exigem um [`Liberado`](crate::config_alheia::Liberado), que so
//! nasce em [`preparar_gravacao`](crate::config_alheia::preparar_gravacao). Pular a guarda vira erro de compilacao.

use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// O passo do assistente que vai gravar na configuracao.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Passo {
    /// "Quem responde": `llm.<provedor>` e `agent.default_provider`.
    Provedor,
    /// A persona: `agent.system_prompt`.
    Persona,
    /// O modo do WhatsApp: a politica de admissao e quem esta autorizado.
    Acesso,
    /// O pareamento: liga o canal e troca a sessao vinculada do WhatsApp.
    Pareamento,
}

impl Passo {
    /// Os quatro passos que gravam, na ordem do assistente.
    pub const TODOS: [Passo; 4] = [
        Passo::Provedor,
        Passo::Pareamento,
        Passo::Persona,
        Passo::Acesso,
    ];

    /// Rotulo curto, ASCII, usado no nome do backup.
    pub fn slug(self) -> &'static str {
        match self {
            Passo::Provedor => "provedor",
            Passo::Persona => "persona",
            Passo::Acesso => "acesso",
            Passo::Pareamento => "pareamento",
        }
    }

    /// O que muda, nas palavras de quem usa.
    pub fn descricao(self) -> &'static str {
        match self {
            Passo::Provedor => "quem responde (o modelo de IA e o provedor padrao)",
            Passo::Persona => "a persona (o prompt de sistema do assistente)",
            Passo::Acesso => "quem pode falar com o Garra pelo WhatsApp",
            Passo::Pareamento => "a conexao do WhatsApp (o canal e a sessao vinculada)",
        }
    }
}

/// Prova de que a guarda da #1824 rodou. So nasce em [`preparar_gravacao`]
/// (sem construtor publico); os construtores de argumentos que gravam a exigem.
#[derive(Debug)]
#[must_use = "o passo so pode gravar com a prova da guarda em maos"]
pub struct Liberado {
    passo: Passo,
    backup: Option<PathBuf>,
}

impl Liberado {
    /// O passo que foi liberado.
    pub fn passo(&self) -> Passo {
        self.passo
    }

    /// A copia do `config.yml` feita antes de gravar (so com gateway de fora).
    pub fn backup(&self) -> Option<&Path> {
        self.backup.as_deref()
    }

    /// So para os testes do proprio core montarem argumentos.
    #[cfg(test)]
    pub(crate) fn para_teste(passo: Passo) -> Self {
        Self {
            passo,
            backup: None,
        }
    }
}

/// Decide se o passo pode gravar. `externo` e a versao do gateway de fora do
/// app, quando ha um. So o gateway de fora exige confirmacao, e o pareamento
/// com ele e recusado mesmo confirmado.
pub fn exigir_confirmacao(
    externo: Option<&str>,
    confirmado: bool,
    passo: Passo,
) -> Result<(), String> {
    let Some(versao) = externo else {
        return Ok(());
    };
    if passo == Passo::Pareamento {
        return Err(format!(
            "Ja existe um Garra {versao} rodando nesta maquina, fora deste app, com a conexao do \
WhatsApp dele. Parear por aqui abriria uma segunda conexao com a mesma conta. Para parear por este \
app, pare esse Garra (por exemplo `systemctl --user stop garraia`) e abra o app de novo; ou pareie \
pelo proprio Garra (`garraia whatsapp link`)."
        ));
    }
    if !confirmado {
        return Err(format!(
            "Ja existe um Garra {versao} rodando nesta maquina, fora deste app, e ele usa esta \
mesma configuracao. Este passo mudaria {}. Confirme na tela que quer configurar esse Garra; \
antes de gravar, o app guarda uma copia do config.yml na pasta backups.",
            passo.descricao()
        ));
    }
    Ok(())
}

/// Quem segura `<config>/gateway.lock` (#1826). O `garraia start` segura essa
/// trava exclusiva a vida inteira do processo (`bind_gate::travar_diretorio`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Trava {
    /// Ninguem segura (ou o arquivo nem existe).
    Livre,
    /// Algum processo segura a trava agora.
    Ocupada,
    /// O sistema de arquivos nao suporta trava: sem sinal, nao decide nada.
    Indeterminada,
}

/// Sonda a trava de `config_dir` sem deixar rastro: nao cria arquivo nem
/// diretorio, e solta a trava na hora se a obteve. Quem decide o que a trava
/// significa e [`externo_efetivo`].
pub fn sondar_trava(config_dir: &Path) -> Trava {
    let arquivo = match OpenOptions::new()
        .read(true)
        .write(true)
        .open(config_dir.join("gateway.lock"))
    {
        Ok(f) => f,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Trava::Livre,
        Err(_) => return Trava::Indeterminada,
    };
    match arquivo.try_lock() {
        // Obteve: ninguem segurava. Fechar o arquivo (fim do escopo) solta.
        Ok(()) => Trava::Livre,
        Err(fs::TryLockError::WouldBlock) => Trava::Ocupada,
        Err(fs::TryLockError::Error(_)) => Trava::Indeterminada,
    }
}

/// O texto usado no lugar da versao quando so a trava denuncia o gateway de
/// fora (nao houve `/api/health` para perguntar).
pub const VERSAO_DESCONHECIDA: &str = "(versao desconhecida)";

/// Junta o sinal da porta (#1816) com o da trava (#1826): a versao do gateway
/// de fora, se ha um. `declarado` e o que o app decidiu pela porta; `trava` e o
/// resultado de [`sondar_trava`]; `filho_vivo` diz se o sidecar DESTE app esta
/// de pe — nesse caso a trava ocupada e dele e nao conta. Sidecar morto com a
/// trava ocupada e outro gateway (a corrida no bind) e conta como de fora.
pub fn externo_efetivo(declarado: Option<&str>, trava: Trava, filho_vivo: bool) -> Option<String> {
    if let Some(v) = declarado {
        return Some(v.to_string());
    }
    if trava == Trava::Ocupada && !filho_vivo {
        return Some(VERSAO_DESCONHECIDA.to_string());
    }
    None
}

/// O trecho que identifica os backups feitos pelo assistente.
const SUFIXO_BACKUP: &str = "_antes-do-desktop-";

/// Nome do backup: `config_<AAAAMMDD>T<HHMMSS>Z_antes-do-desktop-<passo>.yml`,
/// em UTC. Fica ao lado dos backups diarios (`config_<data>_<hora>.yml`).
pub fn nome_do_backup(passo: Passo, agora_unix: u64) -> String {
    format!(
        "config_{}{SUFIXO_BACKUP}{}.yml",
        carimbo_utc(agora_unix),
        passo.slug()
    )
}

/// `AAAAMMDDTHHMMSSZ` de um instante Unix, sem dependencia de data.
fn carimbo_utc(agora_unix: u64) -> String {
    let dias = (agora_unix / 86_400) as i64;
    let seg = agora_unix % 86_400;
    let (ano, mes, dia) = data_civil(dias);
    format!(
        "{ano:04}{mes:02}{dia:02}T{:02}{:02}{:02}Z",
        seg / 3600,
        (seg / 60) % 60,
        seg % 60
    )
}

/// Dias desde 1970-01-01 -> (ano, mes, dia) no calendario gregoriano
/// proleptico (algoritmo `civil_from_days` de Howard Hinnant).
fn data_civil(dias: i64) -> (i64, u32, u32) {
    let z = dias + 719_468;
    let era = (if z >= 0 { z } else { z - 146_096 }) / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let dia = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let mes = (if mp < 10 { mp + 3 } else { mp - 9 }) as u32;
    let ano = yoe + era * 400 + i64::from(mes <= 2);
    (ano, mes, dia)
}

/// Copia `<config_dir>/config.yml` para `<config_dir>/backups/` com o nome de
/// [`nome_do_backup`]. Devolve `Ok(None)` quando ainda nao ha config.
///
/// A copia carrega os mesmos segredos do `config.yml` (`llm.*.api_key`), entao:
/// nasce `0600` no Unix; abre com `create_new` (nunca segue um symlink plantado
/// nem sobrescreve outro backup) e ganha um sufixo `-2`, `-3`... quando o nome
/// ja existe; recusa uma pasta `backups` que seja symlink; e nao duplica: se ja
/// ha um backup do assistente com o mesmo conteudo, devolve esse.
pub fn backup_da_config(
    config_dir: &Path,
    passo: Passo,
    agora_unix: u64,
) -> io::Result<Option<PathBuf>> {
    let conteudo = match fs::read(config_dir.join("config.yml")) {
        Ok(c) => c,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e),
    };
    let pasta = config_dir.join("backups");
    preparar_pasta_privada(&pasta)?;
    if let Some(igual) = backup_igual(&pasta, &conteudo)? {
        return Ok(Some(igual));
    }
    let base = nome_do_backup(passo, agora_unix);
    let raiz = base.trim_end_matches(".yml");
    for n in 1..=100u32 {
        let nome = if n == 1 {
            base.clone()
        } else {
            format!("{raiz}-{n}.yml")
        };
        let destino = pasta.join(nome);
        match abrir_novo_privado(&destino) {
            Ok(mut arq) => {
                arq.write_all(&conteudo)?;
                arq.sync_all()?;
                return Ok(Some(destino));
            }
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e),
        }
    }
    Err(io::Error::new(
        io::ErrorKind::AlreadyExists,
        "ja ha 100 backups com este nome; limpe a pasta backups",
    ))
}

/// Um backup do assistente (arquivo comum, nunca symlink) com exatamente este
/// conteudo, se houver: cada passo confirmado nao precisa de mais uma copia
/// dos mesmos segredos.
fn backup_igual(pasta: &Path, conteudo: &[u8]) -> io::Result<Option<PathBuf>> {
    for entrada in fs::read_dir(pasta)? {
        let entrada = entrada?;
        let nome = entrada.file_name();
        let Some(nome) = nome.to_str() else { continue };
        let do_assistente =
            nome.starts_with("config_") && nome.contains(SUFIXO_BACKUP) && nome.ends_with(".yml");
        if !do_assistente || !entrada.file_type()?.is_file() {
            continue;
        }
        if fs::read(entrada.path())? == conteudo {
            return Ok(Some(entrada.path()));
        }
    }
    Ok(None)
}

/// Cria `backups` (0700 no Unix) ou aceita a que existe, desde que seja uma
/// pasta de verdade: um symlink ali mandaria a copia dos segredos para outro
/// lugar.
fn preparar_pasta_privada(pasta: &Path) -> io::Result<()> {
    match fs::symlink_metadata(pasta) {
        Ok(m) if m.file_type().is_symlink() => Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!(
                "{} e um link simbolico; nao copio segredos para la",
                pasta.display()
            ),
        )),
        Ok(m) if m.is_dir() => Ok(()),
        Ok(_) => Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("{} existe e nao e uma pasta", pasta.display()),
        )),
        Err(e) if e.kind() == io::ErrorKind::NotFound => {
            let mut b = fs::DirBuilder::new();
            b.recursive(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::DirBuilderExt;
                b.mode(0o700);
            }
            b.create(pasta)
        }
        Err(e) => Err(e),
    }
}

fn abrir_novo_privado(destino: &Path) -> io::Result<fs::File> {
    let mut o = OpenOptions::new();
    o.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        o.mode(0o600);
    }
    o.open(destino)
}

/// A guarda inteira, na ordem certa, antes de cada passo do assistente gravar
/// em `config_dir`:
///
/// 1. sem gateway de fora: libera sem copiar nada;
/// 2. gateway de fora sem confirmacao (ou o pareamento): `Err` sem tocar em
///    arquivo nenhum;
/// 3. gateway de fora usando `config.toml` (sem `config.yml`): `Err` — gravar
///    criaria um `config.yml` que passaria a valer no lugar do dele;
/// 4. gateway de fora confirmado: backup do `config.yml` e so entao libera.
///    Falha no backup e `Err`: o passo nao grava sem a copia.
pub fn preparar_gravacao(
    externo: Option<&str>,
    confirmado: bool,
    passo: Passo,
    config_dir: &Path,
    agora_unix: u64,
) -> Result<Liberado, String> {
    exigir_confirmacao(externo, confirmado, passo)?;
    let Some(versao) = externo else {
        return Ok(Liberado {
            passo,
            backup: None,
        });
    };
    if !config_dir.join("config.yml").exists() && config_dir.join("config.toml").exists() {
        return Err(format!(
            "O Garra {versao} que ja roda nesta maquina usa config.toml. Gravar por aqui criaria um \
config.yml que passaria a valer no lugar dele; configure {} pela CLI desse Garra.",
            passo.descricao()
        ));
    }
    let backup = backup_da_config(config_dir, passo, agora_unix).map_err(|e| {
        format!(
            "nao consegui guardar a copia do config.yml antes de mudar {} ({e}); nada foi gravado",
            passo.descricao()
        )
    })?;
    Ok(Liberado { passo, backup })
}

#[cfg(test)]
mod tests {
    use super::*;

    const AGORA: u64 = 1_793_761_325; // 2026-11-04T03:02:05Z
    const CONFIG: &str =
        "agent:\n  system_prompt: null\nllm:\n  openrouter:\n    api_key: segredo\n";
    /// Os passos que um gateway de fora pode liberar com confirmacao.
    const CONFIRMAVEIS: [Passo; 3] = [Passo::Provedor, Passo::Persona, Passo::Acesso];

    fn pasta_com_config() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("config.yml"), CONFIG).unwrap();
        dir
    }

    fn ler_config(dir: &tempfile::TempDir) -> String {
        fs::read_to_string(dir.path().join("config.yml")).unwrap()
    }

    #[test]
    fn gateway_do_app_grava_sem_pedir_nem_copiar() {
        let dir = pasta_com_config();
        for passo in Passo::TODOS {
            let l = preparar_gravacao(None, false, passo, dir.path(), AGORA).unwrap();
            assert_eq!(l.passo(), passo);
            assert_eq!(l.backup(), None);
        }
        assert!(!dir.path().join("backups").exists());
    }

    #[test]
    fn gateway_de_fora_sem_confirmacao_recusa_e_nao_toca_em_nada() {
        let dir = pasta_com_config();
        for passo in Passo::TODOS {
            let erro =
                preparar_gravacao(Some("0.5.1"), false, passo, dir.path(), AGORA).unwrap_err();
            assert!(erro.contains("0.5.1"), "o erro nomeia a instalacao: {erro}");
        }
        for passo in CONFIRMAVEIS {
            let erro = exigir_confirmacao(Some("0.5.1"), false, passo).unwrap_err();
            assert!(
                erro.contains(passo.descricao()),
                "o erro diz o que mudaria: {erro}"
            );
        }
        assert_eq!(ler_config(&dir), CONFIG, "config byte a byte igual");
        assert!(
            !dir.path().join("backups").exists(),
            "recusa nao cria backup"
        );
    }

    #[test]
    fn pareamento_com_gateway_de_fora_e_recusado_mesmo_confirmado() {
        let dir = pasta_com_config();
        let erro = preparar_gravacao(Some("0.5.1"), true, Passo::Pareamento, dir.path(), AGORA)
            .unwrap_err();
        assert!(erro.contains("segunda conexao"), "{erro}");
        assert!(erro.contains("systemctl --user stop garraia"), "{erro}");
        assert!(!dir.path().join("backups").exists());
    }

    #[test]
    fn gateway_de_fora_confirmado_copia_antes_byte_a_byte() {
        let dir = pasta_com_config();
        let l = preparar_gravacao(Some("0.5.1"), true, Passo::Persona, dir.path(), AGORA).unwrap();
        let copia = l.backup().expect("havia config, entao ha backup");
        assert_eq!(copia.parent().unwrap(), dir.path().join("backups"));
        assert_eq!(
            copia.file_name().unwrap().to_str().unwrap(),
            "config_20261104T030205Z_antes-do-desktop-persona.yml"
        );
        assert_eq!(fs::read(copia).unwrap(), CONFIG.as_bytes());
        assert_eq!(ler_config(&dir), CONFIG, "o original fica intacto");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let modo = fs::metadata(copia).unwrap().permissions().mode() & 0o777;
            assert_eq!(modo, 0o600, "a copia carrega segredos");
            let pasta = fs::metadata(dir.path().join("backups")).unwrap();
            assert_eq!(pasta.permissions().mode() & 0o777, 0o700);
        }
    }

    #[test]
    fn o_mesmo_conteudo_nao_vira_outra_copia_de_segredos() {
        let dir = pasta_com_config();
        let a = backup_da_config(dir.path(), Passo::Provedor, AGORA)
            .unwrap()
            .unwrap();
        let b = backup_da_config(dir.path(), Passo::Persona, AGORA + 60)
            .unwrap()
            .unwrap();
        assert_eq!(a, b, "conteudo igual reaproveita a copia");
        assert_eq!(fs::read_dir(dir.path().join("backups")).unwrap().count(), 1);
    }

    #[test]
    fn dois_backups_no_mesmo_segundo_nao_se_sobrescrevem() {
        let dir = pasta_com_config();
        let a = backup_da_config(dir.path(), Passo::Acesso, AGORA)
            .unwrap()
            .unwrap();
        fs::write(
            dir.path().join("config.yml"),
            "agent:\n  system_prompt: outro\n",
        )
        .unwrap();
        let b = backup_da_config(dir.path(), Passo::Acesso, AGORA)
            .unwrap()
            .unwrap();
        assert_ne!(a, b);
        assert!(
            b.to_str()
                .unwrap()
                .ends_with("_antes-do-desktop-acesso-2.yml")
        );
        assert_eq!(
            fs::read_to_string(&a).unwrap(),
            CONFIG,
            "o primeiro backup nao muda"
        );
    }

    #[cfg(unix)]
    #[test]
    fn backup_nao_segue_symlink_plantado_no_nome() {
        let dir = pasta_com_config();
        let alvo = dir.path().join("alvo.txt");
        fs::write(&alvo, "nao mexa").unwrap();
        fs::create_dir_all(dir.path().join("backups")).unwrap();
        let plantado = dir
            .path()
            .join("backups")
            .join(nome_do_backup(Passo::Provedor, AGORA));
        std::os::unix::fs::symlink(&alvo, &plantado).unwrap();

        let copia = backup_da_config(dir.path(), Passo::Provedor, AGORA)
            .unwrap()
            .unwrap();
        assert_ne!(copia, plantado);
        assert_eq!(fs::read_to_string(&alvo).unwrap(), "nao mexa");
        assert_eq!(fs::read(&copia).unwrap(), CONFIG.as_bytes());
    }

    #[cfg(unix)]
    #[test]
    fn pasta_backups_que_e_symlink_e_recusada() {
        let dir = pasta_com_config();
        let fora = tempfile::tempdir().unwrap();
        std::os::unix::fs::symlink(fora.path(), dir.path().join("backups")).unwrap();
        let erro =
            preparar_gravacao(Some("0.5.1"), true, Passo::Persona, dir.path(), AGORA).unwrap_err();
        assert!(erro.contains("nada foi gravado"), "{erro}");
        assert_eq!(
            fs::read_dir(fora.path()).unwrap().count(),
            0,
            "nenhum segredo foi parar la"
        );
    }

    #[test]
    fn sem_config_nao_ha_o_que_copiar() {
        let dir = tempfile::tempdir().unwrap();
        let l = preparar_gravacao(Some("0.5.1"), true, Passo::Provedor, dir.path(), AGORA).unwrap();
        assert_eq!(l.backup(), None);
    }

    #[test]
    fn config_toml_do_outro_garra_nao_ganha_um_yml_por_cima() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("config.toml"), "[agent]\n").unwrap();
        for passo in CONFIRMAVEIS {
            let erro =
                preparar_gravacao(Some("0.5.1"), true, passo, dir.path(), AGORA).unwrap_err();
            assert!(erro.contains("config.toml"), "{erro}");
        }
        assert!(!dir.path().join("config.yml").exists());
        // O app do proprio usuario continua gravando como antes.
        assert!(preparar_gravacao(None, false, Passo::Persona, dir.path(), AGORA).is_ok());
    }

    #[test]
    fn backup_que_falha_impede_a_gravacao() {
        let dir = pasta_com_config();
        // `backups` como ARQUIVO: a pasta nao pode ser usada.
        fs::write(dir.path().join("backups"), "").unwrap();
        let erro =
            preparar_gravacao(Some("0.5.1"), true, Passo::Persona, dir.path(), AGORA).unwrap_err();
        assert!(erro.contains("nada foi gravado"), "{erro}");
    }

    #[test]
    fn carimbo_em_utc_inclusive_anos_bissextos() {
        assert_eq!(carimbo_utc(0), "19700101T000000Z");
        assert_eq!(carimbo_utc(951_782_400), "20000229T000000Z");
        assert_eq!(carimbo_utc(1_709_164_800), "20240229T000000Z");
        assert_eq!(carimbo_utc(1_709_251_199), "20240229T235959Z");
        assert_eq!(carimbo_utc(AGORA), "20261104T030205Z");
        assert_eq!(carimbo_utc(4_102_444_800), "21000101T000000Z");
    }

    #[test]
    fn cada_passo_tem_rotulo_e_descricao_proprios() {
        let mut slugs: Vec<_> = Passo::TODOS.iter().map(|p| p.slug()).collect();
        slugs.sort_unstable();
        slugs.dedup();
        assert_eq!(slugs.len(), 4);
        for p in Passo::TODOS {
            assert!(p.slug().bytes().all(|b| b.is_ascii_lowercase()));
            assert!(!p.descricao().is_empty());
        }
    }

    #[test]
    fn passo_viaja_em_snake_case() {
        assert_eq!(
            serde_json::to_string(&Passo::Pareamento).unwrap(),
            "\"pareamento\""
        );
        let p: Passo = serde_json::from_str("\"acesso\"").unwrap();
        assert_eq!(p, Passo::Acesso);
    }

    #[test]
    fn externo_efetivo_decide_pela_porta_e_pela_trava() {
        // So a porta (comportamento da #1816, intacto).
        assert_eq!(
            externo_efetivo(Some("0.5.1"), Trava::Livre, true).as_deref(),
            Some("0.5.1")
        );
        // Trava de outro processo (sem filho do app): de fora.
        assert_eq!(
            externo_efetivo(None, Trava::Ocupada, false).as_deref(),
            Some(VERSAO_DESCONHECIDA)
        );
        // Trava do filho do app: nao e falso positivo.
        assert_eq!(externo_efetivo(None, Trava::Ocupada, true), None);
        // Sem trava, ou sem sinal: nada a dizer.
        assert_eq!(externo_efetivo(None, Trava::Livre, false), None);
        assert_eq!(externo_efetivo(None, Trava::Indeterminada, false), None);
        // A porta vence a trava quando conhece a versao.
        assert_eq!(
            externo_efetivo(Some("0.5.1"), Trava::Ocupada, false).as_deref(),
            Some("0.5.1")
        );
    }

    #[test]
    fn sondar_trava_ve_o_outro_handle_e_nao_deixa_rastro() {
        let dir = tempfile::tempdir().unwrap();
        // Sem arquivo: livre, e a sonda nao cria nada.
        assert_eq!(sondar_trava(dir.path()), Trava::Livre);
        assert!(!dir.path().join("gateway.lock").exists());
        // Com o arquivo e uma trava exclusiva segurada: ocupada.
        let f = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(dir.path().join("gateway.lock"))
            .unwrap();
        f.try_lock().unwrap();
        assert_eq!(sondar_trava(dir.path()), Trava::Ocupada);
        drop(f);
        // Solta: livre de novo, e a sonda nao ficou segurando.
        assert_eq!(sondar_trava(dir.path()), Trava::Livre);
        assert_eq!(sondar_trava(dir.path()), Trava::Livre);
    }
}
