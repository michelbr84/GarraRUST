//! Supervisão de processo filho, sem Tauri.
//!
//! # De onde isto vem
//!
//! `crates/garraia-desktop/src-tauri/src/gateway.rs` já roda o `garraia` como
//! sidecar, com launch / restart / kill e morte do filho no `RunEvent::Exit`.
//! Supervisão não é território novo no projeto — é primitivo. O que faltava
//! era ele existir num lugar que o CI compila e testa.
//!
//! O que muda em relação ao original, e por quê:
//!
//! - **Sem `unwrap` em lock.** O original faz `handle.lock().unwrap()`; aqui
//!   um lock envenenado vira [`SuperviseError::Poisoned`], porque derrubar o
//!   app inteiro por causa de um panic em outra thread é pior do que relatar.
//! - **Sem `sleep` por dentro.** O original dorme 800 ms entre kill e spawn no
//!   restart. Dormir é decisão de ritmo, e ritmo pertence a quem tem o
//!   relógio: [`RestartPolicy::backoff`] devolve o intervalo, a casca espera.
//!   É o que mantém o teste sem `sleep` real.
//! - **Spawner injetável.** [`Spawner`] e [`ProcessHandle`] abstraem o
//!   nascimento e a morte do filho, então o ciclo de vida inteiro é testável
//!   sem criar processo de verdade.
//!
//! # A garantia que importa
//!
//! *"gateway sobe como sidecar e morre junto com o app"* é critério de
//! não-quebrou-nada da issue #1181. Aqui ela é estrutural: [`Supervisor`]
//! implementa [`Drop`] matando o filho. Não depende de alguém lembrar de
//! chamar [`Supervisor::stop`] no caminho de saída certo — inclusive no
//! caminho de saída que é um panic.

use crate::state::{Desired, ModuleState, PowerEvent};
use std::ffi::OsString;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use thiserror::Error;

/// Falhas de supervisão.
#[derive(Debug, Error)]
pub enum SuperviseError {
    #[error("falha ao lancar `{program}`: {source}")]
    Spawn {
        program: String,
        #[source]
        source: std::io::Error,
    },

    #[error("falha ao encerrar o processo: {0}")]
    Kill(#[source] std::io::Error),

    #[error("falha ao consultar o processo: {0}")]
    Wait(#[source] std::io::Error),

    /// Outra thread entrou em panic segurando o lock.
    #[error("estado do supervisor corrompido por panic em outra thread")]
    Poisoned,

    /// Pediram restart mais vezes do que a política permite.
    #[error("limite de {max} reinicios consecutivos atingido")]
    RestartLimit { max: u32 },
}

/// O que lançar.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcessSpec {
    pub program: PathBuf,
    pub args: Vec<OsString>,
}

impl ProcessSpec {
    pub fn new(program: impl Into<PathBuf>) -> Self {
        Self {
            program: program.into(),
            args: Vec::new(),
        }
    }

    pub fn arg(mut self, arg: impl Into<OsString>) -> Self {
        self.args.push(arg.into());
        self
    }

    /// Linha de comando legível.
    ///
    /// Regra 3 do #1181 — *executar é ação explícita, com o comando visível
    /// antes de rodar* — precisa de algo para mostrar. É isto.
    pub fn display(&self) -> String {
        let mut out = self.program.display().to_string();
        for arg in &self.args {
            out.push(' ');
            out.push_str(&arg.to_string_lossy());
        }
        out
    }
}

/// Um processo filho vivo.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExitStatus {
    pub code: Option<i32>,
    pub success: bool,
}

/// Handle de um processo filho.
///
/// `dyn`-safe de propósito: o [`Supervisor`] guarda `Box<dyn ProcessHandle>`,
/// que é o que permite trocar o processo real por um dublê no teste.
pub trait ProcessHandle: Send {
    /// PID, quando o backend expõe um.
    fn id(&self) -> Option<u32>;

    /// Encerra o processo. Deve ser idempotente: matar quem já morreu é
    /// sucesso, não erro.
    fn kill(&mut self) -> Result<(), SuperviseError>;

    /// `None` se ainda está rodando. Não bloqueia.
    fn try_wait(&mut self) -> Result<Option<ExitStatus>, SuperviseError>;
}

/// Quem sabe criar processos.
pub trait Spawner: Send + Sync {
    fn spawn(&self, spec: &ProcessSpec) -> Result<Box<dyn ProcessHandle>, SuperviseError>;
}

/// Spawner real, sobre `std::process::Command`.
#[derive(Debug, Clone, Copy, Default)]
pub struct CommandSpawner;

/// Handle sobre um `std::process::Child`.
struct ChildHandle {
    child: std::process::Child,
    /// Já foi morto ou já foi colhido: `kill` vira no-op.
    reaped: bool,
}

impl ProcessHandle for ChildHandle {
    fn id(&self) -> Option<u32> {
        Some(self.child.id())
    }

    fn kill(&mut self) -> Result<(), SuperviseError> {
        if self.reaped {
            return Ok(());
        }
        self.reaped = true;
        match self.child.kill() {
            Ok(()) => {
                // Colher o zumbi: sem isto o processo fica como defunct até o
                // pai morrer, e o desktop é um pai longevo.
                let _ = self.child.wait();
                Ok(())
            }
            // `InvalidInput` é o que o std devolve para um filho que já
            // terminou. Matar quem já morreu é sucesso.
            Err(e) if e.kind() == std::io::ErrorKind::InvalidInput => Ok(()),
            Err(e) => Err(SuperviseError::Kill(e)),
        }
    }

    fn try_wait(&mut self) -> Result<Option<ExitStatus>, SuperviseError> {
        if self.reaped {
            return Ok(Some(ExitStatus {
                code: None,
                success: false,
            }));
        }
        match self.child.try_wait() {
            Ok(Some(status)) => {
                self.reaped = true;
                Ok(Some(ExitStatus {
                    code: status.code(),
                    success: status.success(),
                }))
            }
            Ok(None) => Ok(None),
            Err(e) => Err(SuperviseError::Wait(e)),
        }
    }
}

impl Spawner for CommandSpawner {
    fn spawn(&self, spec: &ProcessSpec) -> Result<Box<dyn ProcessHandle>, SuperviseError> {
        let child = std::process::Command::new(&spec.program)
            .args(&spec.args)
            .spawn()
            .map_err(|source| SuperviseError::Spawn {
                program: spec.program.display().to_string(),
                source,
            })?;
        Ok(Box::new(ChildHandle {
            child,
            reaped: false,
        }))
    }
}

/// Quando desistir de reiniciar.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RestartPolicy {
    /// Reinícios consecutivos tolerados antes de desistir. Sem teto, um
    /// processo que morre no boot vira laço quente.
    pub max_restarts: u32,
    /// Espera base entre morte e novo `spawn`, em milissegundos. O original
    /// da casca Tauri usa 800 ms fixos.
    pub backoff_ms: u64,
}

impl Default for RestartPolicy {
    fn default() -> Self {
        Self {
            max_restarts: 3,
            backoff_ms: 800,
        }
    }
}

impl RestartPolicy {
    /// Espera antes da tentativa `attempt` (1 = primeira), com backoff
    /// exponencial e teto de 30 s.
    ///
    /// Devolve a duração; **não** dorme. Quem dorme é a casca, que tem o
    /// relógio — mesmo motivo pelo qual o `spinner.rs` da CLI conta ticks em
    /// vez de ler o tempo.
    pub fn backoff(&self, attempt: u32) -> std::time::Duration {
        let shift = attempt.saturating_sub(1).min(16);
        let ms = self.backoff_ms.saturating_mul(1u64 << shift).min(30_000);
        std::time::Duration::from_millis(ms)
    }

    /// Ainda pode tentar de novo?
    pub fn may_restart(&self, failures: u32) -> bool {
        failures < self.max_restarts
    }
}

/// Supervisor de um processo filho.
///
/// Casa o processo real com o [`ModuleState`] correspondente: cada operação
/// que mexe no processo também registra o fato no estado, então a UI nunca
/// mostra "ligado" para um processo que não existe.
pub struct Supervisor {
    spec: ProcessSpec,
    spawner: Arc<dyn Spawner>,
    policy: RestartPolicy,
    state: ModuleState,
    child: Mutex<Option<Box<dyn ProcessHandle>>>,
}

impl Supervisor {
    pub fn new(state: ModuleState, spec: ProcessSpec, spawner: Arc<dyn Spawner>) -> Self {
        Self {
            spec,
            spawner,
            policy: RestartPolicy::default(),
            state,
            child: Mutex::new(None),
        }
    }

    pub fn with_policy(mut self, policy: RestartPolicy) -> Self {
        self.policy = policy;
        self
    }

    pub fn state(&self) -> &ModuleState {
        &self.state
    }

    pub fn spec(&self) -> &ProcessSpec {
        &self.spec
    }

    pub fn policy(&self) -> RestartPolicy {
        self.policy
    }

    /// `true` quando há um filho registrado.
    pub fn is_running(&self) -> bool {
        self.child.lock().map(|g| g.is_some()).unwrap_or(false)
    }

    /// PID do filho, quando há um.
    pub fn pid(&self) -> Option<u32> {
        self.child.lock().ok()?.as_ref()?.id()
    }

    /// Lança o processo.
    ///
    /// Idempotente: chamar com um filho já rodando não lança um segundo — dois
    /// gateways na mesma porta é exatamente o bug que isto evita.
    pub fn start(&mut self) -> Result<(), SuperviseError> {
        self.state.request(Desired::On);

        {
            let guard = self.child.lock().map_err(|_| SuperviseError::Poisoned)?;
            if guard.is_some() {
                return Ok(());
            }
        }

        match self.spawner.spawn(&self.spec) {
            Ok(handle) => {
                tracing::info!(comando = %self.spec.display(), "processo supervisionado iniciado");
                let mut guard = self.child.lock().map_err(|_| SuperviseError::Poisoned)?;
                *guard = Some(handle);
                self.state.observe(PowerEvent::Started);
                Ok(())
            }
            Err(e) => {
                // Não conseguiu nascer conta como queda: é o que alimenta o
                // contador que a política consulta.
                self.state.observe(PowerEvent::Crashed);
                Err(e)
            }
        }
    }

    /// Encerra o processo e registra a parada.
    ///
    /// Idempotente: parar o que já está parado é sucesso.
    pub fn stop(&mut self) -> Result<(), SuperviseError> {
        self.state.request(Desired::Off);
        self.kill_child()?;
        self.state.observe(PowerEvent::Stopped);
        Ok(())
    }

    /// Mata e relança.
    ///
    /// Respeita [`RestartPolicy::max_restarts`]: acima do teto devolve
    /// [`SuperviseError::RestartLimit`] em vez de entrar em laço. A espera de
    /// [`RestartPolicy::backoff`] é responsabilidade de quem chama — este
    /// método não dorme.
    pub fn restart(&mut self) -> Result<(), SuperviseError> {
        if !self.policy.may_restart(self.state.failures()) {
            return Err(SuperviseError::RestartLimit {
                max: self.policy.max_restarts,
            });
        }
        self.kill_child()?;
        // O filho morreu porque mandamos; o estado continua com `Desired::On`,
        // então `start` retoma do ponto certo.
        self.state.request(Desired::On);
        self.start()
    }

    /// Colhe o filho se ele tiver morrido sozinho.
    ///
    /// É o equivalente, sem Tauri, de reagir à morte do processo: a casca
    /// chama isto no seu próprio ritmo e o estado passa a [`crate::Power::Failed`]
    /// quando a morte não foi pedida. Não bloqueia.
    ///
    /// Devolve o status do filho quando ele de fato terminou.
    pub fn poll(&mut self) -> Result<Option<ExitStatus>, SuperviseError> {
        let status = {
            let mut guard = self.child.lock().map_err(|_| SuperviseError::Poisoned)?;
            match guard.as_mut() {
                Some(child) => match child.try_wait()? {
                    Some(status) => {
                        *guard = None;
                        Some(status)
                    }
                    None => None,
                },
                None => None,
            }
        };

        if let Some(status) = status {
            tracing::warn!(
                codigo = ?status.code,
                comando = %self.spec.display(),
                "processo supervisionado terminou"
            );
            self.state.observe(PowerEvent::Crashed);
        }
        Ok(status)
    }

    fn kill_child(&self) -> Result<(), SuperviseError> {
        let mut guard = self.child.lock().map_err(|_| SuperviseError::Poisoned)?;
        if let Some(mut child) = guard.take() {
            child.kill()?;
        }
        Ok(())
    }
}

/// O filho morre junto com o supervisor.
///
/// É a versão estrutural do `RunEvent::Exit` da casca Tauri: vale para saída
/// normal, para `?` no meio de um caminho de erro e para desenrolamento de
/// panic. Nenhum deles depende de alguém ter lembrado de chamar
/// [`Supervisor::stop`].
impl Drop for Supervisor {
    fn drop(&mut self) {
        // Sem `?` e sem panic: `Drop` não pode falhar, e entrar em panic
        // durante desenrolamento aborta o processo.
        if let Ok(mut guard) = self.child.lock()
            && let Some(mut child) = guard.take()
            && let Err(e) = child.kill()
        {
            tracing::warn!(erro = %e, "falha ao encerrar processo filho no drop");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::{ModuleId, Power};
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

    /// Processo de mentira: lembra se foi morto e pode fingir ter terminado.
    #[derive(Default)]
    struct FakeChild {
        killed: Arc<AtomicBool>,
        exited: Arc<AtomicBool>,
    }

    impl ProcessHandle for FakeChild {
        fn id(&self) -> Option<u32> {
            Some(4242)
        }

        fn kill(&mut self) -> Result<(), SuperviseError> {
            self.killed.store(true, Ordering::SeqCst);
            Ok(())
        }

        fn try_wait(&mut self) -> Result<Option<ExitStatus>, SuperviseError> {
            if self.exited.load(Ordering::SeqCst) {
                Ok(Some(ExitStatus {
                    code: Some(1),
                    success: false,
                }))
            } else {
                Ok(None)
            }
        }
    }

    #[derive(Default)]
    struct FakeSpawner {
        spawns: AtomicUsize,
        killed: Arc<AtomicBool>,
        exited: Arc<AtomicBool>,
        fail: AtomicBool,
    }

    impl Spawner for FakeSpawner {
        fn spawn(&self, _spec: &ProcessSpec) -> Result<Box<dyn ProcessHandle>, SuperviseError> {
            if self.fail.load(Ordering::SeqCst) {
                return Err(SuperviseError::Spawn {
                    program: "fake".into(),
                    source: std::io::Error::new(std::io::ErrorKind::NotFound, "sem binario"),
                });
            }
            self.spawns.fetch_add(1, Ordering::SeqCst);
            Ok(Box::new(FakeChild {
                killed: Arc::clone(&self.killed),
                exited: Arc::clone(&self.exited),
            }))
        }
    }

    fn sup(spawner: Arc<FakeSpawner>) -> Supervisor {
        Supervisor::new(
            ModuleState::new(ModuleId::Gateway),
            ProcessSpec::new("/usr/bin/garraia").arg("start"),
            spawner,
        )
    }

    #[test]
    fn start_lanca_e_registra_o_estado() {
        let spawner = Arc::new(FakeSpawner::default());
        let mut s = sup(Arc::clone(&spawner));

        s.start().expect("lanca");
        assert_eq!(spawner.spawns.load(Ordering::SeqCst), 1);
        assert!(s.is_running());
        assert_eq!(s.pid(), Some(4242));
        assert_eq!(s.state().power(), Power::On);
    }

    #[test]
    fn start_duas_vezes_nao_lanca_dois_processos() {
        let spawner = Arc::new(FakeSpawner::default());
        let mut s = sup(Arc::clone(&spawner));

        s.start().expect("lanca");
        s.start().expect("segundo start e no-op");
        assert_eq!(
            spawner.spawns.load(Ordering::SeqCst),
            1,
            "dois gateways na mesma porta"
        );
    }

    #[test]
    fn stop_mata_o_filho_e_e_idempotente() {
        let spawner = Arc::new(FakeSpawner::default());
        let killed = Arc::clone(&spawner.killed);
        let mut s = sup(Arc::clone(&spawner));

        s.start().expect("lanca");
        s.stop().expect("para");

        assert!(killed.load(Ordering::SeqCst));
        assert!(!s.is_running());
        assert_eq!(s.state().power(), Power::Off);

        s.stop().expect("parar o que ja parou e sucesso");
        assert_eq!(s.state().power(), Power::Off);
    }

    #[test]
    fn restart_mata_e_relanca() {
        let spawner = Arc::new(FakeSpawner::default());
        let killed = Arc::clone(&spawner.killed);
        let mut s = sup(Arc::clone(&spawner));

        s.start().expect("lanca");
        s.restart().expect("reinicia");

        assert!(killed.load(Ordering::SeqCst));
        assert_eq!(spawner.spawns.load(Ordering::SeqCst), 2);
        assert_eq!(s.state().power(), Power::On);
    }

    #[test]
    fn falha_no_spawn_marca_queda_e_nao_fica_rodando() {
        let spawner = Arc::new(FakeSpawner::default());
        spawner.fail.store(true, Ordering::SeqCst);
        let mut s = sup(Arc::clone(&spawner));

        let err = s.start().expect_err("nao deveria lancar");
        assert!(matches!(err, SuperviseError::Spawn { .. }));
        assert!(!s.is_running());
        assert_eq!(s.state().power(), Power::Failed);
        assert_eq!(s.state().failures(), 1);
    }

    #[test]
    fn restart_para_no_teto_da_politica() {
        let spawner = Arc::new(FakeSpawner::default());
        spawner.fail.store(true, Ordering::SeqCst);
        let mut s = sup(Arc::clone(&spawner)).with_policy(RestartPolicy {
            max_restarts: 2,
            backoff_ms: 10,
        });

        // Duas falhas consecutivas esgotam a cota.
        let _ = s.start();
        let _ = s.restart();
        assert_eq!(s.state().failures(), 2);

        let err = s.restart().expect_err("acima do teto");
        assert!(matches!(err, SuperviseError::RestartLimit { max: 2 }));
    }

    #[test]
    fn poll_colhe_a_morte_espontanea_e_marca_falha() {
        let spawner = Arc::new(FakeSpawner::default());
        let exited = Arc::clone(&spawner.exited);
        let mut s = sup(Arc::clone(&spawner));

        s.start().expect("lanca");
        assert_eq!(s.poll().expect("consulta"), None, "ainda vivo");

        exited.store(true, Ordering::SeqCst);
        let status = s.poll().expect("consulta").expect("morreu");

        assert_eq!(status.code, Some(1));
        assert!(!status.success);
        assert!(!s.is_running());
        // Ninguém pediu para desligar: isso é falha, não parada.
        assert_eq!(s.state().power(), Power::Failed);
        assert_eq!(s.state().desired(), Desired::On);
    }

    #[test]
    fn poll_sem_filho_nao_inventa_evento() {
        let spawner = Arc::new(FakeSpawner::default());
        let mut s = sup(spawner);
        assert_eq!(s.poll().expect("consulta"), None);
        assert_eq!(s.state().power(), Power::Off);
        assert_eq!(s.state().failures(), 0);
    }

    /// "gateway sobe como sidecar e morre junto com o app" — critério de
    /// não-quebrou-nada da #1181.
    #[test]
    fn o_filho_morre_quando_o_supervisor_e_descartado() {
        let spawner = Arc::new(FakeSpawner::default());
        let killed = Arc::clone(&spawner.killed);

        {
            let mut s = sup(Arc::clone(&spawner));
            s.start().expect("lanca");
            assert!(!killed.load(Ordering::SeqCst));
        } // <- drop, sem `stop()` explícito

        assert!(
            killed.load(Ordering::SeqCst),
            "processo filho vazou depois do drop do supervisor"
        );
    }

    #[test]
    fn backoff_cresce_e_tem_teto() {
        let p = RestartPolicy {
            max_restarts: 10,
            backoff_ms: 800,
        };
        assert_eq!(p.backoff(1).as_millis(), 800);
        assert_eq!(p.backoff(2).as_millis(), 1_600);
        assert_eq!(p.backoff(3).as_millis(), 3_200);
        // Teto de 30 s, e sem overflow em expoente absurdo.
        assert_eq!(p.backoff(99).as_millis(), 30_000);
        // `attempt` 0 é tratado como a primeira tentativa.
        assert_eq!(p.backoff(0).as_millis(), 800);
    }

    #[test]
    fn spec_mostra_o_comando_antes_de_rodar() {
        let spec = ProcessSpec::new("/usr/bin/garraia").arg("start");
        assert_eq!(spec.display(), "/usr/bin/garraia start");
    }

    /// O spawner real precisa continuar `dyn`-safe e utilizável — é o que a
    /// casca Tauri vai injetar no lugar do dublê.
    #[test]
    fn command_spawner_serve_como_dyn_spawner() {
        let spawner: Arc<dyn Spawner> = Arc::new(CommandSpawner);
        let s = Supervisor::new(
            ModuleState::new(ModuleId::Gateway),
            ProcessSpec::new("/caminho/que/nao/existe/garraia"),
            spawner,
        );
        assert!(!s.is_running());
    }

    #[test]
    fn spawn_de_binario_inexistente_falha_com_erro_nomeado() {
        let mut s = Supervisor::new(
            ModuleState::new(ModuleId::Gateway),
            ProcessSpec::new("/caminho/que/nao/existe/garraia"),
            Arc::new(CommandSpawner),
        );
        let err = s.start().expect_err("binario nao existe");
        let msg = err.to_string();
        assert!(
            msg.contains("/caminho/que/nao/existe/garraia"),
            "erro deve nomear o binario: {msg}"
        );
        assert_eq!(s.state().power(), Power::Failed);
    }
}
