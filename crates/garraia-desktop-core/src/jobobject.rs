//! Job Object do Windows: contencao do processo supervisionado (o sidecar
//! `garraia start`, e por tabela a arvore que ele lanca) quando o pai — a
//! casca do desktop — morre de forma abrupta, sem rodar `Drop`.
//!
//! # Por que existe, alem do kill no `Drop` do [`crate::supervise::Supervisor`]
//!
//! O `Supervisor` ja mata o filho no `stop`, no `restart` e no `Drop`. Mas
//! `Drop` so roda quando o processo pai termina desenrolando a pilha. Um
//! `TerminateProcess` (Gerenciador de Tarefas, crash) nao roda nada — e ai o
//! sidecar fica orfao, e com ele o filho Node da ponte do WhatsApp. No Unix o
//! equivalente seria PDEATHSIG; no Windows a unica garantia e o Job Object.
//!
//! # O mecanismo
//!
//! Um Job Object com `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE` mata todos os
//! processos do job quando o ULTIMO handle do job fecha. Quando o processo que
//! segura o handle morre — por qualquer via, inclusive `TerminateProcess` —, o
//! Windows fecha os handles dele, o job fecha e os filhos morrem. Processos
//! filhos entram no job do pai automaticamente, entao a arvore inteira (sidecar
//! → ponte → neto) morre junto.
//!
//! # Jobs aninhados e `ERROR_ACCESS_DENIED`
//!
//! Desde o Windows 8 um processo pode estar em varios jobs aninhados, entao
//! assinar um processo que ja esta num job funciona. Em versoes anteriores o
//! `AssignProcessToJobObject` devolve `ERROR_ACCESS_DENIED`:
//! [`KillOnCloseJob::assign_handle`] trata como aviso, sem panico.
//!
//! # Seguranca
//!
//! O `unsafe` esta confinado a este modulo. Cada chamada e a uma funcao Win32
//! documentada, com os argumentos conferidos antes; nenhuma invariante de
//! memoria de Rust depende do resultado. `unwrap`/`expect` em producao:
//! nenhum.

use std::io;

use windows_sys::Win32::Foundation::{CloseHandle, ERROR_ACCESS_DENIED, HANDLE};
use windows_sys::Win32::System::JobObjects::{
    AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
    JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectExtendedLimitInformation,
    SetInformationJobObject,
};
use windows_sys::Win32::System::Threading::{OpenProcess, PROCESS_SET_QUOTA, PROCESS_TERMINATE};

/// Um Job Object do Windows armado com `KILL_ON_JOB_CLOSE`.
///
/// Enquanto este valor existe, o handle do job fica aberto. Quando ele e
/// descartado — ou quando o processo que o segura morre —, o job fecha e os
/// processos nele sao mortos pelo proprio Windows.
#[derive(Debug)]
pub struct KillOnCloseJob {
    handle: HANDLE,
}

// SAFETY: `handle` e um HANDLE do Windows (ponteiro opaco do kernel). So e
// usado em chamadas Win32 que aceitam o handle de qualquer thread, e fechado
// uma unica vez no `Drop`. Move-lo entre threads e seguro.
unsafe impl Send for KillOnCloseJob {}
// SAFETY: as chamadas Win32 sobre um handle de job sao thread-safe; `&self` so
// faz chamadas que o kernel serializa.
unsafe impl Sync for KillOnCloseJob {}

impl KillOnCloseJob {
    /// Cria um job anonimo e arma o `KILL_ON_JOB_CLOSE`.
    pub fn new() -> io::Result<Self> {
        // SAFETY: `CreateJobObjectW(NULL, NULL)` cria um job anonimo sem
        // atributos de seguranca. Devolve NULL em falha, tratado abaixo.
        let handle = unsafe { CreateJobObjectW(std::ptr::null(), std::ptr::null()) };
        if handle.is_null() {
            return Err(io::Error::last_os_error());
        }
        let job = Self { handle };
        job.arm_kill_on_close()?;
        Ok(job)
    }

    fn arm_kill_on_close(&self) -> io::Result<()> {
        // SAFETY: a struct e um agregado de inteiros (POD): zero e valido.
        let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = unsafe { std::mem::zeroed() };
        info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        // SAFETY: `info` vive durante a chamada; o ponteiro e o tamanho batem
        // com a struct, e a classe casa com o tipo dela.
        let ok = unsafe {
            SetInformationJobObject(
                self.handle,
                JobObjectExtendedLimitInformation,
                std::ptr::addr_of!(info).cast(),
                std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            )
        };
        if ok == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }

    /// Assina um processo ao job pelo valor do handle (como `isize`).
    ///
    /// Recebe `isize` e nao `HANDLE` de proposito: o handle vem de outra crate
    /// (`std` via `AsRawHandle`), o valor nunca e desreferenciado por nos — so
    /// repassado a uma chamada Win32 — e um ponteiro cru na assinatura publica
    /// dispararia `clippy::not_unsafe_ptr_arg_deref` sem motivo.
    ///
    /// Preferivel a [`Self::assign_pid`]: sem corrida de reuso de PID.
    /// `ERROR_ACCESS_DENIED` (processo ja num job, Windows < 8) vira aviso e
    /// devolve `Ok`: a contencao externa continua valendo.
    pub fn assign_handle(&self, process: isize) -> io::Result<()> {
        // 0 = NULL, -1 = INVALID_HANDLE_VALUE: nenhum e um processo.
        if process == 0 || process == -1 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "handle de processo invalido",
            ));
        }
        let process = process as HANDLE;
        // SAFETY: `self.handle` e um job vivo e `process` e um handle de
        // processo valido fornecido pelo chamador.
        let ok = unsafe { AssignProcessToJobObject(self.handle, process) };
        if ok == 0 {
            let err = io::Error::last_os_error();
            if err.raw_os_error() == Some(ERROR_ACCESS_DENIED as i32) {
                tracing::warn!(
                    "processo ja pertence a um job (Windows < 8?); contencao do job externo mantida"
                );
                return Ok(());
            }
            return Err(err);
        }
        Ok(())
    }

    /// Assina um processo ao job pelo PID.
    ///
    /// Abre o processo com os direitos minimos (`SET_QUOTA | TERMINATE`) e
    /// fecha o handle em seguida. Usado por quem so tem o PID — a casca Tauri,
    /// que lanca o sidecar pelo plugin shell e so expoe o pid.
    pub fn assign_pid(&self, pid: u32) -> io::Result<()> {
        // SAFETY: `OpenProcess` com direitos minimos; devolve NULL em falha.
        let process = unsafe { OpenProcess(PROCESS_SET_QUOTA | PROCESS_TERMINATE, 0, pid) };
        if process.is_null() {
            return Err(io::Error::last_os_error());
        }
        let res = self.assign_handle(process as isize);
        // SAFETY: fecha o handle aberto logo acima, uma unica vez.
        unsafe { CloseHandle(process) };
        res
    }
}

impl Drop for KillOnCloseJob {
    fn drop(&mut self) {
        if !self.handle.is_null() {
            // SAFETY: fecha o handle do job uma unica vez. Depois disto o
            // `KILL_ON_JOB_CLOSE` mata os processos ainda no job.
            unsafe { CloseHandle(self.handle) };
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::windows::io::AsRawHandle;
    use std::process::Command;
    use std::time::{Duration, Instant};

    use windows_sys::Win32::System::Threading::{
        GetExitCodeProcess, PROCESS_QUERY_LIMITED_INFORMATION,
    };

    /// `GetExitCodeProcess` devolve 259 (STILL_ACTIVE) enquanto o processo
    /// roda. Usado em vez de `WaitForSingleObject` porque este exige o direito
    /// `SYNCHRONIZE`, que `PROCESS_QUERY_LIMITED_INFORMATION` nao concede — sem
    /// ele o wait falha e todo processo parece morto. Um processo que saia com
    /// 259 seria ambiguo, mas cmd/ping nunca saem assim.
    const STILL_ACTIVE_U32: u32 = 259;

    fn esta_vivo(pid: u32) -> bool {
        // SAFETY: abre com o direito minimo de consulta; NULL = ja nao existe.
        let h = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
        if h.is_null() {
            return false;
        }
        let mut code: u32 = 0;
        // SAFETY: `h` e um handle valido recem-aberto; `code` e um u32 vivo.
        let ok = unsafe { GetExitCodeProcess(h, &mut code) };
        // SAFETY: fecha o handle de consulta, uma unica vez.
        unsafe { CloseHandle(h) };
        ok != 0 && code == STILL_ACTIVE_U32
    }

    fn morreu_ate(pid: u32, prazo: Duration) -> bool {
        let fim = Instant::now() + prazo;
        while Instant::now() < fim {
            if !esta_vivo(pid) {
                return true;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        !esta_vivo(pid)
    }

    /// Fechar o handle do job (= o que o SO faz quando o pai e morto por
    /// `TerminateProcess`) mata o processo nele. O caso pai→filho→neto com um
    /// pai morto de verdade vive no teste de integracao `orphan_process`.
    #[test]
    fn fechar_o_job_mata_o_processo_assinado() {
        let job = KillOnCloseJob::new().expect("cria job");
        let mut filho = Command::new("cmd.exe")
            .args(["/c", "ping -n 60 127.0.0.1 >NUL"])
            .spawn()
            .expect("spawn cmd");
        let pid = filho.id();

        job.assign_handle(filho.as_raw_handle() as isize)
            .expect("assina ao job");
        assert!(esta_vivo(pid), "o filho {pid} deveria estar vivo");

        drop(job);

        assert!(
            morreu_ate(pid, Duration::from_secs(10)),
            "o filho {pid} sobreviveu ao fechamento do job"
        );
        let _ = filho.wait();
    }
}
