//! Parte so-Linux de [`super`]: a varredura por marcador e a colheita dos
//! orfaos que o encerramento deixa quando o gateway e o pai adotivo deles.

use super::VAR_DOMINIO;

/// O processo `pid` carrega exatamente este marcador no ambiente com que
/// foi executado? Processo ilegivel (outro uid, setuid) conta como nao.
pub(super) fn tem_marcador(pid: u32, marcador: &str) -> bool {
    let Ok(ambiente) = std::fs::read(format!("/proc/{pid}/environ")) else {
        return false;
    };
    let alvo = format!("{VAR_DOMINIO}={marcador}");
    ambiente
        .split(|byte| *byte == 0)
        .any(|entrada| entrada == alvo.as_bytes())
}

/// Estado, pai e grupo de `pid`, de `/proc/<pid>/stat`.
fn estado(pid: u32) -> Option<(String, u32, u32)> {
    let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    // Os campos comecam depois do ULTIMO `)` — o nome do comando vai entre
    // parenteses e pode conter espaco e `)`.
    let (_, resto) = stat.rsplit_once(')')?;
    let mut campos = resto.split_whitespace();
    let estado = campos.next()?.to_string();
    let pai = campos.next()?.parse().ok()?;
    let grupo = campos.next()?.parse().ok()?;
    Some((estado, pai, grupo))
}

/// Zumbi nao e sobrevivente: ja morreu, so falta o pai colher.
fn e_zumbi(pid: u32) -> bool {
    estado(pid).is_some_and(|(estado, _, _)| estado == "Z" || estado == "X")
}

/// Colhe os zumbis do dominio que ficaram orfaos **deste** processo.
///
/// Quando o lider morre, os descendentes dele sao adotados pelo subreaper
/// mais proximo. Se esse pai adotivo e o gateway (PID 1 de um container, ou
/// subreaper), ninguem mais vai chamar `wait` por eles: cada encerramento
/// deixaria um `<defunct>` para sempre. Sao colhidos aqui os zumbis filhos
/// deste processo que ainda estao no grupo do dominio ou que a varredura
/// matou pelo marcador (`mortos`). O lider nunca: ele e do `Child` do tokio.
pub(super) fn colher_orfaos(lider: Option<u32>, mortos: &[u32]) {
    let eu = std::process::id();
    let Ok(entradas) = std::fs::read_dir("/proc") else {
        return;
    };
    for pid in entradas
        .flatten()
        .filter_map(|entrada| entrada.file_name().to_str()?.parse::<u32>().ok())
    {
        if Some(pid) == lider || pid == eu {
            continue;
        }
        let Some((estado, pai, grupo)) = estado(pid) else {
            continue;
        };
        if estado == "Z" && pai == eu && (Some(grupo) == lider || mortos.contains(&pid)) {
            // SAFETY: `waitpid` com `WNOHANG` num filho zumbi nosso so o
            // colhe; status nulo e permitido.
            unsafe { libc::waitpid(pid as libc::pid_t, std::ptr::null_mut(), libc::WNOHANG) };
        }
    }
}

/// PIDs vivos que carregam o marcador; `None` se `/proc` nao pode ser
/// listado. O proprio gateway nunca entra, carregue o que carregar.
pub(super) fn com_marcador(marcador: &str) -> Option<Vec<u32>> {
    let eu = std::process::id();
    let vivos = std::fs::read_dir("/proc")
        .ok()?
        .flatten()
        .filter_map(|entrada| entrada.file_name().to_str()?.parse::<u32>().ok())
        .filter(|pid| *pid != eu && tem_marcador(*pid, marcador) && !e_zumbi(*pid))
        .collect();
    Some(vivos)
}

/// `SIGKILL` no processo `pid`, com protecao contra reuso do numero: o
/// processo e fixado por `pidfd` e o marcador e relido ANTES do sinal. Se
/// o PID foi reciclado entre a varredura e aqui, o `pidfd` aponta para o
/// processo novo, que nao tem o marcador, e nada e enviado.
pub(super) fn matar_fixado(pid: u32, marcador: &str) -> bool {
    if pid == std::process::id() {
        return false;
    }
    // `syscall` e variadica: cada argumento vai como `c_long`, que e o
    // que o kernel le, em vez de depender da promocao de um `c_int`.
    let zero: libc::c_long = 0;
    // SAFETY: `pidfd_open(pid, 0)` so devolve um descritor ou erro.
    let fd = unsafe { libc::syscall(libc::SYS_pidfd_open, pid as libc::c_long, zero) };
    if fd < 0 {
        // Sem identidade fixada, nao se manda sinal. `ESRCH` e o processo
        // que ja se foi; qualquer outro erro (kernel sem pidfd, seccomp) e
        // contencao que NAO aconteceu, e fica dito.
        let erro = std::io::Error::last_os_error();
        if erro.raw_os_error() != Some(libc::ESRCH) {
            tracing::debug!(pid, "pidfd_open falhou, processo nao sinalizado: {erro}");
        }
        return false;
    }
    let enviado = tem_marcador(pid, marcador)
        // SAFETY: `fd` e um pidfd valido; `info` nulo e flags 0 sao o uso
        // documentado de `pidfd_send_signal`.
        && unsafe {
            libc::syscall(
                libc::SYS_pidfd_send_signal,
                fd,
                libc::SIGKILL as libc::c_long,
                std::ptr::null::<libc::siginfo_t>(),
                zero,
            )
        } == 0;
    // SAFETY: `fd` foi aberto acima e nao e usado depois.
    unsafe { libc::close(fd as libc::c_int) };
    enviado
}
