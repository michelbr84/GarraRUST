//! Vigia do processo pai: encerra este processo quando quem o lancou some.
//!
//! # Por que existe (#1636)
//!
//! A cadeia do app desktop e: app (Tauri) -> sidecar `garraia start` (gateway)
//! -> ponte Node. No Windows um Job Object `KILL_ON_JOB_CLOSE` ja encerra a
//! arvore quando o app morre de forma abrupta. No Linux e no macOS nao ha
//! equivalente de nivel de SO para o par app->sidecar: se o app morre de
//! `SIGKILL` (ou crash), o sidecar e reparentado ao init e fica orfao,
//! segurando a ponte Node aberta.
//!
//! A solucao portatil e o proprio sidecar vigiar o PID do pai e, quando ele
//! some, pedir o **mesmo encerramento gracioso do SIGTERM** — o gateway ja
//! escuta SIGTERM no `with_graceful_shutdown`, entao a ponte e os demais
//! subsistemas caem pelo caminho normal. A vigia e ligada pela flag
//! `garra start --exit-with-parent <pid>` (flag e nao env, para o PID nao
//! vazar para processos filhos).
//!
//! # Camadas
//!
//! Este modulo tem a parte **pura e testavel**: observar o pai
//! ([`observe_parent`]), decidir se ele se foi ([`parent_gone`]) e pedir o
//! encerramento ([`request_self_shutdown`]). O laco assincrono (um tick de ~1 s
//! numa task tokio) vive em quem liga a vigia (a `garraia-cli`), porque so ela
//! tem o runtime; aqui tudo e sincrono e sem relogio, no mesmo espirito do
//! `spinner.rs`.
//!
//! Fora de Unix a vigia e **no-op**: [`observe_parent`] sempre reporta o pai
//! vivo, entao [`parent_gone`] nunca dispara. No Windows a contencao do par
//! app->sidecar e o Job Object, e a `garraia-cli` nem liga a task la.

/// O que observamos do pai num instante.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ParentObservation {
    /// PPID atual (Unix). `None` fora de Unix, onde a vigia e no-op.
    pub observed_ppid: Option<u32>,
    /// O PID esperado ainda existe?
    pub expected_alive: bool,
}

/// O PPID no inicio da vigia (Unix). `None` fora de Unix.
///
/// Capturado uma vez por quem liga a vigia e passado a [`parent_gone`]: e o
/// sinal a prova de reuso de PID. Quando o pai original morre, o processo e
/// reparentado (ao init ou a um subreaper) e o PPID muda — independentemente de
/// o PID antigo ter sido reaproveitado por outro processo.
pub fn initial_parent() -> Option<u32> {
    #[cfg(unix)]
    {
        // SAFETY: `getppid` nao tem efeito colateral e nao pode falhar.
        Some(unsafe { libc::getppid() } as u32)
    }
    #[cfg(not(unix))]
    {
        None
    }
}

/// Observa o pai agora.
///
/// Unix: o PPID atual (`getppid`) e se o PID esperado ainda existe
/// (`kill(pid, 0)`; `EPERM` conta como "existe, so nao podemos sinaliza-lo").
/// Fora de Unix: sempre "vivo", porque a vigia e no-op.
pub fn observe_parent(expected_pid: u32) -> ParentObservation {
    #[cfg(unix)]
    {
        // SAFETY: `getppid` nao falha.
        let ppid = unsafe { libc::getppid() } as u32;
        // SAFETY: `kill` com o sinal 0 nao envia nada — so testa a existencia e
        // a permissao sobre o alvo.
        let rc = unsafe { libc::kill(expected_pid as libc::pid_t, 0) };
        let alive = if rc == 0 {
            true
        } else {
            // Leitura do errno IMEDIATAMENTE apos o `kill`, antes de qualquer
            // outra chamada que o sobrescreva. `EPERM` = o processo existe.
            std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
        };
        ParentObservation {
            observed_ppid: Some(ppid),
            expected_alive: alive,
        }
    }
    #[cfg(not(unix))]
    {
        let _ = expected_pid;
        ParentObservation {
            observed_ppid: None,
            expected_alive: true,
        }
    }
}

/// Decisao pura: o pai se foi?
///
/// Dois sinais, e basta um:
/// - **reparentado** — o PPID mudou do inicial (Unix): a prova a prova de reuso
///   de PID de que o pai original morreu;
/// - **o PID sumiu** — `kill(pid, 0)` deu `ESRCH`.
pub fn parent_gone(obs: &ParentObservation, initial_ppid: Option<u32>) -> bool {
    let reparented = matches!(
        (initial_ppid, obs.observed_ppid),
        (Some(inicial), Some(atual)) if atual != inicial
    );
    reparented || !obs.expected_alive
}

/// Pede o encerramento gracioso DESTE processo, pelo caminho do SIGTERM.
///
/// Unix: levanta `SIGTERM` para o proprio processo — o gateway ja tem o handler
/// instalado pelo `with_graceful_shutdown`, entao a ponte Node e os demais
/// subsistemas sao encerrados pelo mesmo caminho do Ctrl+C/SIGTERM. Devolve
/// `true` se sinalizou. Fora de Unix e no-op (devolve `false`): la o Job Object
/// cobre o caso.
pub fn request_self_shutdown() -> bool {
    #[cfg(unix)]
    {
        // SAFETY: `raise` entrega o sinal a este processo; e async-signal-safe.
        unsafe { libc::raise(libc::SIGTERM) };
        true
    }
    #[cfg(not(unix))]
    {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn obs(ppid: Option<u32>, alive: bool) -> ParentObservation {
        ParentObservation {
            observed_ppid: ppid,
            expected_alive: alive,
        }
    }

    #[test]
    fn vivo_e_mesmo_ppid_nao_se_foi() {
        assert!(!parent_gone(&obs(Some(42), true), Some(42)));
    }

    #[test]
    fn reparentado_se_foi_mesmo_com_pid_vivo() {
        // PID reaproveitado por outro processo: `kill(pid,0)` diz "vivo", mas o
        // PPID mudou -> o pai original morreu. E este o caso a prova de reuso.
        assert!(parent_gone(&obs(Some(1), true), Some(42)));
    }

    #[test]
    fn pid_sumiu_se_foi() {
        assert!(parent_gone(&obs(Some(42), false), Some(42)));
    }

    #[test]
    fn fora_de_unix_so_conta_o_sumico_do_pid() {
        // initial e observed_ppid None (o caso no-op): reparentar nunca dispara,
        // so a (in)existencia do PID — que fora de Unix e sempre "vivo".
        assert!(!parent_gone(&obs(None, true), None));
        assert!(parent_gone(&obs(None, false), None));
    }
}
