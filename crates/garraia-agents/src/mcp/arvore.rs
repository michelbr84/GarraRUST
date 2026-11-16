//! #1913: dominio de isolamento da arvore de processos de um servidor MCP
//! stdio, e a politica de encerramento dela.
//!
//! # O que e o dominio
//!
//! Um servidor MCP stdio e UM processo compartilhado por todas as chamadas
//! daquele servidor. O dominio de isolamento e portanto **a conexao**, nao a
//! chamada: o lider (o processo que o gateway lancou) mais tudo o que ele
//! lancar. Uma chamada isolada so tem a via cooperativa do protocolo — o
//! `notifications/cancelled` que o rmcp manda quando o `tools/call` estoura o
//! tempo ou e abandonado. Matar o dominio para parar UMA chamada derrubaria as
//! outras chamadas do mesmo servidor, e por isso nunca acontece.
//!
//! # Como o dominio e delimitado
//!
//! Duas marcas, porque nenhuma basta sozinha:
//!
//! - **Grupo de processos** (Unix): o lider nasce lider do proprio grupo
//!   (`setpgid(0, 0)`), entao filhos e netos herdam o grupo e um
//!   `kill(-pgid)` alcanca todos — menos quem abrir sessao/grupo novo
//!   (`setsid`), que sai do grupo de proposito.
//! - **Marcador de ambiente** ([`VAR_DOMINIO`], um token aleatorio por
//!   conexao): herdado por `fork`/`exec` mesmo atraves de `setsid`. No Linux o
//!   encerramento varre `/proc/<pid>/environ` atras dele e mata o que sobrou.
//!   Um descendente que limpa o proprio ambiente, ou que o kernel nao deixa
//!   ler (setuid, `hidepid`), escapa — e por isso a varredura e contencao de
//!   processo cooperativo ou descuidado, **nao** sandbox de processo hostil.
//!
//! # Reutilizacao de PID
//!
//! `kill(-pgid)` em numero reciclado mata o grupo de um inocente. O numero so
//! e garantido enquanto o lider nao foi colhido (`wait`): vivo ou zumbi, o
//! kernel nao reusa o PID. Por isso o grupo so e sinalizado nesse intervalo —
//! [`unix::FilhoDoGrupo`] olha a saida do lider com `WNOWAIT` (ve sem colher),
//! mata o grupo e so entao colhe; depois de colhido nao sinaliza mais nada.
//! Na varredura por marcador, o processo e fixado por `pidfd` e o marcador e
//! relido antes do sinal.

use std::time::{Duration, Instant};

use process_wrap::tokio::CommandWrap;
use tokio::process::Command;
use tracing::{info, warn};

/// Variavel que carrega o marcador do dominio para o lider e seus
/// descendentes. O valor e um token aleatorio por conexao, sem segredo.
pub(super) const VAR_DOMINIO: &str = "GARRAIA_MCP_DOMINIO";

/// Teto da espera pela confirmacao de que o dominio ficou vazio.
const TETO_DA_CONFIRMACAO: Duration = Duration::from_secs(2);

/// Intervalo entre duas olhadas (no lider ou no dominio).
const INTERVALO: Duration = Duration::from_millis(25);

/// De quanto em quanto tempo o vigia olha se o lider morreu.
#[cfg(unix)]
const INTERVALO_DO_VIGIA: Duration = Duration::from_millis(250);

/// Por que o dominio esta sendo encerrado. Vai para o log, e so.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum MotivoDoFim {
    /// `disconnect` de um servidor (admin, restart manual).
    Desconexao,
    /// `disconnect_all`: o gateway esta saindo.
    Encerramento,
    /// O transporte caiu sozinho (crash do servidor) e a conexao morta esta
    /// sendo trocada.
    Queda,
    /// O filho subiu mas o handshake ou o `tools/list` falhou.
    FalhaNaSubida,
}

impl MotivoDoFim {
    pub(super) fn as_str(self) -> &'static str {
        match self {
            Self::Desconexao => "desconexao",
            Self::Encerramento => "encerramento",
            Self::Queda => "queda",
            Self::FalhaNaSubida => "falha_na_subida",
        }
    }
}

/// O que o encerramento de um dominio apurou.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Relatorio {
    pub(super) motivo: MotivoDoFim,
    /// Quanto tempo o dominio existiu.
    pub(super) vida: Duration,
    /// Quanto tempo levou para confirmar (ou desistir de confirmar) o fim.
    pub(super) duracao: Duration,
    /// Processos fora do grupo (sessao nova) mortos pelo marcador.
    pub(super) mortos_por_marcador: usize,
    /// PIDs que ainda existiam quando a espera acabou. Vazio e o esperado.
    pub(super) remanescentes: Vec<u32>,
    /// `false` quando a plataforma nao da como conferir (Windows).
    pub(super) verificado: bool,
}

/// Prepara o comando de um servidor stdio para nascer dentro de um dominio:
/// injeta o marcador e, no Unix, faz o filho lider do proprio grupo.
pub(super) fn envolver(mut cmd: Command, marcador: &str) -> CommandWrap {
    cmd.env(VAR_DOMINIO, marcador);
    #[allow(unused_mut)]
    let mut envolto = CommandWrap::from(cmd);
    #[cfg(unix)]
    envolto.wrap(unix::GrupoDaArvore);
    envolto
}

/// Token novo para um dominio.
pub(super) fn novo_marcador() -> String {
    uuid::Uuid::new_v4().simple().to_string()
}

/// O dominio de uma conexao stdio viva.
#[derive(Debug)]
pub(super) struct Dominio {
    /// So a varredura do Linux le o marcador de volta.
    #[cfg_attr(not(target_os = "linux"), allow(dead_code))]
    marcador: String,
    /// PID do lider, que no Unix e tambem o id do grupo.
    #[cfg_attr(not(unix), allow(dead_code))]
    lider: Option<u32>,
    nascimento: Instant,
    /// Olha o lider de tempos em tempos (ver [`Dominio::novo`]).
    #[cfg(unix)]
    vigia: Option<tokio::task::JoinHandle<()>>,
}

impl Dominio {
    /// Registra o dominio de um filho recem-lancado por [`envolver`].
    ///
    /// No Unix sobe um vigia: se o lider morrer com descendentes segurando o
    /// stdout dele, o transporte nunca ve EOF, ninguem chama `wait` e a
    /// arvore ficaria de pe ate a proxima desconexao. O vigia ve a saida do
    /// lider sem colhe-lo e mata o grupo; os descendentes soltam o pipe e a
    /// queda aparece para o resto do gerenciador como qualquer outra.
    pub(super) fn novo(marcador: String, lider: Option<u32>) -> Self {
        Self {
            #[cfg(unix)]
            vigia: lider
                .and_then(|pid| i32::try_from(pid).ok())
                .map(|pid| tokio::spawn(unix::vigiar(pid))),
            marcador,
            lider,
            nascimento: Instant::now(),
        }
    }

    /// Encerra o dominio e confere que nao sobrou nada.
    ///
    /// Chamado **depois** de o transporte ter sido fechado ou largado — o
    /// lider e o grupo dele ja foram (ou estao sendo) derrubados por
    /// [`unix::FilhoDoGrupo`]. O que falta aqui e o que o grupo nao alcanca
    /// (quem abriu sessao nova, pelo marcador, no Linux) e a prova: esperar
    /// ate [`TETO_DA_CONFIRMACAO`] que o dominio esvazie e registrar o
    /// resultado, a duracao e o motivo.
    pub(super) async fn encerrar(self, servidor: &str, motivo: MotivoDoFim) -> Relatorio {
        let comeco = Instant::now();
        let mut mortos_por_marcador = 0usize;
        let (remanescentes, verificado) = loop {
            let (vivos, verificado) = self.sobreviventes(&mut mortos_por_marcador);
            if vivos.is_empty() || comeco.elapsed() >= TETO_DA_CONFIRMACAO {
                break (vivos, verificado);
            }
            tokio::time::sleep(INTERVALO).await;
        };

        let relatorio = Relatorio {
            motivo,
            vida: self.nascimento.elapsed(),
            duracao: comeco.elapsed(),
            mortos_por_marcador,
            remanescentes,
            verificado,
        };
        if relatorio.remanescentes.is_empty() {
            info!(
                server = %servidor,
                motivo = motivo.as_str(),
                vida_ms = relatorio.vida.as_millis() as u64,
                duracao_ms = relatorio.duracao.as_millis() as u64,
                mortos_por_marcador = relatorio.mortos_por_marcador,
                verificado = relatorio.verificado,
                "MCP server '{servidor}': arvore de processos encerrada"
            );
        } else {
            warn!(
                server = %servidor,
                motivo = motivo.as_str(),
                vida_ms = relatorio.vida.as_millis() as u64,
                duracao_ms = relatorio.duracao.as_millis() as u64,
                remanescentes = ?relatorio.remanescentes,
                "MCP server '{servidor}': processos da arvore ainda vivos apos o encerramento"
            );
        }
        relatorio
    }

    /// Uma passada: mata o que der e devolve quem ainda existe, mais se a
    /// plataforma soube conferir.
    #[cfg(target_os = "linux")]
    fn sobreviventes(&self, mortos_por_marcador: &mut usize) -> (Vec<u32>, bool) {
        let vivos = linux::com_marcador(&self.marcador);
        for pid in &vivos {
            if linux::matar_fixado(*pid, &self.marcador) && Some(*pid) != self.lider {
                *mortos_por_marcador += 1;
            }
        }
        (vivos, true)
    }

    /// Sem `/proc`, a prova e o grupo: `kill(-pgid, 0)` responde `ESRCH`
    /// quando nao ha mais ninguem nele. Quem saiu do grupo nao e visto.
    #[cfg(all(unix, not(target_os = "linux")))]
    fn sobreviventes(&self, _mortos_por_marcador: &mut usize) -> (Vec<u32>, bool) {
        match self.lider.and_then(|pid| i32::try_from(pid).ok()) {
            Some(pgid) if unix::grupo_tem_alguem(pgid) => (vec![pgid as u32], true),
            Some(_) => (Vec::new(), true),
            None => (Vec::new(), false),
        }
    }

    #[cfg(not(unix))]
    fn sobreviventes(&self, _mortos_por_marcador: &mut usize) -> (Vec<u32>, bool) {
        (Vec::new(), false)
    }
}

impl Drop for Dominio {
    fn drop(&mut self) {
        // O vigia so faz sentido enquanto a conexao existe; depois dela o PID
        // do lider pode ser de outro processo.
        #[cfg(unix)]
        if let Some(vigia) = self.vigia.take() {
            vigia.abort();
        }
    }
}

#[cfg(unix)]
pub(super) mod unix {
    use std::future::Future;
    use std::io;
    use std::pin::Pin;
    use std::process::ExitStatus;

    use process_wrap::tokio::{ChildWrapper, CommandWrap, CommandWrapper};
    use tokio::process::Command;

    use super::{INTERVALO, INTERVALO_DO_VIGIA};

    /// Faz o filho lider do proprio grupo e embrulha o `Child` em
    /// [`FilhoDoGrupo`].
    #[derive(Debug)]
    pub(in super::super) struct GrupoDaArvore;

    impl CommandWrapper for GrupoDaArvore {
        fn pre_spawn(&mut self, command: &mut Command, _core: &CommandWrap) -> io::Result<()> {
            command.process_group(0);
            Ok(())
        }

        fn wrap_child(
            &mut self,
            inner: Box<dyn ChildWrapper>,
            _core: &CommandWrap,
        ) -> io::Result<Box<dyn ChildWrapper>> {
            let pgid = inner
                .id()
                .and_then(|pid| i32::try_from(pid).ok())
                .filter(|pid| *pid > 1)
                .ok_or_else(|| io::Error::other("filho MCP sem PID utilizavel"))?;
            Ok(Box::new(FilhoDoGrupo {
                inner,
                pgid,
                colhido: None,
            }))
        }
    }

    /// O lider de um dominio, visto pelo rmcp como um `Child` qualquer.
    ///
    /// Tudo o que o rmcp faz com o filho — o `SIGTERM` e o `kill` do
    /// encerramento gracioso, o `kill` do `Drop` do transporte — passa a valer
    /// para o grupo inteiro, e a saida do lider derruba o grupo antes de ele
    /// ser colhido.
    #[derive(Debug)]
    pub(in super::super) struct FilhoDoGrupo {
        inner: Box<dyn ChildWrapper>,
        pgid: i32,
        /// `Some` depois que o lider foi colhido: dai em diante o numero do
        /// grupo nao e mais nosso e nada e sinalizado.
        colhido: Option<ExitStatus>,
    }

    impl FilhoDoGrupo {
        #[cfg(test)]
        pub(in super::super) fn para_teste(inner: Box<dyn ChildWrapper>, pgid: i32) -> Self {
            Self {
                inner,
                pgid,
                colhido: None,
            }
        }

        /// Sinaliza o grupo, se o numero dele ainda for garantidamente nosso.
        /// Devolve se o sinal foi de fato enviado.
        pub(in super::super) fn sinalizar_grupo(&self, sinal: i32) -> io::Result<bool> {
            if self.colhido.is_some() {
                return Ok(false);
            }
            // SAFETY: `kill(2)` com pid negativo so envia sinal; `pgid > 1`
            // (conferido em `wrap_child`), entao nunca vira `kill(-1)`.
            if unsafe { libc::kill(-self.pgid, sinal) } == 0 {
                return Ok(true);
            }
            let erro = io::Error::last_os_error();
            // Grupo ja vazio: nao ha o que sinalizar, e nao e erro.
            if erro.raw_os_error() == Some(libc::ESRCH) {
                return Ok(false);
            }
            Err(erro)
        }

        /// O lider saiu e ainda nao foi colhido: derruba o resto do grupo
        /// agora, enquanto o numero do grupo nao pode ser reciclado.
        fn derrubar_o_resto(&self) {
            if let Err(e) = self.sinalizar_grupo(libc::SIGKILL) {
                tracing::warn!("falha ao encerrar o grupo de processos MCP: {e}");
            }
        }
    }

    impl ChildWrapper for FilhoDoGrupo {
        fn inner(&self) -> &dyn ChildWrapper {
            self.inner.as_ref()
        }

        fn inner_mut(&mut self) -> &mut dyn ChildWrapper {
            self.inner.as_mut()
        }

        fn into_inner(self: Box<Self>) -> Box<dyn ChildWrapper> {
            self.inner
        }

        fn start_kill(&mut self) -> io::Result<()> {
            self.sinalizar_grupo(libc::SIGKILL).map(|_| ())
        }

        fn signal(&self, sig: i32) -> io::Result<()> {
            self.sinalizar_grupo(sig).map(|_| ())
        }

        fn try_wait(&mut self) -> io::Result<Option<ExitStatus>> {
            if let Some(status) = self.colhido {
                return Ok(Some(status));
            }
            if lider_saiu(self.pgid) == Some(false) {
                return Ok(None);
            }
            self.derrubar_o_resto();
            let status = self.inner.try_wait()?;
            self.colhido = status;
            Ok(status)
        }

        fn wait(&mut self) -> Pin<Box<dyn Future<Output = io::Result<ExitStatus>> + Send + '_>> {
            Box::pin(async move {
                if let Some(status) = self.colhido {
                    return Ok(status);
                }
                // Espera a saida SEM colher (`WNOWAIT`): colher antes de
                // sinalizar o grupo abriria a janela de reuso do PID.
                while lider_saiu(self.pgid) == Some(false) {
                    tokio::time::sleep(INTERVALO).await;
                }
                self.derrubar_o_resto();
                let status = self.inner.wait().await?;
                self.colhido = Some(status);
                Ok(status)
            })
        }
    }

    /// O filho `pid` ja saiu? Olha sem colher.
    ///
    /// `Some(true)` saiu (zumbi a espera de `wait`), `Some(false)` ainda
    /// roda, `None` nao e (mais) filho nosso — ja foi colhido.
    pub(super) fn lider_saiu(pid: i32) -> Option<bool> {
        // SAFETY: `siginfo_t` zerado e um valor valido, e `waitid` so escreve
        // nele. `WNOWAIT` deixa o filho colhivel por quem e dono do `Child`.
        let mut info: libc::siginfo_t = unsafe { std::mem::zeroed() };
        let rc = unsafe {
            libc::waitid(
                libc::P_PID,
                pid as libc::id_t,
                &mut info,
                libc::WEXITED | libc::WNOHANG | libc::WNOWAIT,
            )
        };
        if rc != 0 {
            return None;
        }
        // POSIX: com `WNOHANG` e nenhum filho pronto, `si_signo` fica zero.
        Some(info.si_signo != 0)
    }

    /// Ha algum processo (zumbi incluso) no grupo `pgid`?
    #[cfg(not(target_os = "linux"))]
    pub(super) fn grupo_tem_alguem(pgid: i32) -> bool {
        // SAFETY: sinal 0 nao entrega nada, so consulta.
        unsafe { libc::kill(-pgid, 0) == 0 }
    }

    /// Vigia do lider (ver [`super::Dominio::novo`]). Para sozinho quando o
    /// lider e colhido por outro caminho, e e abortado no `Drop` do dominio.
    pub(super) async fn vigiar(pid: i32) {
        loop {
            tokio::time::sleep(INTERVALO_DO_VIGIA).await;
            match lider_saiu(pid) {
                Some(false) => {}
                Some(true) => {
                    // Lider zumbi: o numero do grupo ainda e nosso.
                    // SAFETY: como em `sinalizar_grupo`; `pid > 1`.
                    if pid > 1 {
                        unsafe { libc::kill(-pid, libc::SIGKILL) };
                    }
                    return;
                }
                None => return,
            }
        }
    }
}

/// Varredura por marcador. So Linux: precisa de `/proc/<pid>/environ` e de
/// `pidfd` (kernel >= 5.3). Android fica de fora de proposito — la o seccomp
/// de apps pode responder a um syscall desconhecido com `SIGSYS`, nao `ENOSYS`.
#[cfg(target_os = "linux")]
mod linux {
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

    /// Zumbi nao e sobrevivente: ja morreu, so falta o pai colher.
    fn e_zumbi(pid: u32) -> bool {
        let Ok(stat) = std::fs::read_to_string(format!("/proc/{pid}/stat")) else {
            return false;
        };
        // O estado e o primeiro campo depois do ULTIMO `)` — o nome do
        // comando vai entre parenteses e pode conter espaco e `)`.
        stat.rsplit_once(')')
            .and_then(|(_, resto)| resto.split_whitespace().next())
            .is_some_and(|estado| estado == "Z" || estado == "X")
    }

    /// PIDs vivos que carregam o marcador.
    pub(super) fn com_marcador(marcador: &str) -> Vec<u32> {
        let Ok(entradas) = std::fs::read_dir("/proc") else {
            return Vec::new();
        };
        entradas
            .flatten()
            .filter_map(|entrada| entrada.file_name().to_str()?.parse::<u32>().ok())
            .filter(|pid| tem_marcador(*pid, marcador) && !e_zumbi(*pid))
            .collect()
    }

    /// `SIGKILL` no processo `pid`, com protecao contra reuso do numero: o
    /// processo e fixado por `pidfd` e o marcador e relido ANTES do sinal. Se
    /// o PID foi reciclado entre a varredura e aqui, o `pidfd` aponta para o
    /// processo novo, que nao tem o marcador, e nada e enviado.
    pub(super) fn matar_fixado(pid: u32, marcador: &str) -> bool {
        // SAFETY: `pidfd_open(pid, 0)` so devolve um descritor ou erro.
        let fd = unsafe { libc::syscall(libc::SYS_pidfd_open, pid as libc::pid_t, 0) };
        if fd < 0 {
            // Kernel sem pidfd, ou o processo ja se foi: sem identidade
            // fixada, nao se manda sinal.
            return false;
        }
        let enviado = tem_marcador(pid, marcador)
            // SAFETY: `fd` e um pidfd valido; `info` nulo e flags 0 sao o uso
            // documentado de `pidfd_send_signal`.
            && unsafe {
                libc::syscall(
                    libc::SYS_pidfd_send_signal,
                    fd,
                    libc::SIGKILL,
                    std::ptr::null::<libc::siginfo_t>(),
                    0,
                )
            } == 0;
        // SAFETY: `fd` foi aberto acima e nao e usado depois.
        unsafe { libc::close(fd as libc::c_int) };
        enviado
    }
}

#[cfg(all(test, unix))]
mod tests {
    use process_wrap::tokio::ChildWrapper;

    use super::*;

    fn dorminhoco() -> Command {
        let mut cmd = Command::new("sh");
        // O `sh` lanca um neto e espera por ele: lider + um descendente.
        cmd.args(["-c", "sleep 300 & wait"])
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null());
        cmd
    }

    /// O lider nasce lider do proprio grupo — sem isso `kill(-pgid)` nao tem
    /// alvo. **Mutacao**: tire o `process_group(0)` de `pre_spawn`.
    #[tokio::test]
    async fn o_lider_e_lider_do_proprio_grupo() {
        let mut filho = envolver(dorminhoco(), &novo_marcador())
            .spawn()
            .expect("spawn");
        let pid = filho.id().expect("pid") as i32;
        // SAFETY: `getpgid` so consulta.
        let pgid = unsafe { libc::getpgid(pid) };
        assert_eq!(pgid, pid, "o filho deveria liderar o proprio grupo");
        assert_ne!(
            pgid,
            unsafe { libc::getpgid(0) },
            "o filho nao pode ficar no grupo do gateway"
        );
        Box::into_pin(filho.kill()).await.expect("kill");
    }

    /// Protecao contra reuso de PID: depois de colhido o lider, o numero do
    /// grupo nao e mais nosso e NENHUM sinal sai. **Mutacao**: tire a guarda
    /// `colhido.is_some()` de `sinalizar_grupo` e o `kill` passa a ser
    /// tentado (devolve `ESRCH` aqui, mas num PID reciclado mataria outro).
    #[tokio::test]
    async fn depois_de_colhido_nada_e_sinalizado() {
        let mut cmd = dorminhoco();
        cmd.process_group(0);
        let filho = cmd.spawn().expect("spawn");
        let pgid = filho.id().expect("pid") as i32;
        let mut filho = unix::FilhoDoGrupo::para_teste(Box::new(filho), pgid);

        assert!(
            filho.sinalizar_grupo(0).expect("sonda"),
            "com o lider vivo o grupo e alcancavel"
        );
        Box::into_pin(filho.kill()).await.expect("kill");
        assert!(
            !filho.sinalizar_grupo(libc::SIGKILL).expect("pos-colheita"),
            "depois de colhido o lider, nenhum sinal pode sair"
        );
        assert_eq!(
            unix::lider_saiu(pgid),
            None,
            "o lider ja foi colhido: nao e mais filho"
        );
    }

    /// O `kill` derruba o grupo inteiro, nao so o lider. **Mutacao**: troque
    /// `start_kill` pelo passthrough (`self.inner.start_kill()`) e o neto
    /// `sleep` sobrevive.
    #[tokio::test]
    async fn kill_derruba_o_grupo_inteiro() {
        let mut filho = envolver(dorminhoco(), &novo_marcador())
            .spawn()
            .expect("spawn");
        let pgid = filho.id().expect("pid") as i32;
        // Da tempo de o `sh` lancar o neto.
        tokio::time::sleep(Duration::from_millis(200)).await;
        Box::into_pin(filho.kill()).await.expect("kill");

        let comeco = Instant::now();
        // SAFETY: sinal 0 so consulta.
        while unsafe { libc::kill(-pgid, 0) } == 0 {
            assert!(
                comeco.elapsed() < Duration::from_secs(5),
                "ainda ha processo no grupo {pgid} depois do kill"
            );
            tokio::time::sleep(INTERVALO).await;
        }
    }

    /// O marcador casa por igualdade exata da entrada — um token que e
    /// prefixo de outro nao pode casar com o processo errado.
    #[cfg(target_os = "linux")]
    #[tokio::test]
    async fn marcador_casa_so_por_igualdade() {
        let marcador = novo_marcador();
        let mut filho = envolver(dorminhoco(), &marcador).spawn().expect("spawn");
        let pid = filho.id().expect("pid");
        // O `environ` so reflete o marcador depois do `exec`.
        let comeco = Instant::now();
        while !linux::tem_marcador(pid, &marcador) {
            assert!(comeco.elapsed() < Duration::from_secs(5), "exec nao veio");
            tokio::time::sleep(INTERVALO).await;
        }
        assert!(!linux::tem_marcador(pid, &marcador[..marcador.len() - 1]));
        assert!(!linux::tem_marcador(pid, &format!("{marcador}0")));
        assert!(
            !linux::matar_fixado(pid, &novo_marcador()),
            "marcador de outro dominio nao pode matar este processo"
        );
        assert!(linux::com_marcador(&marcador).contains(&pid));
        assert!(!linux::tem_marcador(std::process::id(), &marcador));
        Box::into_pin(filho.kill()).await.expect("kill");
    }
}
