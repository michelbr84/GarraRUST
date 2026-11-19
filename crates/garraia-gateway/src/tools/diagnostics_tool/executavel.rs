//! `versao.executavel` — o executavel CARREGADO e o arquivo INSTALADO sao o
//! mesmo build? (#1914)
//!
//! `versao.instalada` diz `CARGO_PKG_VERSION`, e isso nao basta: a versao do
//! crate nao muda entre candidatos da mesma release (rc5 e rc6 saem os dois
//! como `0.5.2`), e um `garra update` troca o arquivo em disco sem trocar o
//! processo que esta rodando. A unica evidencia que distingue os dois casos e
//! o conteudo — o SHA-256 — de cada um, lido por interface nativa:
//!
//! - **carregado**: a imagem que o kernel mapeou para ESTE processo. No
//!   Linux/Android e `/proc/self/exe`, que abre o inode em uso mesmo depois
//!   de o arquivo ter sido substituido ou apagado. Nas outras plataformas nao
//!   ha handle equivalente, e a observacao sai `nao_suportado` — nunca o hash
//!   do arquivo em disco fingindo ser o do processo.
//! - **instalado**: o arquivo que esta HOJE no caminho de onde o processo
//!   saiu (`std::env::current_exe`). No Linux esse caminho vem do mesmo link
//!   do procfs, que ganha o sufixo ` (deleted)` quando o arquivo original foi
//!   substituido por rename — o sufixo e tirado para chegar ao arquivo novo.
//!
//! ## O que conta como prova
//!
//! So dois hashes LIDOS e iguais dao `ok`. Leitura negada (procfs bloqueado,
//! permissao) e `nao_verificado` com o motivo, e `iguais` fica `null`: falta
//! de leitura nunca e lida como igualdade, e sai `erro` — como toda leitura
//! que falhou neste relatorio. Os hashes passam pelo redator como o resto da
//! evidencia: ele isenta hash hexadecimal de proposito
//! (`garraia_security::mascarar_numeros_longos`), e um teste aqui prova que o
//! digest sai inteiro. Nenhum caminho sai no relatorio.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use sha2::{Digest, Sha256};

use super::{EstadoDiag, Verificacao};

/// Onde ler cada lado. Separado da leitura para os testes apontarem para
/// arquivos pequenos: o binario de teste em debug tem centenas de MB.
#[derive(Debug, Clone)]
pub(crate) struct FonteDoExecutavel {
    /// `None` = a plataforma nao tem handle para a imagem carregada.
    pub carregado: Option<PathBuf>,
    /// `Err` = nao deu para descobrir o caminho do executavel.
    pub instalado: std::result::Result<PathBuf, &'static str>,
}

impl FonteDoExecutavel {
    /// A fonte real deste processo.
    pub(crate) fn deste_processo() -> Self {
        let carregado = if cfg!(any(target_os = "linux", target_os = "android")) {
            Some(PathBuf::from("/proc/self/exe"))
        } else {
            None
        };
        let instalado = std::env::current_exe()
            .map(|p| caminho_instalado(&p))
            .map_err(|_| "caminho_desconhecido");
        Self {
            carregado,
            instalado,
        }
    }
}

/// O sufixo que o Linux poe no link `/proc/self/exe` quando o arquivo de onde
/// o processo saiu foi apagado ou substituido por rename.
const SUFIXO_APAGADO: &str = " (deleted)";

/// O caminho do arquivo instalado HOJE: tira o ` (deleted)` do link do
/// procfs, mas so quando o caminho com o sufixo nao existe — um arquivo que
/// se chame literalmente assim continua sendo ele mesmo.
pub(crate) fn caminho_instalado(exe: &Path) -> PathBuf {
    if !exe.exists()
        && let Some(texto) = exe.to_str()
        && let Some(sem) = texto.strip_suffix(SUFIXO_APAGADO)
    {
        return PathBuf::from(sem);
    }
    exe.to_path_buf()
}

/// O que se sabe de UM lado.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Observacao {
    Verificado { sha256: String },
    NaoVerificado { motivo: &'static str },
    NaoSuportado,
}

impl Observacao {
    fn sha256(&self) -> Option<&str> {
        match self {
            Self::Verificado { sha256 } => Some(sha256),
            _ => None,
        }
    }

    fn evidencia(&self) -> serde_json::Value {
        match self {
            Self::Verificado { sha256 } => {
                serde_json::json!({ "estado": "verificado", "sha256": sha256 })
            }
            Self::NaoVerificado { motivo } => {
                serde_json::json!({ "estado": "nao_verificado", "motivo": motivo })
            }
            Self::NaoSuportado => serde_json::json!({ "estado": "nao_suportado" }),
        }
    }
}

/// Motivo fechado a partir do erro de I/O — nunca o texto do erro, que pode
/// citar caminho.
fn motivo_do_erro(erro: &std::io::Error) -> &'static str {
    match erro.kind() {
        std::io::ErrorKind::PermissionDenied => "acesso_negado",
        std::io::ErrorKind::NotFound => "nao_encontrado",
        _ => "falha_de_leitura",
    }
}

/// SHA-256 do arquivo inteiro, em blocos (o binario de release passa de
/// dezenas de MB).
pub(crate) fn sha256_do_arquivo(caminho: &Path) -> Observacao {
    let mut arquivo = match std::fs::File::open(caminho) {
        Ok(a) => a,
        Err(e) => {
            return Observacao::NaoVerificado {
                motivo: motivo_do_erro(&e),
            };
        }
    };
    let mut hasher = Sha256::new();
    let mut bloco = vec![0u8; 64 * 1024];
    loop {
        match arquivo.read(&mut bloco) {
            Ok(0) => break,
            Ok(n) => hasher.update(&bloco[..n]),
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
            Err(e) => {
                return Observacao::NaoVerificado {
                    motivo: motivo_do_erro(&e),
                };
            }
        }
    }
    Observacao::Verificado {
        sha256: hex::encode(hasher.finalize()),
    }
}

/// Le os dois lados. O carregado nao muda enquanto o processo vive, entao o
/// hash LIDO com sucesso fica em `cache` (falha nao entra: pode ser
/// transitoria). O instalado e lido a cada chamada — e ele que muda num
/// `garra update`.
pub(crate) fn observar(
    fonte: &FonteDoExecutavel,
    cache: &OnceLock<String>,
) -> (Observacao, Observacao) {
    let carregado = match (&fonte.carregado, cache.get()) {
        (None, _) => Observacao::NaoSuportado,
        (Some(_), Some(sha256)) => Observacao::Verificado {
            sha256: sha256.clone(),
        },
        (Some(caminho), None) => {
            let obs = sha256_do_arquivo(caminho);
            if let Observacao::Verificado { sha256 } = &obs {
                let _ = cache.set(sha256.clone());
            }
            obs
        }
    };
    let instalado = match &fonte.instalado {
        Ok(caminho) => sha256_do_arquivo(caminho),
        Err(motivo) => Observacao::NaoVerificado { motivo },
    };
    (carregado, instalado)
}

/// A entrada `versao.executavel` do relatorio.
pub(crate) fn verificacao(carregado: &Observacao, instalado: &Observacao) -> Verificacao {
    let iguais = match (carregado.sha256(), instalado.sha256()) {
        (Some(a), Some(b)) => Some(a == b),
        _ => None,
    };
    let (estado, detalhe) = match (iguais, carregado, instalado) {
        (Some(true), _, _) => (
            EstadoDiag::Ok,
            "O executavel em execucao e o arquivo instalado tem o mesmo SHA-256: o processo roda \
             o build que esta em disco.",
        ),
        (Some(false), _, _) => (
            EstadoDiag::Degradado,
            "O arquivo instalado difere do executavel em execucao (SHA-256 diferentes): o binario \
             foi trocado depois que este processo subiu. Reinicie o gateway para rodar o build \
             instalado.",
        ),
        (None, Observacao::NaoSuportado, Observacao::Verificado { .. }) => (
            EstadoDiag::NaoSuportado,
            "Esta plataforma nao expoe a imagem carregada do processo: so o arquivo instalado foi \
             lido. Nao ha prova de que o processo roda esse mesmo build.",
        ),
        (None, Observacao::NaoSuportado, _) => (
            EstadoDiag::Erro,
            "Nao verificado: esta plataforma nao expoe a imagem carregada e o arquivo instalado \
             nao pode ser lido (veja `motivo`).",
        ),
        (None, _, _) => (
            EstadoDiag::Erro,
            "Nao verificado: ao menos um dos lados nao pode ser lido (veja `motivo`). Leitura \
             negada nao prova que os builds sao iguais.",
        ),
    };
    Verificacao::nova(
        "versao.executavel",
        estado,
        serde_json::json!({
            "carregado": carregado.evidencia(),
            "instalado": instalado.evidencia(),
            "iguais": iguais,
        }),
        detalhe,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn gravar(dir: &Path, nome: &str, conteudo: &[u8]) -> PathBuf {
        let p = dir.join(nome);
        std::fs::write(&p, conteudo).expect("grava");
        p
    }

    fn fonte(carregado: Option<PathBuf>, instalado: PathBuf) -> FonteDoExecutavel {
        FonteDoExecutavel {
            carregado,
            instalado: Ok(instalado),
        }
    }

    #[test]
    fn sha256_confere_com_o_vetor_conhecido() {
        let dir = tempfile::tempdir().expect("tempdir");
        let p = gravar(dir.path(), "abc", b"abc");
        assert_eq!(
            sha256_do_arquivo(&p),
            Observacao::Verificado {
                sha256: "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad".into()
            }
        );
    }

    #[test]
    fn mesmo_build_e_ok_com_os_dois_hashes_intactos() {
        let dir = tempfile::tempdir().expect("tempdir");
        let a = gravar(dir.path(), "a", b"abc");
        let b = gravar(dir.path(), "b", b"abc");
        let (c, i) = observar(&fonte(Some(a), b), &OnceLock::new());
        let v = verificacao(&c, &i);
        assert_eq!(v.id, "versao.executavel");
        assert_eq!(v.estado, EstadoDiag::Ok);
        assert_eq!(v.evidencia["iguais"], true);
        let hash = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";
        assert_eq!(v.evidencia["carregado"]["sha256"], hash);
        assert_eq!(v.evidencia["instalado"]["sha256"], hash);
        assert_eq!(v.evidencia["carregado"]["estado"], "verificado");
    }

    #[test]
    fn binario_trocado_depois_do_boot_e_degradado_e_pede_restart() {
        let dir = tempfile::tempdir().expect("tempdir");
        let a = gravar(dir.path(), "a", b"build antigo");
        let b = gravar(dir.path(), "b", b"build novo");
        let (c, i) = observar(&fonte(Some(a), b), &OnceLock::new());
        let v = verificacao(&c, &i);
        assert_eq!(v.estado, EstadoDiag::Degradado);
        assert_eq!(v.evidencia["iguais"], false);
        assert!(v.detalhe.contains("Reinicie o gateway"), "{}", v.detalhe);
    }

    /// Antes/depois de uma atualizacao real: o carregado fica no cache, o
    /// instalado e relido — trocar o arquivo em disco aparece na proxima
    /// chamada sem restart.
    #[test]
    fn troca_do_arquivo_em_disco_aparece_na_chamada_seguinte() {
        let dir = tempfile::tempdir().expect("tempdir");
        let exe = gravar(dir.path(), "garraia", b"v1");
        let cache = OnceLock::new();
        let f = fonte(Some(exe.clone()), exe.clone());
        let (c, i) = observar(&f, &cache);
        assert_eq!(verificacao(&c, &i).estado, EstadoDiag::Ok);

        // `garra update` troca o arquivo por rename; o processo segue no v1.
        let novo = gravar(dir.path(), "garraia.new", b"v2");
        std::fs::rename(&novo, &exe).expect("rename");
        let (c, i) = observar(&f, &cache);
        let v = verificacao(&c, &i);
        assert_eq!(v.estado, EstadoDiag::Degradado, "{:?}", v.evidencia);
        assert_eq!(v.evidencia["iguais"], false);
    }

    #[test]
    fn leitura_negada_nao_e_prova_de_igualdade() {
        let dir = tempfile::tempdir().expect("tempdir");
        let b = gravar(dir.path(), "b", b"abc");
        let ausente = dir.path().join("nao-existe");
        let (c, i) = observar(&fonte(Some(ausente), b), &OnceLock::new());
        let v = verificacao(&c, &i);
        assert_eq!(v.estado, EstadoDiag::Erro);
        assert!(v.evidencia["iguais"].is_null());
        assert_eq!(v.evidencia["carregado"]["estado"], "nao_verificado");
        assert_eq!(v.evidencia["carregado"]["motivo"], "nao_encontrado");
        assert!(v.evidencia["carregado"].get("sha256").is_none());
        assert!(v.detalhe.contains("Nao verificado"), "{}", v.detalhe);
    }

    #[cfg(unix)]
    #[test]
    fn permissao_negada_vira_acesso_negado() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().expect("tempdir");
        let p = gravar(dir.path(), "fechado", b"abc");
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o000)).expect("chmod");
        // Root le mesmo sem permissao: ai o teste nao tem o que provar.
        if std::fs::File::open(&p).is_ok() {
            return;
        }
        assert_eq!(
            sha256_do_arquivo(&p),
            Observacao::NaoVerificado {
                motivo: "acesso_negado"
            }
        );
    }

    #[test]
    fn plataforma_sem_imagem_carregada_e_nao_suportado_sem_inventar_igualdade() {
        let dir = tempfile::tempdir().expect("tempdir");
        let b = gravar(dir.path(), "b", b"abc");
        let (c, i) = observar(&fonte(None, b), &OnceLock::new());
        assert_eq!(c, Observacao::NaoSuportado);
        let v = verificacao(&c, &i);
        assert_eq!(v.estado, EstadoDiag::NaoSuportado);
        assert!(v.evidencia["iguais"].is_null());
        assert!(v.evidencia["instalado"]["sha256"].is_string());
    }

    #[test]
    fn caminho_desconhecido_e_nao_verificado() {
        let dir = tempfile::tempdir().expect("tempdir");
        let a = gravar(dir.path(), "a", b"abc");
        let f = FonteDoExecutavel {
            carregado: Some(a),
            instalado: Err("caminho_desconhecido"),
        };
        let (c, i) = observar(&f, &OnceLock::new());
        let v = verificacao(&c, &i);
        assert_eq!(v.estado, EstadoDiag::Erro);
        assert_eq!(v.evidencia["instalado"]["motivo"], "caminho_desconhecido");
    }

    #[test]
    fn so_sucesso_entra_no_cache_do_carregado() {
        let dir = tempfile::tempdir().expect("tempdir");
        let exe = dir.path().join("garraia");
        let cache = OnceLock::new();
        let f = fonte(Some(exe.clone()), exe.clone());
        let (c, _) = observar(&f, &cache);
        assert!(matches!(c, Observacao::NaoVerificado { .. }));
        assert!(cache.get().is_none(), "falha nao pode ficar no cache");
        gravar(dir.path(), "garraia", b"abc");
        let (c, _) = observar(&f, &cache);
        assert!(matches!(c, Observacao::Verificado { .. }));
        assert!(cache.get().is_some());
    }

    #[test]
    fn sufixo_deleted_so_sai_quando_o_caminho_com_ele_nao_existe() {
        let dir = tempfile::tempdir().expect("tempdir");
        let real = dir.path().join("garraia");
        let link = PathBuf::from(format!("{}{SUFIXO_APAGADO}", real.display()));
        assert_eq!(caminho_instalado(&link), real);
        // Arquivo que se chama literalmente assim continua sendo ele.
        std::fs::write(&link, b"x").expect("grava");
        assert_eq!(caminho_instalado(&link), link);
        // Caminho comum passa intocado.
        assert_eq!(caminho_instalado(&real), real);
    }

    #[test]
    fn nenhum_caminho_sai_na_evidencia_nem_no_detalhe() {
        let dir = tempfile::tempdir().expect("tempdir");
        let a = gravar(dir.path(), "a", b"abc");
        let ausente = dir.path().join("nao-existe");
        let (c, i) = observar(&fonte(Some(a), ausente), &OnceLock::new());
        let v = verificacao(&c, &i);
        let texto = format!("{} {}", v.evidencia, v.detalhe);
        // O ultimo componente do tempdir: nao depende do escape de `\\` que
        // o JSON aplica aos caminhos do Windows.
        let nome = dir
            .path()
            .file_name()
            .expect("tempdir tem nome")
            .to_string_lossy()
            .into_owned();
        assert!(!texto.contains(&nome), "{texto}");
        assert!(!texto.contains("nao-existe"), "{texto}");
    }

    /// O hash passa pelo redator do relatorio (`Verificacao::nova`) e sai
    /// inteiro: o redator isenta hex, inclusive com corrido longo de digitos.
    #[test]
    fn redator_nao_altera_o_digest() {
        let digest = format!("{}{}", "1234567890".repeat(6), "abcd");
        let obs = Observacao::Verificado {
            sha256: digest.clone(),
        };
        let v = verificacao(&obs, &obs);
        assert_eq!(v.evidencia["carregado"]["sha256"], digest.as_str());
        assert_eq!(v.evidencia["instalado"]["sha256"], digest.as_str());
    }

    #[test]
    fn sem_imagem_carregada_e_instalado_ilegivel_e_erro() {
        let dir = tempfile::tempdir().expect("tempdir");
        let (c, i) = observar(&fonte(None, dir.path().join("x")), &OnceLock::new());
        let v = verificacao(&c, &i);
        assert_eq!(v.estado, EstadoDiag::Erro);
        assert!(
            v.detalhe.contains("nao expoe a imagem carregada"),
            "{}",
            v.detalhe
        );
    }

    #[cfg(not(any(target_os = "linux", target_os = "android")))]
    #[test]
    fn fora_do_linux_nao_ha_imagem_carregada() {
        assert_eq!(FonteDoExecutavel::deste_processo().carregado, None);
    }

    #[cfg(any(target_os = "linux", target_os = "android"))]
    #[test]
    fn no_linux_o_carregado_e_o_procfs() {
        let f = FonteDoExecutavel::deste_processo();
        assert_eq!(f.carregado, Some(PathBuf::from("/proc/self/exe")));
        assert!(f.instalado.is_ok());
    }
}
