//! Confinamento das chamadas MCP de **filesystem** ao jail da sessao.
//!
//! O `ToolGate` decide por **nome** de ferramenta e nunca ve o argumento
//! `path`. Isso bastava enquanto nenhum modo restrito enxergava o servidor
//! `filesystem`; com a #1384 o piso `search` passou a ler por ele, e a raiz
//! desse servidor em `standard` e `<data_dir>/workspace` — o **pai** de todo
//! diretorio de sessao da #1449. Sem esta camada, um contato admitido lista o
//! pai e le o que o agente escreveu para outra pessoa.
//!
//! A regra e a das file tools nativas, sem copia: cada argumento de caminho
//! das operacoes conhecidas passa por [`FileJail::confine`] com o
//! `working_dir` da sessao; o que sai e o caminho **resolvido**, que e o que
//! vai para o servidor. `list_allowed_directories` nem chega ao servidor —
//! responder "as raizes do servidor" seria entregar o pai — e sai daqui com
//! as raizes efetivas **da sessao**.
//!
//! Casa pelo nome da **operacao** (`read_file`, `write_file`…) em qualquer
//! servidor, pela mesma razao que a `allowed` do `search` libera `*/read_file`
//! em qualquer servidor: a operacao e que e de filesystem. Operacao
//! desconhecida passa intocada — o portao de nomes continua sendo quem decide
//! se ela roda.

use std::path::{Path, PathBuf};

use serde_json::{Map, Value};

use crate::tools::file_jail::{Denial, FileJail};

/// Operacoes do `@modelcontextprotocol/server-filesystem` que recebem
/// caminho, e em quais argumentos. Fechada por nome, como
/// [`crate::modes::LEITURA_MCP_FILESYSTEM`].
pub const OPERACOES_COM_CAMINHO: &[(&str, &[&str])] = &[
    ("read_file", &["path"]),
    ("read_text_file", &["path"]),
    ("read_media_file", &["path"]),
    ("read_multiple_files", &["paths"]),
    ("list_directory", &["path"]),
    ("list_directory_with_sizes", &["path"]),
    ("directory_tree", &["path"]),
    ("search_files", &["path"]),
    ("get_file_info", &["path"]),
    ("write_file", &["path"]),
    ("edit_file", &["path"]),
    ("create_directory", &["path"]),
    ("move_file", &["source", "destination"]),
];

/// A operacao que responde as raizes: sai daqui, nunca do servidor.
pub const LIST_ALLOWED_DIRECTORIES: &str = "list_allowed_directories";

/// O desfecho de [`confinar_argumentos`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Confinado {
    /// Operacao sem caminho, ou desconhecida: os argumentos seguem como vieram.
    Passa(Option<Map<String, Value>>),
    /// Cada caminho substituido pelo resolvido dentro do jail.
    Reescrito(Map<String, Value>),
    /// A resposta e local (`list_allowed_directories`): este texto, e o
    /// servidor nao e chamado.
    RespondeLocal(String),
}

/// Confina os argumentos de caminho de `operacao` ao jail da sessao.
///
/// Fail-closed: um unico caminho fora (ou irresoluvel, ou relativo sem
/// diretorio de sessao) recusa a chamada inteira com a [`Denial`] das tools
/// nativas — e a mesma frase unica para o modelo.
pub fn confinar_argumentos(
    operacao: &str,
    argumentos: Option<Map<String, Value>>,
    jail: &FileJail,
    session_dir: Option<&str>,
) -> Result<Confinado, Denial> {
    if operacao == LIST_ALLOWED_DIRECTORIES {
        let raizes = jail.effective_roots(session_dir);
        if raizes.is_empty() {
            return Err(Denial::NoRoots);
        }
        let texto = raizes
            .iter()
            .map(|r| r.display().to_string())
            .collect::<Vec<_>>()
            .join("\n");
        return Ok(Confinado::RespondeLocal(texto));
    }
    let Some(chaves) = OPERACOES_COM_CAMINHO
        .iter()
        .find(|(op, _)| *op == operacao)
        .map(|(_, chaves)| *chaves)
    else {
        return Ok(Confinado::Passa(argumentos));
    };
    // Operacao de filesystem sem argumento de caminho e um pedido malformado:
    // recusa, em vez de deixar o servidor resolver contra o cwd dele.
    let mut mapa = argumentos.ok_or(Denial::Unresolvable)?;
    for chave in chaves {
        let novo = match mapa.get(*chave).ok_or(Denial::Unresolvable)? {
            Value::String(pedido) => Value::String(confinar_um(pedido, jail, session_dir)?),
            Value::Array(itens) => Value::Array(
                itens
                    .iter()
                    .map(|item| match item {
                        Value::String(pedido) => {
                            confinar_um(pedido, jail, session_dir).map(Value::String)
                        }
                        _ => Err(Denial::Unresolvable),
                    })
                    .collect::<Result<Vec<_>, _>>()?,
            ),
            _ => return Err(Denial::Unresolvable),
        };
        mapa.insert((*chave).to_string(), novo);
    }
    Ok(Confinado::Reescrito(mapa))
}

/// Um caminho, confinado: o que o modelo pediu vira o resolvido dentro do
/// jail, ou a recusa. Relativo sem diretorio de sessao e "sem raiz".
fn confinar_um(pedido: &str, jail: &FileJail, session_dir: Option<&str>) -> Result<String, Denial> {
    let caminho = caminho_pedido(pedido, session_dir).ok_or(Denial::NoRoots)?;
    Ok(jail
        .confine(&caminho, session_dir)?
        .to_string_lossy()
        .into_owned())
}

/// Um caminho do modelo, absoluto ou relativo ao diretorio da sessao.
fn caminho_pedido(pedido: &str, session_dir: Option<&str>) -> Option<PathBuf> {
    let p = Path::new(pedido);
    if p.is_absolute() {
        return Some(p.to_path_buf());
    }
    session_dir
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| Path::new(s).join(p))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn jail_so_sessao() -> FileJail {
        FileJail::sessions_only()
    }

    fn args(pares: &[(&str, Value)]) -> Map<String, Value> {
        pares
            .iter()
            .map(|(k, v)| (k.to_string(), v.clone()))
            .collect()
    }

    fn sessao() -> (tempfile::TempDir, PathBuf, PathBuf) {
        let tmp = tempfile::tempdir().expect("tempdir");
        let workspace = tmp.path().join("workspace");
        let a = workspace.join("sessao-a");
        let b = workspace.join("sessao-b");
        std::fs::create_dir_all(&a).expect("a");
        std::fs::create_dir_all(&b).expect("b");
        std::fs::write(a.join("meu.txt"), "meu").expect("meu");
        std::fs::write(b.join("segredo.txt"), "segredo").expect("segredo");
        (tmp, a, b)
    }

    fn canon(p: &Path) -> String {
        std::fs::canonicalize(p)
            .expect("canonicalize")
            .to_string_lossy()
            .into_owned()
    }

    /// **O achado.** A sessao A pede o arquivo da sessao B pelo caminho
    /// absoluto (que `list_allowed_directories` do servidor entregaria): fora.
    #[test]
    fn sessao_a_nao_le_o_diretorio_da_sessao_b() {
        let (_tmp, a, b) = sessao();
        let a_str = a.to_string_lossy().into_owned();
        for op in [
            "read_file",
            "read_text_file",
            "list_directory",
            "directory_tree",
            "search_files",
            "get_file_info",
        ] {
            let r = confinar_argumentos(
                op,
                Some(args(&[(
                    "path",
                    Value::String(b.join("segredo.txt").to_string_lossy().into_owned()),
                )])),
                &jail_so_sessao(),
                Some(&a_str),
            );
            assert!(matches!(r, Err(Denial::Outside)), "{op}: {r:?}");
        }
        // Nem o pai de todas as sessoes.
        let pai = b.parent().expect("pai").to_string_lossy().into_owned();
        let r = confinar_argumentos(
            "list_directory",
            Some(args(&[("path", Value::String(pai))])),
            &jail_so_sessao(),
            Some(&a_str),
        );
        assert!(matches!(r, Err(Denial::Outside)), "{r:?}");
    }

    /// Dentro do proprio diretorio o caminho sai **resolvido** (canonico): e
    /// ele que vai para o servidor, senao a resolucao nao vale nada.
    #[test]
    fn dentro_da_sessao_o_caminho_e_reescrito_pelo_resolvido() {
        let (_tmp, a, _b) = sessao();
        let a_str = a.to_string_lossy().into_owned();
        let r = confinar_argumentos(
            "read_text_file",
            Some(args(&[
                (
                    "path",
                    Value::String(a.join("meu.txt").to_string_lossy().into_owned()),
                ),
                ("head", Value::from(10)),
            ])),
            &jail_so_sessao(),
            Some(&a_str),
        )
        .expect("dentro");
        let Confinado::Reescrito(m) = r else {
            panic!("{r:?}")
        };
        assert_eq!(m["path"], Value::String(canon(&a.join("meu.txt"))));
        assert_eq!(m["head"], Value::from(10), "os outros argumentos ficam");
    }

    /// Caminho relativo e relativo ao diretorio da sessao — nunca ao cwd do
    /// servidor. Sem diretorio de sessao, relativo nao resolve.
    #[test]
    fn relativo_resolve_contra_o_diretorio_da_sessao_e_sem_sessao_nao_resolve() {
        let (_tmp, a, _b) = sessao();
        let a_str = a.to_string_lossy().into_owned();
        let r = confinar_argumentos(
            "read_file",
            Some(args(&[("path", Value::String("meu.txt".into()))])),
            &jail_so_sessao(),
            Some(&a_str),
        )
        .expect("relativo");
        let Confinado::Reescrito(m) = r else {
            panic!("{r:?}")
        };
        assert_eq!(m["path"], Value::String(canon(&a.join("meu.txt"))));

        let r = confinar_argumentos(
            "read_file",
            Some(args(&[("path", Value::String("meu.txt".into()))])),
            &jail_so_sessao(),
            None,
        );
        assert!(matches!(r, Err(Denial::NoRoots)), "{r:?}");
    }

    /// `..` e recusado como nas tools nativas.
    #[test]
    fn ponto_ponto_e_recusado() {
        let (_tmp, a, _b) = sessao();
        let a_str = a.to_string_lossy().into_owned();
        let r = confinar_argumentos(
            "read_file",
            Some(args(&[(
                "path",
                Value::String("../sessao-b/segredo.txt".into()),
            )])),
            &jail_so_sessao(),
            Some(&a_str),
        );
        assert!(matches!(r, Err(Denial::Outside)), "{r:?}");
    }

    /// Lista de caminhos: um fora recusa a chamada inteira.
    #[test]
    fn lista_de_caminhos_com_um_fora_recusa_tudo() {
        let (_tmp, a, b) = sessao();
        let a_str = a.to_string_lossy().into_owned();
        let dentro = a.join("meu.txt").to_string_lossy().into_owned();
        let fora = b.join("segredo.txt").to_string_lossy().into_owned();
        let r = confinar_argumentos(
            "read_multiple_files",
            Some(args(&[(
                "paths",
                Value::Array(vec![Value::String(dentro.clone()), Value::String(fora)]),
            )])),
            &jail_so_sessao(),
            Some(&a_str),
        );
        assert!(matches!(r, Err(Denial::Outside)), "{r:?}");
        let r = confinar_argumentos(
            "read_multiple_files",
            Some(args(&[(
                "paths",
                Value::Array(vec![Value::String(dentro)]),
            )])),
            &jail_so_sessao(),
            Some(&a_str),
        )
        .expect("so dentro");
        let Confinado::Reescrito(m) = r else {
            panic!("{r:?}")
        };
        assert_eq!(
            m["paths"],
            Value::Array(vec![Value::String(canon(&a.join("meu.txt")))])
        );
    }

    /// `move_file` tem dois caminhos; os dois precisam estar dentro.
    #[test]
    fn mover_para_fora_e_recusado() {
        let (_tmp, a, b) = sessao();
        let a_str = a.to_string_lossy().into_owned();
        let r = confinar_argumentos(
            "move_file",
            Some(args(&[
                (
                    "source",
                    Value::String(a.join("meu.txt").to_string_lossy().into_owned()),
                ),
                (
                    "destination",
                    Value::String(b.join("roubado.txt").to_string_lossy().into_owned()),
                ),
            ])),
            &jail_so_sessao(),
            Some(&a_str),
        );
        assert!(matches!(r, Err(Denial::Outside)), "{r:?}");
    }

    /// `list_allowed_directories` responde as raizes DA SESSAO, e o servidor
    /// nao e chamado — a resposta dele seria o pai.
    #[test]
    fn list_allowed_directories_responde_as_raizes_da_sessao() {
        let (_tmp, a, b) = sessao();
        let a_str = a.to_string_lossy().into_owned();
        let r = confinar_argumentos(
            LIST_ALLOWED_DIRECTORIES,
            None,
            &jail_so_sessao(),
            Some(&a_str),
        )
        .expect("local");
        let Confinado::RespondeLocal(texto) = r else {
            panic!("{r:?}")
        };
        assert!(texto.contains(&canon(&a)), "{texto}");
        assert!(!texto.contains("sessao-b"), "{texto}");
        let pai = b.parent().expect("pai");
        assert!(
            !texto.contains(&format!("{}\n", canon(pai)))
                && !texto.trim_end().ends_with(&canon(pai)),
            "o pai vazou: {texto}"
        );
        // Sem raiz nenhuma: a frase e a de "sem raiz", nao uma lista vazia.
        let r = confinar_argumentos(LIST_ALLOWED_DIRECTORIES, None, &jail_so_sessao(), None);
        assert!(matches!(r, Err(Denial::NoRoots)), "{r:?}");
    }

    /// Operacao desconhecida (ou de outro servidor) passa intocada: quem
    /// decide se ela roda e o portao de nomes.
    #[test]
    fn operacao_desconhecida_passa_intocada() {
        let (_tmp, a, _b) = sessao();
        let a_str = a.to_string_lossy().into_owned();
        let entrada = args(&[("query", Value::String("x".into()))]);
        let r = confinar_argumentos(
            "search_issues",
            Some(entrada.clone()),
            &jail_so_sessao(),
            Some(&a_str),
        )
        .expect("passa");
        assert_eq!(r, Confinado::Passa(Some(entrada)));
        let r = confinar_argumentos("ping", None, &jail_so_sessao(), None).expect("passa");
        assert_eq!(r, Confinado::Passa(None));
    }

    /// Raiz declarada (`agent.file_roots`) vale para o MCP como para as
    /// nativas: dentro dela passa mesmo sem diretorio de sessao.
    #[test]
    fn raiz_declarada_vale_como_nas_tools_nativas() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let projeto = tmp.path().join("projeto");
        std::fs::create_dir_all(&projeto).expect("mkdir");
        std::fs::write(projeto.join("README.md"), "x").expect("write");
        let jail = FileJail::from_roots([&projeto]);
        let r = confinar_argumentos(
            "read_file",
            Some(args(&[(
                "path",
                Value::String(projeto.join("README.md").to_string_lossy().into_owned()),
            )])),
            &jail,
            None,
        )
        .expect("dentro da declarada");
        assert!(matches!(r, Confinado::Reescrito(_)), "{r:?}");
        let r = confinar_argumentos(
            "read_file",
            Some(args(&[(
                "path",
                Value::String(tmp.path().join("fora.txt").to_string_lossy().into_owned()),
            )])),
            &jail,
            None,
        );
        assert!(matches!(r, Err(Denial::Outside)), "{r:?}");
    }

    /// **A fiacao.** `McpTool::execute` confina ANTES de chamar o servidor —
    /// e o unico ponto por onde uma chamada MCP passa.
    #[test]
    fn a_ponte_confina_antes_de_chamar_o_servidor() {
        let ponte = include_str!("tool_bridge.rs");
        let confina = ponte
            .find("confinar_argumentos(")
            .expect("tool_bridge.rs deixou de confinar (#1482)");
        let chama = ponte
            .find("call_tool_once(")
            .expect("tool_bridge.rs chama o servidor");
        assert!(
            confina < chama,
            "o confinamento tem de vir antes da chamada (#1482)"
        );
        assert!(
            ponte.contains("jail_das_file_tools()"),
            "a ponte tem de ler o jail do manager (#1482)"
        );
    }

    /// Argumento de caminho que nao e string (ou falta): recusa, nao passa.
    #[test]
    fn caminho_ausente_ou_nao_string_e_recusado() {
        let (_tmp, a, _b) = sessao();
        let a_str = a.to_string_lossy().into_owned();
        let r = confinar_argumentos(
            "read_file",
            Some(args(&[("path", Value::from(7))])),
            &jail_so_sessao(),
            Some(&a_str),
        );
        assert!(matches!(r, Err(Denial::Unresolvable)), "{r:?}");
        let r = confinar_argumentos("read_file", None, &jail_so_sessao(), Some(&a_str));
        assert!(matches!(r, Err(Denial::Unresolvable)), "{r:?}");
    }
}
