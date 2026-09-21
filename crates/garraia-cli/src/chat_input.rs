//! Entrada de linha do REPL do `garra chat` (#1297).
//!
//! Ate aqui o loop lia com `BufRead::read_line` em modo canonico, e quem
//! editava a linha era o driver do terminal: seta para cima imprimia `^[[A`,
//! nao havia historico nem edicao no meio da linha. Este modulo poe um editor
//! de linha (`rustyline`) **so** no caminho em que ha um humano num terminal,
//! e mantem o `read_line` byte a byte em todo o resto — `echo pergunta |
//! garra chat`, CI e os testes de integracao dependem desse caminho continuar
//! identico.
//!
//! # Duas fontes, uma decisao
//!
//! [`editor_de_linha_cabe`] decide uma vez, no boot, e a fonte nao troca no
//! meio da sessao. Exige stdin, stdout **e** stderr em terminal: stdin porque
//! e de onde as teclas vem; stdout porque e onde o editor desenha, e uma
//! saida redirecionada para arquivo ou pipe receberia sequencia de escape a
//! cada tecla; stderr por ser o criterio de "interativo" que o resto da CLI
//! ja usa (`offer_pull_ollama_model`). `NO_COLOR` e `TERM=dumb` desligam cor e
//! animacao, nao o editor: seta e historico nao sao enfeite.
//!
//! # Dono unico do SIGINT
//!
//! O REPL tem um unico handler de sinal, o vigia em `run_chat`. Este modulo
//! nao registra nenhum — e isso depende da feature `signal-hook` do rustyline
//! estar **ligada** (ha teste varrendo o `Cargo.toml` do workspace). Sem ela
//! o rustyline instala um `sigaction(SIGINT)` proprio a cada `readline` e
//! restaura o anterior ao sair, disputando o sinal com o vigia: um `kill
//! -INT` no prompt nunca chegaria ao tokio, e se o `sigaction` do rustyline
//! vencesse a corrida do primeiro prompt, o restore no fim daquele `readline`
//! desengancharia o handler do tokio pelo resto do processo. Com a feature, o
//! rustyline so registra SIGWINCH, pelo `signal-hook-registry` que o proprio
//! tokio usa (encadeia, nao substitui).
//!
//! Em raw mode o terminal deixa de gerar SIGINT no Ctrl+C (`ISIG` desligado)
//! e a tecla chega ao editor, que a devolve como [`Leitura::Interrompida`];
//! quem chama decide, e o `run_chat` faz o que sempre fez com Ctrl+C no
//! prompt ocioso: encerra com 130. Um SIGINT externo (`kill -INT`) com o
//! editor em raw mode vai ao vigia, que devolve o terminal fotografado antes
//! de sair com 130. Durante o turno o terminal esta em modo canonico (o
//! `readline` ja devolveu), o Ctrl+C vira SIGINT e o vigia cancela o turno,
//! como antes.
//!
//! # Prompt com cor
//!
//! O rustyline so desenha o prompt estilizado quando ha um `Helper`: e o
//! `Highlighter` dele que recebe `prompt.styled()`, e sem helper o renderer
//! escreve `prompt.raw()`. `DefaultEditor::with_config` nasce sem helper, por
//! isso [`abrir_editor`] liga o `()` — todos os defaults, e o unico efeito e o
//! prompt verde aparecer.
//!
//! # Historico
//!
//! Em memoria durante a sessao, sempre. Em disco **so** quando a sessao e
//! persistida (`--persist`/`--resume`): o contrato do `--persist` (#1088) e
//! "sem ele nada e escrito", e o que se digita num chat e tao sensivel quanto
//! a resposta. Quando gravado, mora em `garraia_dir()/history` (nunca
//! `~/.garra`), criado com `0600` no Unix. Falha ao carregar ou gravar e
//! **fail-soft**: vira aviso pela mesma moldura do renderer e o REPL segue
//! sem historico persistente.

use std::fs::OpenOptions;
use std::io::{self, BufRead as _, Write as _};
use std::path::{Path, PathBuf};

use rustyline::error::ReadlineError;

/// Quantas linhas o historico persistente guarda. O default do rustyline
/// (100) e pouco para um chat que se retoma por dias; mil linhas curtas cabem
/// em dezenas de KB.
const TAMANHO_DO_HISTORICO: usize = 1000;

/// Nome do arquivo de historico dentro de `garraia_dir()`.
const NOME_DO_HISTORICO: &str = "history";

/// Usar o editor de linha? Decisao pura, tomada uma vez no boot.
///
/// Os tres precisam ser terminal (ver o docblock do modulo). Qualquer pipe ou
/// redirecionamento cai no `read_line` cru, que e o contrato que scripts e
/// testes conhecem.
pub(crate) fn editor_de_linha_cabe(stdin_tty: bool, stdout_tty: bool, stderr_tty: bool) -> bool {
    stdin_tty && stdout_tty && stderr_tty
}

/// Onde o historico persistente mora: `garraia_dir()/history`.
///
/// Recebe o diretorio em vez de resolve-lo para o teste nao depender do HOME
/// da maquina. Nunca `~/.garra`: pid, log e config ja vivem em
/// `garraia_dir()`, e um segundo diretorio oculto seria um lugar a mais para
/// o `doctor` esquecer.
pub(crate) fn caminho_do_historico(garraia_dir: &Path) -> PathBuf {
    garraia_dir.join(NOME_DO_HISTORICO)
}

/// O que uma leitura devolve.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Leitura {
    /// Uma linha sem tratamento — quem chama faz o `trim`, como sempre fez.
    Linha(String),
    /// Fim da entrada: Ctrl+D no terminal, EOF no pipe. Contrato: o cursor ja
    /// esta no inicio de uma linha nova.
    Fim,
    /// Ctrl+C no prompt, entregue pelo editor em raw mode. So acontece no
    /// caminho do editor — no `read_line` o Ctrl+C vira SIGINT e nunca chega
    /// aqui. Contrato: o cursor ja esta no inicio de uma linha nova.
    Interrompida,
}

enum Fonte {
    Editor {
        /// Em `Box` porque o editor pesa centenas de bytes e o outro braco,
        /// dezesseis — e o `Fonte` vive dentro do futuro do `run_chat`.
        editor: Box<rustyline::DefaultEditor>,
        /// `None` quando a sessao nao e persistida (#1088) ou quando o
        /// arquivo nao pode ser preparado, carregado ou gravado: o historico
        /// segue em memoria e o disco nao e tentado de novo, para nao avisar
        /// a cada linha.
        historico: Option<PathBuf>,
    },
    Plano(io::StdinLock<'static>),
}

/// A fonte de linhas do REPL, escolhida em [`LeitorDeLinha::abrir`].
pub(crate) struct LeitorDeLinha {
    fonte: Fonte,
    /// Avisos fail-soft acumulados desde o ultimo [`Self::drenar_avisos`].
    /// Este modulo nao conhece o renderer; quem chama e que os desenha.
    avisos: Vec<String>,
}

impl LeitorDeLinha {
    /// Constroi a fonte. `usar_editor` vem de [`editor_de_linha_cabe`];
    /// `historico` e `Some(`[`caminho_do_historico`]`)` so quando a sessao e
    /// persistida (#1088) — com `None` as setas funcionam igual, mas o
    /// historico morre com o processo. Nunca falha: sem editor, sem arquivo
    /// ou sem permissao, o resultado e um aviso e o caminho plano.
    pub(crate) fn abrir(usar_editor: bool, historico: Option<PathBuf>) -> Self {
        let mut avisos = Vec::new();
        let fonte = if usar_editor {
            abrir_editor(historico, &mut avisos)
        } else {
            Fonte::Plano(io::stdin().lock())
        };
        Self { fonte, avisos }
    }

    /// O editor esta de fato ligado? O vigia de SIGINT usa isto para saber se
    /// precisa fotografar o terminal antes de encerrar o processo.
    pub(crate) fn com_editor(&self) -> bool {
        matches!(self.fonte, Fonte::Editor { .. })
    }

    /// Entrega (e esvazia) os avisos pendentes.
    pub(crate) fn drenar_avisos(&mut self) -> Vec<String> {
        std::mem::take(&mut self.avisos)
    }

    /// Le uma linha. `prompt_cru` e `prompt_estilizado` sao o mesmo texto com
    /// e sem cor: o editor desenha o segundo — pelo helper `()` ligado em
    /// [`abrir_editor`]; sem helper o rustyline desenharia o cru — e mede o
    /// primeiro (escape nao ocupa coluna). O caminho plano imprime o
    /// estilizado, como o loop antigo fazia.
    ///
    /// `Err` e um erro de I/O de verdade — o mesmo que o `read_line` antigo
    /// propagava com `?` — e nao Ctrl+C nem Ctrl+D, que sao [`Leitura`].
    pub(crate) fn ler(&mut self, prompt_cru: &str, prompt_estilizado: &str) -> io::Result<Leitura> {
        match &mut self.fonte {
            Fonte::Editor { editor, historico } => {
                match editor.readline(&(prompt_cru, prompt_estilizado)) {
                    Ok(linha) => {
                        if !linha.trim().is_empty() {
                            guardar_no_historico(editor, historico, &linha, &mut self.avisos);
                        }
                        Ok(Leitura::Linha(linha))
                    }
                    Err(ReadlineError::Interrupted) => Ok(Leitura::Interrompida),
                    Err(ReadlineError::Eof) => Ok(Leitura::Fim),
                    // `ReadlineError::Signal(_)` nao chega aqui: com a feature
                    // `signal-hook` o unico sinal que o rustyline ve e SIGWINCH,
                    // e ele o trata por dentro (redesenha). O que sobra e I/O
                    // de verdade.
                    Err(e) => Err(io::Error::other(e)),
                }
            }
            Fonte::Plano(stdin) => {
                print!("{prompt_estilizado}");
                io::stdout().flush()?;
                let mut linha = String::new();
                if stdin.read_line(&mut linha)? == 0 {
                    // Ctrl+D em modo canonico nao ecoa quebra de linha; o
                    // editor, no braco de cima, ja deixa o cursor numa linha
                    // nova. Os dois cumprem o mesmo contrato de `Fim`.
                    println!();
                    return Ok(Leitura::Fim);
                }
                Ok(Leitura::Linha(linha))
            }
        }
    }
}

fn abrir_editor(historico: Option<PathBuf>, avisos: &mut Vec<String>) -> Fonte {
    let mut editor = match rustyline::DefaultEditor::with_config(configuracao()) {
        Ok(editor) => editor,
        Err(e) => {
            // Sem editor nao ha o que perder: o caminho antigo continua
            // funcionando, so sem setas.
            avisos.push(format!(
                "Editor de linha indisponivel ({e}); seguindo com a leitura simples, \
                 sem setas nem historico."
            ));
            return Fonte::Plano(io::stdin().lock());
        }
    };
    // Sem helper o rustyline desenha `prompt.raw()` e o estilizado que `ler`
    // passa e dado morto: `PosixRenderer::refresh_line` so chama
    // `highlight_prompt(prompt.styled())` quando ha um `Highlighter`, e
    // `with_config` nasce com `helper: None`. O `()` implementa `Helper` com
    // todos os defaults (`highlight_prompt` devolve o prompt como veio), entao
    // o unico efeito e o prompt verde aparecer; a medida do cursor continua
    // sendo `prompt.raw()`. Ha teste.
    editor.set_helper(Some(()));
    let historico =
        historico.and_then(
            |historico| match preparar_arquivo_de_historico(&historico) {
                Ok(()) => match editor.load_history(&historico) {
                    Ok(()) => Some(historico),
                    // Arquivo que nao carrega nao recebe gravacao: sobrescrever o que
                    // nao se conseguiu ler seria apagar o historico do usuario.
                    Err(e) => {
                        avisos.push(format!(
                            "Historico do chat em {} nao carregado ({e}); a sessao segue sem ele.",
                            historico.display()
                        ));
                        None
                    }
                },
                Err(e) => {
                    avisos.push(format!(
                        "Historico do chat em {} indisponivel ({e}); a sessao segue sem ele.",
                        historico.display()
                    ));
                    None
                }
            },
        );
    Fonte::Editor {
        editor: Box::new(editor),
        historico,
    }
}

fn configuracao() -> rustyline::Config {
    use rustyline::config::{Behavior, BellStyle};

    let base = rustyline::Config::builder()
        // O mesmo stdin/stdout que `editor_de_linha_cabe` inspecionou. O
        // default (`PreferTerm`) abre `/dev/tty` por conta propria, e ai o
        // editor leria do terminal mesmo com stdin redirecionado — o oposto
        // do que a decisao de boot prometeu.
        .behavior(Behavior::Stdio)
        // Linha comecando com espaco fica fora do historico: e a valvula
        // classica do shell para "isto nao guarda".
        .history_ignore_space(true)
        // Quem adiciona e `guardar_no_historico`, que pula linha vazia e
        // grava no disco na hora.
        .auto_add_history(false)
        // Um chat nao apita.
        .bell_style(BellStyle::None);
    // Os dois devolvem `Result` para recusar combinacoes invalidas; com
    // constantes fixas isso nao acontece, e se acontecesse o default do
    // rustyline (100 linhas, sem duplicata) ainda serve.
    base.clone()
        .max_history_size(TAMANHO_DO_HISTORICO)
        .and_then(|b| b.history_ignore_dups(true))
        .unwrap_or(base)
        .build()
}

/// Garante que o arquivo existe com modo `0600` sem tocar no conteudo.
///
/// O modo e fixado em dois momentos: no `open`, para o arquivo ja nascer
/// fechado (entre criar e apertar haveria uma janela com o umask do processo),
/// e num `harden` logo depois, para um arquivo que ja existia largo tambem
/// ser corrigido. O rustyline preserva o modo do arquivo existente ao gravar,
/// entao este e o unico ponto em que ele e decidido. No-op fora de Unix.
fn preparar_arquivo_de_historico(caminho: &Path) -> io::Result<()> {
    if let Some(pai) = caminho.parent() {
        std::fs::create_dir_all(pai)?;
    }
    let mut opcoes = OpenOptions::new();
    // `create` sem `truncate`: um historico que ja existe nao pode ser zerado
    // so por abrir o chat.
    opcoes.write(true).create(true).truncate(false);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt as _;
        opcoes.mode(garraia_common::fs_perms::SECRET_FILE_MODE);
    }
    opcoes.open(caminho)?;
    garraia_common::fs_perms::harden_secret_file(caminho)
}

/// Poe a linha no historico em memoria e, se houver arquivo, no disco.
///
/// Uma linha por vez, e nao tudo no fim: assim um `kill` ou uma queda no meio
/// da sessao nao leva o que ja foi digitado. `append` reconcilia com outra
/// sessao `garra chat` aberta ao mesmo tempo. A primeira falha de gravacao
/// vira aviso e desliga o disco para o resto da sessao.
fn guardar_no_historico(
    editor: &mut rustyline::DefaultEditor,
    historico: &mut Option<PathBuf>,
    linha: &str,
    avisos: &mut Vec<String>,
) {
    // `Err` aqui e um limite interno do rustyline, nao motivo para parar o
    // chat; `false` e duplicata ou linha com espaco na frente.
    let nova = editor.add_history_entry(linha).unwrap_or(false);
    if !nova {
        return;
    }
    let Some(caminho) = historico.take() else {
        return;
    };
    match editor.append_history(&caminho) {
        Ok(()) => *historico = Some(caminho),
        Err(e) => avisos.push(format!(
            "Historico do chat nao gravado em {} ({e}); a sessao segue sem gravar.",
            caminho.display()
        )),
    }
}

/// Atributos do terminal antes do REPL, para o vigia de SIGINT devolver antes
/// de encerrar o processo (ver o comentario do vigia em `run_chat`).
#[cfg(unix)]
pub(crate) type EstadoDoTerminal = nix::sys::termios::Termios;
/// Fora de Unix nao ha o que fotografar: a casca de console do Windows e
/// devolvida pelo proprio rustyline ao fim de cada `readline`, e o caminho do
/// `kill -INT` externo nao existe la.
#[cfg(not(unix))]
pub(crate) type EstadoDoTerminal = ();

/// `None` fora de Unix e quando stdin nao e terminal.
pub(crate) fn fotografar_terminal() -> Option<EstadoDoTerminal> {
    #[cfg(unix)]
    {
        nix::sys::termios::tcgetattr(io::stdin()).ok()
    }
    #[cfg(not(unix))]
    {
        None
    }
}

/// Devolve o terminal ao estado fotografado. Fail-soft: se o terminal ja se
/// foi, nao ha o que devolver, e o processo esta encerrando de qualquer jeito.
pub(crate) fn restaurar_terminal(estado: &EstadoDoTerminal) {
    #[cfg(unix)]
    {
        use nix::sys::termios::{SetArg, tcsetattr};
        let _ = tcsetattr(io::stdin(), SetArg::TCSANOW, estado);
    }
    #[cfg(not(unix))]
    {
        let _ = estado;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A decisao e pura e fecha para o lado do `read_line`: qualquer pipe ou
    /// redirecionamento entre os tres descritores desliga o editor.
    #[test]
    fn editor_so_quando_stdin_stdout_e_stderr_sao_terminal() {
        assert!(editor_de_linha_cabe(true, true, true));
        assert!(
            !editor_de_linha_cabe(false, true, true),
            "`echo pergunta | garra chat` tem de ficar no read_line byte a byte"
        );
        assert!(
            !editor_de_linha_cabe(true, false, true),
            "stdout em arquivo ou pipe nao pode receber escape do editor"
        );
        assert!(
            !editor_de_linha_cabe(true, true, false),
            "stderr redirecionado e o criterio de nao-interativo do resto da CLI"
        );
        assert!(!editor_de_linha_cabe(false, false, false));
    }

    #[test]
    fn historico_mora_em_garraia_dir_e_nao_em_ponto_garra() {
        let dir = PathBuf::from("base").join("garraia");
        let caminho = caminho_do_historico(&dir);
        assert_eq!(caminho, dir.join("history"));
        assert!(
            !caminho.to_string_lossy().contains(".garra"),
            "a issue pede garraia_dir(), nao ~/.garra: {caminho:?}"
        );
    }

    #[test]
    fn arquivo_de_historico_nasce_fechado_e_abrir_nao_trunca() {
        let tmp = tempfile::tempdir().expect("tempdir");
        // O pai ainda nao existe: e o caso da instalacao nova.
        let caminho = caminho_do_historico(&tmp.path().join("garraia"));

        preparar_arquivo_de_historico(&caminho).expect("preparar");
        assert!(caminho.is_file());
        if let Some(modo) = garraia_common::fs_perms::mode_of(&caminho).expect("mode") {
            assert_eq!(modo, 0o600, "historico de chat e material sensivel");
        }

        // Um arquivo que ja existia, com conteudo e modo largo: o conteudo
        // fica e o modo e corrigido.
        std::fs::write(&caminho, "#V2\nprimeira pergunta\n").expect("write");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&caminho, std::fs::Permissions::from_mode(0o644))
                .expect("loosen");
        }
        preparar_arquivo_de_historico(&caminho).expect("preparar de novo");
        assert_eq!(
            std::fs::read_to_string(&caminho).expect("read"),
            "#V2\nprimeira pergunta\n",
            "abrir o chat nao pode zerar o historico"
        );
        if let Some(modo) = garraia_common::fs_perms::mode_of(&caminho).expect("mode") {
            assert_eq!(modo, 0o600);
        }
    }

    /// Sem terminal a fonte e a antiga, e ela nao tem nada a avisar — e o
    /// caminho de todo teste de integracao que canaliza stdin.
    #[test]
    fn caminho_plano_nao_constroi_editor_nem_avisa() {
        let mut leitor = LeitorDeLinha::abrir(false, Some(PathBuf::from("irrelevante")));
        assert!(!leitor.com_editor());
        assert!(leitor.drenar_avisos().is_empty());
    }

    /// `DefaultEditor::with_config` nasce sem helper, e sem helper o
    /// rustyline desenha `prompt.raw()` — o prompt verde de
    /// `Style::user_prompt` nunca apareceria. O `()` ligado em `abrir_editor`
    /// e o que faz o `Highlighter` receber `prompt.styled()`.
    #[test]
    fn editor_tem_helper_para_o_prompt_estilizado_aparecer() {
        let leitor = LeitorDeLinha::abrir(true, None);
        let Fonte::Editor { editor, .. } = &leitor.fonte else {
            panic!("esperava a fonte com editor");
        };
        assert!(
            editor.helper().is_some(),
            "sem helper o rustyline ignora o prompt estilizado e desenha o cru"
        );
    }

    /// Sem `--persist`/`--resume` nada vai ao disco (#1088): o editor abre, as
    /// setas funcionam sobre o historico em memoria, e nao ha arquivo nem
    /// aviso — `historico: None` e o que desliga a gravacao em
    /// `guardar_no_historico`.
    #[test]
    fn sem_persistencia_o_historico_fica_so_em_memoria() {
        use rustyline::history::History as _;

        let mut leitor = LeitorDeLinha::abrir(true, None);
        assert!(leitor.com_editor(), "{:?}", leitor.drenar_avisos());
        assert!(leitor.drenar_avisos().is_empty());
        let Fonte::Editor { editor, historico } = &mut leitor.fonte else {
            panic!("esperava a fonte com editor");
        };
        assert!(
            historico.is_none(),
            "sem persistencia nao ha arquivo de historico"
        );

        guardar_no_historico(editor, historico, "primeira pergunta", &mut leitor.avisos);
        assert_eq!(editor.history().len(), 1, "as setas continuam funcionando");
        assert!(historico.is_none());
        assert!(leitor.avisos.is_empty(), "{:?}", leitor.avisos);
    }

    /// O invariante do docblock ("este modulo nao registra handler de
    /// sinal") so vale com a feature `signal-hook` do rustyline ligada: sem
    /// ela o rustyline instala `sigaction(SIGINT)` a cada `readline` e
    /// restaura o anterior ao sair, disputando o sinal com o vigia do
    /// `run_chat`. A feature e declarada no `Cargo.toml` do workspace, entao
    /// e ele que o teste varre.
    #[test]
    fn rustyline_entra_com_a_feature_signal_hook() {
        let manifesto = include_str!("../../../Cargo.toml");
        let linha = manifesto
            .lines()
            .find(|l| l.trim_start().starts_with("rustyline ="))
            .expect("rustyline declarado no Cargo.toml do workspace");
        assert!(
            linha.contains("\"signal-hook\""),
            "rustyline sem `signal-hook` instala um segundo handler de SIGINT: {linha}"
        );
    }

    /// Historico que nao pode existir (o "diretorio" e um arquivo) vira um
    /// aviso e o REPL segue — nunca um erro que impeca o chat de abrir.
    #[test]
    fn historico_indisponivel_e_aviso_e_nao_erro() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let nao_e_dir = tmp.path().join("nao-e-dir");
        std::fs::write(&nao_e_dir, b"x").expect("write");

        let mut leitor = LeitorDeLinha::abrir(true, Some(nao_e_dir.join("history")));
        // O editor em si abre mesmo sem terminal (`Behavior::Stdio` so olha
        // os descritores); o que falha e o arquivo.
        assert!(leitor.com_editor(), "{:?}", leitor.drenar_avisos());
        let avisos = leitor.drenar_avisos();
        assert_eq!(avisos.len(), 1, "{avisos:?}");
        assert!(
            avisos[0].starts_with("Historico do chat em ") && avisos[0].contains("indisponivel"),
            "{avisos:?}"
        );
        // Drenar esvazia: o aviso nao volta a cada linha.
        assert!(leitor.drenar_avisos().is_empty());
    }

    /// Cada linha aceita vai para o disco na hora, e nao so na saida — um
    /// `kill` no meio da sessao nao pode levar o que ja foi digitado. Linha
    /// com espaco na frente e duplicata consecutiva ficam de fora.
    #[test]
    fn cada_linha_aceita_vai_para_o_disco_na_hora() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let caminho = caminho_do_historico(tmp.path());

        let mut leitor = LeitorDeLinha::abrir(true, Some(caminho.clone()));
        assert!(leitor.drenar_avisos().is_empty());
        let Fonte::Editor { editor, historico } = &mut leitor.fonte else {
            panic!("esperava a fonte com editor");
        };
        assert_eq!(historico.as_deref(), Some(caminho.as_path()));

        for linha in [
            "/help",
            "primeira pergunta",
            "primeira pergunta",
            " nao guarda",
            "/exit",
        ] {
            guardar_no_historico(editor, historico, linha, &mut leitor.avisos);
        }

        let conteudo = std::fs::read_to_string(&caminho).expect("read");
        let linhas: Vec<&str> = conteudo.lines().filter(|l| !l.starts_with('#')).collect();
        assert_eq!(
            linhas,
            vec!["/help", "primeira pergunta", "/exit"],
            "conteudo gravado: {conteudo:?}"
        );
        assert!(leitor.avisos.is_empty(), "{:?}", leitor.avisos);
        assert!(
            historico.is_some(),
            "o disco continua ligado depois de gravar"
        );
    }
}
