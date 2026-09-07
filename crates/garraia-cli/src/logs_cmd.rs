//! `garra logs` — ler o log sem depender do gateway (#943).
//!
//! # O que este comando e, e o que ele nao e
//!
//! E um leitor do arquivo canonico. **Nao** fala com o gateway, nao abre
//! socket e nao pede que nada esteja rodando — o criterio de aceite pede isso
//! com essas palavras, e e o que torna o comando util justamente quando o
//! gateway morreu.
//!
//! # Redacao
//!
//! Nao ha redacao aqui, e isso e de proposito. Ela acontece na **escrita**: o
//! `RedactingMakeWriter` embrulha o appender em `main.rs`, entao o que esta no
//! disco ja esta redigido. Redigir de novo na leitura mascararia um vazamento
//! em vez de conserta-lo — quem depura um incidente lendo `garraia.log` no
//! `less` veria o segredo do mesmo jeito, e nos acharíamos que estamos
//! protegidos. Ha um teste afirmando que o comando devolve o byte que leu.
//!
//! # Por que nao ha `--level`
//!
//! A issue cita `--level debug` como extensao possivel. Uma entrada de log
//! ocupa **varias linhas** quando carrega backtrace ou payload multi-linha, e
//! filtrar linha a linha partiria a entrada ao meio: sobraria a primeira linha
//! do erro sem o rastro que explica. Quem quer menos ruido no arquivo tem o
//! `RUST_LOG`, que decide na escrita e nao corta entrada pela metade.

use std::io::{BufRead, BufReader, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Context, Result};

/// Quantas linhas do fim mostrar quando ninguem pede outra coisa.
pub const LINHAS_PADRAO: usize = 100;

/// De quanto em quanto tempo o `--follow` procura conteudo novo.
///
/// Poll, e nao `inotify`: o `--follow` precisa funcionar no Windows e num
/// diretorio montado por rede, onde a notificacao do sistema nao chega. 250 ms
/// e imperceptivel para quem le e e barato — um `read` num fd ja aberto.
const INTERVALO_DO_FOLLOW: Duration = Duration::from_millis(250);

/// O caminho canonico do log.
///
/// Vive aqui e nao no `main.rs` porque la ele e `#[cfg(unix)]` — o daemon so
/// existe no Unix. O **arquivo** existe nos dois, escrito pelo appender de
/// tracing, e a issue pede o comando cross-platform onde for pratico.
pub fn caminho_do_log(dir: &Path) -> PathBuf {
    dir.join("garraia.log")
}

/// O que fazer quando o arquivo nao esta la.
///
/// Nao e erro: e o estado normal de uma instalacao que nunca rodou nada. A
/// issue pede "helpful message", e a mensagem util diz **onde** ele estaria e
/// **o que** o cria.
fn ausente(caminho: &Path, out: &mut impl Write) -> Result<()> {
    writeln!(out, "Nenhum log ainda em {}", caminho.display())?;
    writeln!(
        out,
        "O arquivo aparece quando algo roda: `garra start` e o caso comum."
    )?;
    writeln!(
        out,
        "Para mais detalhe no arquivo: RUST_LOG=debug garra start"
    )?;
    Ok(())
}

/// Imprime as ultimas `linhas` do log.
///
/// Le o arquivo inteiro para pegar o fim. E o suficiente e nao vale otimizar:
/// o appender e `rolling::never`, o arquivo cresce ate a instalacao ser
/// limpa, e ainda assim e um log de texto de uma maquina so — quando ele
/// chegar a um tamanho em que isso doa, o problema a resolver e a rotacao, e
/// nao a leitura.
pub fn tail(caminho: &Path, linhas: usize, out: &mut impl Write) -> Result<()> {
    if !caminho.exists() {
        return ausente(caminho, out);
    }
    let arquivo = std::fs::File::open(caminho)
        .with_context(|| format!("nao consegui abrir {}", caminho.display()))?;
    let leitor = BufReader::new(arquivo);

    // Buffer circular: guarda so as ultimas `linhas`.
    let mut ultimas: std::collections::VecDeque<String> = std::collections::VecDeque::new();
    for linha in leitor.lines() {
        // Linha invalida em UTF-8 nao derruba o comando: um log truncado no
        // meio de um caractere e exatamente o caso em que se quer ler o log.
        let linha = match linha {
            Ok(l) => l,
            Err(_) => continue,
        };
        if ultimas.len() == linhas {
            ultimas.pop_front();
        }
        ultimas.push_back(linha);
    }

    if ultimas.is_empty() {
        writeln!(out, "O log em {} esta vazio.", caminho.display())?;
        return Ok(());
    }
    for l in &ultimas {
        writeln!(out, "{l}")?;
    }
    Ok(())
}

/// Segue o arquivo ate ser interrompido.
///
/// `parar` e consultado a cada volta: e por onde o `Ctrl+C` entra. Sem isso o
/// laco so morreria com o processo, e o criterio de aceite pede saida limpa.
pub fn follow(
    caminho: &Path,
    linhas: usize,
    out: &mut impl Write,
    parar: &dyn Fn() -> bool,
) -> Result<()> {
    tail(caminho, linhas, out)?;
    let _ = out.flush();

    // A posicao de onde continuar. Quando o arquivo ainda nao existe,
    // comeca do zero — e o caso de `garra logs --follow` aberto antes do
    // `garra start`, que e justamente quando ele e mais util.
    let mut posicao = std::fs::metadata(caminho).map(|m| m.len()).unwrap_or(0);

    while !parar() {
        std::thread::sleep(INTERVALO_DO_FOLLOW);
        let tamanho = match std::fs::metadata(caminho) {
            Ok(m) => m.len(),
            // Sumiu no meio do caminho (limpeza, reinstalacao): espera voltar
            // em vez de morrer.
            Err(_) => continue,
        };
        if tamanho < posicao {
            // Truncado ou recriado: o appender e `rolling::never`, mas nada
            // impede alguem de apagar o arquivo. Recomeça do inicio em vez de
            // ficar lendo alem do fim para sempre.
            posicao = 0;
        }
        if tamanho == posicao {
            continue;
        }
        let mut arquivo = match std::fs::File::open(caminho) {
            Ok(f) => f,
            Err(_) => continue,
        };
        if arquivo.seek(SeekFrom::Start(posicao)).is_err() {
            continue;
        }
        let leitor = BufReader::new(&mut arquivo);
        for linha in leitor.lines().map_while(std::result::Result::ok) {
            writeln!(out, "{linha}")?;
        }
        let _ = out.flush();
        posicao = tamanho;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn escreve(dir: &Path, conteudo: &str) -> PathBuf {
        let p = caminho_do_log(dir);
        std::fs::write(&p, conteudo).expect("escrever");
        p
    }

    #[test]
    fn arquivo_ausente_da_mensagem_util_e_nao_erro() {
        let dir = tempfile::tempdir().expect("tmp");
        let mut out: Vec<u8> = Vec::new();
        tail(&caminho_do_log(dir.path()), 10, &mut out).expect("nao e erro");
        let s = String::from_utf8(out).expect("utf8");

        assert!(s.contains("Nenhum log ainda"), "saiu: {s:?}");
        assert!(s.contains("garraia.log"), "diz onde estaria: {s:?}");
        assert!(s.contains("garra start"), "diz o que o cria: {s:?}");
    }

    #[test]
    fn arquivo_vazio_tambem_e_dito() {
        let dir = tempfile::tempdir().expect("tmp");
        let p = escreve(dir.path(), "");
        let mut out: Vec<u8> = Vec::new();
        tail(&p, 10, &mut out).expect("ok");
        assert!(String::from_utf8_lossy(&out).contains("esta vazio"));
    }

    #[test]
    fn mostra_so_as_ultimas_linhas_e_na_ordem() {
        let dir = tempfile::tempdir().expect("tmp");
        let corpo: String = (1..=50).map(|n| format!("linha {n}\n")).collect();
        let p = escreve(dir.path(), &corpo);

        let mut out: Vec<u8> = Vec::new();
        tail(&p, 3, &mut out).expect("ok");
        assert_eq!(
            String::from_utf8(out).expect("utf8"),
            "linha 48\nlinha 49\nlinha 50\n"
        );
    }

    /// Pedir mais linhas do que existem devolve o arquivo inteiro.
    #[test]
    fn pedir_demais_devolve_o_que_ha() {
        let dir = tempfile::tempdir().expect("tmp");
        let p = escreve(dir.path(), "a\nb\n");
        let mut out: Vec<u8> = Vec::new();
        tail(&p, 1000, &mut out).expect("ok");
        assert_eq!(String::from_utf8(out).expect("utf8"), "a\nb\n");
    }

    /// O comando devolve o byte que leu — a redacao e da escrita.
    ///
    /// Se isto falhar porque alguem acrescentou redacao na leitura, o
    /// problema nao e o teste: mascarar aqui esconderia um vazamento que
    /// continua no disco, visivel para quem abrir o arquivo no `less`.
    #[test]
    fn o_conteudo_sai_como_esta_no_disco() {
        let dir = tempfile::tempdir().expect("tmp");
        let linha = "2026-01-01T00:00:00Z INFO tudo bem por aqui\n";
        let p = escreve(dir.path(), linha);
        let mut out: Vec<u8> = Vec::new();
        tail(&p, 10, &mut out).expect("ok");
        assert_eq!(String::from_utf8(out).expect("utf8"), linha);
    }

    /// Byte invalido em UTF-8 nao derruba a leitura.
    #[test]
    fn log_com_byte_invalido_nao_derruba() {
        let dir = tempfile::tempdir().expect("tmp");
        let p = caminho_do_log(dir.path());
        std::fs::write(&p, b"boa\n\xff\xfe quebrado\nfim\n").expect("escrever");
        let mut out: Vec<u8> = Vec::new();
        tail(&p, 10, &mut out).expect("nao derruba");
        let s = String::from_utf8(out).expect("utf8");
        assert!(s.contains("boa") && s.contains("fim"), "saiu: {s:?}");
    }

    /// O follow para quando mandam parar — e o caminho do Ctrl+C.
    #[test]
    fn follow_para_quando_mandam_parar() {
        let dir = tempfile::tempdir().expect("tmp");
        let p = escreve(dir.path(), "inicio\n");
        let mut out: Vec<u8> = Vec::new();

        // Para na primeira consulta: o `tail` inicial ja saiu, e o laco nem
        // chega a dormir.
        follow(&p, 10, &mut out, &|| true).expect("ok");
        assert!(String::from_utf8_lossy(&out).contains("inicio"));
    }

    /// O que for acrescentado depois aparece.
    #[test]
    fn follow_mostra_o_que_chega_depois() {
        let dir = tempfile::tempdir().expect("tmp");
        let p = escreve(dir.path(), "antes\n");

        // Para na terceira consulta, dando duas voltas ao laco.
        let voltas = std::sync::atomic::AtomicUsize::new(0);
        let caminho = p.clone();
        let parar = move || {
            let n = voltas.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            if n == 0 {
                // Entre a primeira e a segunda volta, alguem escreve.
                let mut f = std::fs::OpenOptions::new()
                    .append(true)
                    .open(&caminho)
                    .expect("abrir");
                f.write_all(b"depois\n").expect("escrever");
            }
            n >= 2
        };

        let mut out: Vec<u8> = Vec::new();
        follow(&p, 10, &mut out, &parar).expect("ok");
        let s = String::from_utf8(out).expect("utf8");
        assert!(s.contains("antes"), "o tail inicial: {s:?}");
        assert!(s.contains("depois"), "e o que chegou depois: {s:?}");
    }

    /// Arquivo apagado no meio do follow nao derruba o comando.
    #[test]
    fn follow_sobrevive_ao_arquivo_sumir() {
        let dir = tempfile::tempdir().expect("tmp");
        let p = escreve(dir.path(), "antes\n");

        let voltas = std::sync::atomic::AtomicUsize::new(0);
        let caminho = p.clone();
        let parar = move || {
            let n = voltas.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            if n == 0 {
                let _ = std::fs::remove_file(&caminho);
            }
            n >= 2
        };

        let mut out: Vec<u8> = Vec::new();
        follow(&p, 10, &mut out, &parar).expect("nao derruba");
    }
}
