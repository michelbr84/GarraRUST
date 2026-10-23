//! Job Object do Windows: contencao do filho `node` da ponte quando o pai
//! morre de forma abrupta (crash, kill do Gerenciador de Tarefas), onde nenhum
//! `Drop` roda.
//!
//! # Por que existe, alem do `kill_on_drop` e do PDEATHSIG
//!
//! No Unix a ponte ja se protege por tres camadas (ver o topo de
//! `bridge.rs`): `kill_on_drop`, o `Drop` que mata o filho e, no Linux, o
//! PDEATHSIG. No Windows nao ha PDEATHSIG, e `kill_on_drop`/`Drop` so rodam
//! quando o processo pai termina de um jeito que desenrola a pilha. Um
//! `TerminateProcess` (o que o Gerenciador de Tarefas faz, e o que um crash
//! vira) nao roda nada — e ai o `node` fica orfao segurando a sessao do
//! WhatsApp aberta.
//!
//! # O mecanismo
//!
//! Um Job Object com `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE` mata todos os
//! processos do job no instante em que o ULTIMO handle do job fecha. Quando o
//! processo que segura o handle morre — por qualquer via, inclusive
//! `TerminateProcess` —, o Windows fecha os handles dele, o job fecha, e os
//! filhos morrem. Processos filhos entram no job do pai automaticamente, entao
//! um `node` que lance qualquer coisa morre inteiro, neto incluso.
//!
//! # Jobs aninhados e `ERROR_ACCESS_DENIED`
//!
//! Desde o Windows 8 um processo pode estar em varios jobs aninhados, entao
//! assinar um processo que ja esta num job (porque o proprio pai esta num)
//! funciona. Em versoes anteriores o `AssignProcessToJobObject` devolve
//! `ERROR_ACCESS_DENIED`: [`KillOnCloseJob::assign_handle`] trata como aviso,
//! sem panico — a contencao do job externo ainda vale, e as outras camadas
//! (`kill_on_drop`, o `Drop`) continuam.
//!
//! # Seguranca
//!
//! O `unsafe` esta confinado a este modulo. Cada chamada e a uma funcao Win32
//! documentada, com os argumentos conferidos antes; nenhuma invariante de
//! memoria de Rust depende do resultado de uma delas. `unwrap`/`expect` em
//! producao: nenhum.

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
// faz chamadas de leitura/atribuicao que o kernel serializa.
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
        // SAFETY: a struct e um agregado de inteiros (POD), entao zero e um
        // valor valido para todos os campos.
        let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = unsafe { std::mem::zeroed() };
        info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        // SAFETY: `info` vive durante a chamada; o ponteiro e o tamanho batem
        // com a struct, e a classe `JobObjectExtendedLimitInformation` casa
        // com o tipo dela.
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

    /// Assina um processo ao job pelo valor do handle dele (como `isize`).
    ///
    /// Recebe `isize` e nao `HANDLE` de proposito: o handle vem de outra crate
    /// (`tokio`/`std` via `AsRawHandle`), o valor nunca e desreferenciado por
    /// nos — so repassado a uma chamada Win32 — e um ponteiro cru na assinatura
    /// publica dispararia `clippy::not_unsafe_ptr_arg_deref` sem motivo.
    ///
    /// Preferivel a [`Self::assign_pid`]: nao ha corrida de reuso de PID, porque
    /// o handle fixa a identidade do processo. `ERROR_ACCESS_DENIED` (processo
    /// ja num job, Windows < 8) vira aviso e devolve `Ok`: a contencao externa
    /// continua valendo e as outras camadas tambem.
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
    /// fecha o handle em seguida. Usado por quem so tem o PID (a casca Tauri,
    /// que lanca o sidecar pelo plugin shell).
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
    use std::process::Command;
    use std::time::{Duration, Instant};

    use windows_sys::Win32::Foundation::{INVALID_HANDLE_VALUE, WAIT_TIMEOUT};
    use windows_sys::Win32::System::Threading::{
        PROCESS_QUERY_LIMITED_INFORMATION, WaitForSingleObject,
    };

    /// `true` enquanto o processo de `pid` existe e nao terminou.
    ///
    /// `OpenProcess` falha quando o processo ja saiu; se abre, o objeto fica
    /// sinalizado assim que ele termina, entao `WaitForSingleObject(0)` so
    /// devolve `WAIT_TIMEOUT` enquanto ele roda de fato.
    fn esta_vivo(pid: u32) -> bool {
        // SAFETY: abre com o direito minimo de consulta; NULL = ja nao existe.
        let h = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
        if h.is_null() {
            return false;
        }
        // SAFETY: `h` e um handle valido recem-aberto.
        let rodando = unsafe { WaitForSingleObject(h, 0) } == WAIT_TIMEOUT;
        // SAFETY: fecha o handle de consulta, uma unica vez.
        unsafe { CloseHandle(h) };
        rodando
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

    /// Fechar o handle do job (= o que o SO faz quando o pai e morto) mata a
    /// subarvore inteira: o `cmd` filho e o `ping` neto.
    #[test]
    fn fechar_o_job_mata_a_subarvore() {
        let job = KillOnCloseJob::new().expect("cria job");

        // `cmd` (filho) lanca `ping` (neto) e espera por ele: os dois ficam
        // vivos por ~60 s. O neto prova que o job pega a subarvore, nao so o
        // filho direto.
        let mut filho = Command::new("cmd.exe")
            .args(["/c", "ping -n 60 127.0.0.1 >NUL"])
            .spawn()
            .expect("spawn cmd");
        let filho_pid = filho.id();

        use std::os::windows::io::AsRawHandle;
        job.assign_handle(filho.as_raw_handle() as isize)
            .expect("assina o filho ao job");

        // Da tempo de o `cmd` lancar o `ping` e de ele herdar o job.
        std::thread::sleep(Duration::from_millis(400));
        let neto_pid = neto_ping_pid(filho_pid);

        // Fechar o handle do job e identico ao que o SO faz com o handle quando
        // o processo que o segura e morto por `TerminateProcess` (Gerenciador
        // de Tarefas / crash): `KILL_ON_JOB_CLOSE` dispara igual.
        drop(job);

        assert!(
            morreu_ate(filho_pid, Duration::from_secs(10)),
            "o filho {filho_pid} sobreviveu ao fechamento do job"
        );
        if let Some(neto) = neto_pid {
            assert!(
                morreu_ate(neto, Duration::from_secs(10)),
                "o neto {neto} sobreviveu ao fechamento do job"
            );
        }

        // Colhe o zumbi do filho para nao vazar handle no proprio teste.
        let _ = filho.wait();
    }

    /// Descobre o PID do `ping` neto varrendo os filhos do `cmd` via toolhelp.
    /// `None` quando nao der para identificar — o teste do filho ainda vale.
    fn neto_ping_pid(pai: u32) -> Option<u32> {
        use windows_sys::Win32::System::Diagnostics::ToolHelp::{
            CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW,
            TH32CS_SNAPPROCESS,
        };
        // SAFETY: snapshot de todos os processos; INVALID_HANDLE_VALUE em falha.
        let snap = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) };
        if snap == INVALID_HANDLE_VALUE {
            return None;
        }
        // SAFETY: a struct e POD; `dwSize` precisa estar certo antes do First.
        let mut entry: PROCESSENTRY32W = unsafe { std::mem::zeroed() };
        entry.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;
        let mut achou = None;
        // SAFETY: `snap` valido, `entry` vivo com `dwSize` setado.
        if unsafe { Process32FirstW(snap, &mut entry) } != 0 {
            loop {
                if entry.th32ParentProcessID == pai {
                    achou = Some(entry.th32ProcessID);
                    break;
                }
                // SAFETY: mesma pre-condicao; para quando nao ha mais entradas.
                if unsafe { Process32NextW(snap, &mut entry) } == 0 {
                    break;
                }
            }
        }
        // SAFETY: fecha o handle do snapshot, uma unica vez.
        unsafe { CloseHandle(snap) };
        achou
    }
}
