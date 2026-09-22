//! SIGPIPE de volta ao padrao do Unix nos comandos que so imprimem e saem.
//!
//! O runtime do Rust ignora SIGPIPE antes do `main`. Com o sinal ignorado, um
//! `println!` num pipe cujo leitor ja fechou (`garra status | head -1`) recebe
//! `EPIPE`, e o `println!` entra em panico: "failed printing to stdout:
//! Broken pipe", exit 101 (L2 do smoke de instalacao limpa da v0.4.4). Com
//! `SIG_DFL` o processo termina em silencio no primeiro `write` sem leitor,
//! exatamente como `ls | head` ou `cat | head` — o shell ve 141 (128 + 13).
//!
//! Por que o sinal e nao um writer que traduz `BrokenPipe` em saida limpa: os
//! comandos imprimem por centenas de `println!` espalhados pelos modulos, e
//! parte da saida vem de crates de terceiros. Trocar cada ponto de escrita
//! deixaria de fora o proximo `println!` que alguem escrevesse, e o sinal
//! cobre todos de uma vez, inclusive os que ainda nao existem.
//!
//! Por que so em alguns comandos, e numa lista fechada: `SIG_DFL` mata o
//! processo no primeiro `write` sem leitor, em qualquer ponto da execucao. So
//! e aceitavel onde morrer no meio nao deixa nada pela metade — comandos de
//! leitura que imprimem e saem. O gateway (`start`, `restart`, o daemon), o
//! `mcp-server` e o REPL do `chat` continuam com o sinal ignorado: la um
//! leitor que some tem de virar erro tratado, nunca a morte do processo. Um
//! comando novo fica de fora ate alguem incluir, que e o lado seguro.
//!
//! O que a troca NAO alcanca:
//! - sockets: o `std` (e o `mio`, embaixo do tokio/reqwest) escreve com
//!   `MSG_NOSIGNAL` no Linux e liga `SO_NOSIGPIPE` no macOS, entao um par
//!   HTTP que fecha continua sendo um erro de I/O, nao um sinal;
//! - filhos: o `std::process::Command` ja reseta SIGPIPE para `SIG_DFL` no
//!   filho antes do `exec`, com ou sem esta troca.
//!
//! Windows nao tem SIGPIPE; la nada muda.

/// Poe SIGPIPE em `SIG_DFL` neste processo. Devolve `false` se o `signal(2)`
/// falhar — o processo segue como antes (sinal ignorado, `EPIPE` como erro),
/// que e um defeito cosmetico, nao motivo para abortar o comando.
#[cfg(unix)]
pub(crate) fn restaurar_padrao() -> bool {
    // SAFETY: `signal(2)` so troca a disposicao de SIGPIPE para o valor
    // padrao. Nenhum handler e instalado, entao nao ha codigo Rust rodando em
    // contexto de sinal, e `SIG_DFL` e um valor valido para qualquer sinal.
    let anterior = unsafe { libc::signal(libc::SIGPIPE, libc::SIG_DFL) };
    anterior != libc::SIG_ERR
}
