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
//! O vigia e o `FilhoDoGrupo` dividem a mesma trava ([`Colheita`]): quem vai
//! colher fecha a trava antes, e o vigia so sinaliza com ela na mao.
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

/// Intervalo entre duas olhadas no lider, e o primeiro entre duas passadas
/// de conferencia do dominio (que dobra ate [`INTERVALO_MAXIMO`]).
const INTERVALO: Duration = Duration::from_millis(25);

/// Teto do intervalo entre passadas: cada uma le `/proc` inteiro, e um filho
/// que nao para de se multiplicar nao pode transformar isso em laco quente.
const INTERVALO_MAXIMO: Duration = Duration::from_millis(400);

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

/// Trava dividida entre quem colhe o lider e o vigia: `true` quando o lider
/// ja foi (ou esta sendo) colhido e o numero do grupo deixou de ser nosso.
/// Fora do Unix nao ha grupo e a trava nao guarda nada.
#[derive(Debug, Clone, Default)]
pub(super) struct Colheita(
    #[cfg_attr(not(unix), allow(dead_code))] std::sync::Arc<std::sync::Mutex<bool>>,
);

/// Prepara o comando de um servidor stdio para nascer dentro de um dominio:
/// injeta o marcador e, no Unix, faz o filho lider do proprio grupo. A
/// [`Colheita`] devolvida vai para [`Dominio::novo`].
pub(super) fn envolver(mut cmd: Command, marcador: &str) -> (CommandWrap, Colheita) {
    cmd.env(VAR_DOMINIO, marcador);
    let colheita = Colheita::default();
    #[allow(unused_mut)]
    let mut envolto = CommandWrap::from(cmd);
    #[cfg(unix)]
    envolto.wrap(unix::GrupoDaArvore::novo(colheita.clone()));
    (envolto, colheita)
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
    pub(super) fn novo(marcador: String, lider: Option<u32>, colheita: Colheita) -> Self {
        #[cfg(not(unix))]
        let _ = colheita;
        Self {
            #[cfg(unix)]
            vigia: lider
                .and_then(|pid| i32::try_from(pid).ok())
                .filter(|pid| *pid > 1)
                .map(|pid| tokio::spawn(unix::vigiar(pid, colheita))),
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
        let mut intervalo = INTERVALO;
        let (remanescentes, verificado) = loop {
            // A passada le `/proc` inteiro e o conteudo dele e ditado por
            // processo nao confiavel: roda fora das threads da runtime e com
            // o que resta do teto como prazo. Estourado o prazo, o relatorio
            // sai como NAO verificado em vez de segurar o encerramento.
            let marcador = self.marcador.clone();
            let lider = self.lider;
            let passada = tokio::task::spawn_blocking(move || sobreviventes(&marcador, lider));
            let resta = TETO_DA_CONFIRMACAO.saturating_sub(comeco.elapsed());
            let Ok(Ok(passada)) = tokio::time::timeout(resta, passada).await else {
                break (Vec::new(), false);
            };
            mortos_por_marcador += passada.mortos_por_marcador;
            if passada.vivos.is_empty() || comeco.elapsed() >= TETO_DA_CONFIRMACAO {
                break (passada.vivos, passada.verificado);
            }
            tokio::time::sleep(intervalo).await;
            intervalo = (intervalo * 2).min(INTERVALO_MAXIMO);
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
}

/// O que uma passada de conferencia viu.
struct Passada {
    /// Quem ainda existia no dominio (ja com o sinal enviado, onde ha como).
    vivos: Vec<u32>,
    /// Processos fora do grupo mortos pelo marcador nesta passada.
    mortos_por_marcador: usize,
    /// `false` quando a plataforma nao soube conferir.
    verificado: bool,
}

/// Uma passada: mata o que der e devolve quem ainda existe.
#[cfg(target_os = "linux")]
fn sobreviventes(marcador: &str, lider: Option<u32>) -> Passada {
    let Some(vivos) = linux::com_marcador(marcador) else {
        // `/proc` ilegivel: nao ha prova nenhuma, e o relatorio diz isso.
        return Passada {
            vivos: Vec::new(),
            mortos_por_marcador: 0,
            verificado: false,
        };
    };
    let mortos_por_marcador = vivos
        .iter()
        .filter(|pid| linux::matar_fixado(**pid, marcador) && Some(**pid) != lider)
        .count();
    Passada {
        vivos,
        mortos_por_marcador,
        verificado: true,
    }
}

/// Sem `/proc`, a prova e o grupo: `kill(-pgid, 0)` responde `ESRCH` quando
/// nao ha mais ninguem nele. Quem saiu do grupo nao e visto.
#[cfg(all(unix, not(target_os = "linux")))]
fn sobreviventes(_marcador: &str, lider: Option<u32>) -> Passada {
    let pgid = lider
        .and_then(|pid| i32::try_from(pid).ok())
        .filter(|p| *p > 1);
    Passada {
        vivos: match pgid {
            Some(pgid) if unix::grupo_tem_alguem(pgid) => vec![pgid as u32],
            _ => Vec::new(),
        },
        mortos_por_marcador: 0,
        verificado: pgid.is_some(),
    }
}

#[cfg(not(unix))]
fn sobreviventes(_marcador: &str, _lider: Option<u32>) -> Passada {
    Passada {
        vivos: Vec::new(),
        mortos_por_marcador: 0,
        verificado: false,
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

    use super::{Colheita, INTERVALO, INTERVALO_DO_VIGIA};

    /// Faz o filho lider do proprio grupo e embrulha o `Child` em
    /// [`FilhoDoGrupo`].
    #[derive(Debug)]
    pub(in super::super) struct GrupoDaArvore {
        colheita: Colheita,
    }

    impl GrupoDaArvore {
        pub(in super::super) fn novo(colheita: Colheita) -> Self {
            Self { colheita }
        }
    }

    impl Colheita {
        /// Roda `f` com a trava na mao, entregando se o lider ja foi colhido.
        /// Trava envenenada conta como colhido: na duvida, nao se sinaliza.
        fn com<T>(&self, f: impl FnOnce(&mut bool) -> T) -> T {
            match self.0.lock() {
                Ok(mut colhido) => f(&mut colhido),
                Err(envenenada) => {
                    let mut colhido = envenenada.into_inner();
                    *colhido = true;
                    f(&mut colhido)
                }
            }
        }
    }

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
            let mut inner = inner;
            let Some(pgid) = inner
                .id()
                .and_then(|pid| i32::try_from(pid).ok())
                .filter(|pid| *pid > 1)
            else {
                // O filho ja foi lancado: devolver so o erro o largaria vivo
                // e fora de qualquer dominio.
                let _ = inner.start_kill();
                return Err(io::Error::other("filho MCP sem PID utilizavel"));
            };
            Ok(Box::new(FilhoDoGrupo {
                inner,
                pgid,
                colhido: None,
                colheita: self.colheita.clone(),
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
        /// A mesma trava do vigia (ver [`Colheita`]).
        colheita: Colheita,
    }

    impl FilhoDoGrupo {
        #[cfg(test)]
        pub(in super::super) fn para_teste(inner: Box<dyn ChildWrapper>, pgid: i32) -> Self {
            Self {
                inner,
                pgid,
                colhido: None,
                colheita: Colheita::default(),
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

        /// Ultimo passo antes de colher: derruba o resto do grupo (se o
        /// lider de fato saiu e o numero ainda e nosso) e fecha a trava, para
        /// que o vigia nao sinalize mais nada depois da colheita.
        fn fechar_antes_de_colher(&self, sinalizar: bool) {
            self.colheita.com(|colhido| {
                if sinalizar
                    && !*colhido
                    && let Err(e) = self.sinalizar_grupo(libc::SIGKILL)
                {
                    tracing::warn!("falha ao encerrar o grupo de processos MCP: {e}");
                }
                *colhido = true;
            });
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
            let lider = olhar_lider(self.pgid);
            if lider == Lider::Rodando {
                return Ok(None);
            }
            self.fechar_antes_de_colher(lider == Lider::Saiu);
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
                let lider = loop {
                    match olhar_lider(self.pgid) {
                        Lider::Rodando => tokio::time::sleep(INTERVALO).await,
                        outro => break outro,
                    }
                };
                self.fechar_antes_de_colher(lider == Lider::Saiu);
                let status = self.inner.wait().await?;
                self.colhido = Some(status);
                Ok(status)
            })
        }
    }

    /// O que `waitid` disse do lider, olhando sem colher.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub(super) enum Lider {
        /// Ainda roda.
        Rodando,
        /// Saiu e espera a colheita: o numero do grupo ainda e nosso.
        Saiu,
        /// Nao e (mais) filho nosso, ou o kernel nao soube dizer. Em nenhum
        /// dos dois casos ha garantia sobre o numero: nada e sinalizado.
        Indefinido,
    }

    /// O filho `pid` ja saiu? Olha sem colher.
    pub(super) fn olhar_lider(pid: i32) -> Lider {
        loop {
            // SAFETY: `siginfo_t` zerado e um valor valido, e `waitid` so
            // escreve nele. `WNOWAIT` deixa o filho colhivel por quem e dono
            // do `Child`.
            let mut info: libc::siginfo_t = unsafe { std::mem::zeroed() };
            let rc = unsafe {
                libc::waitid(
                    libc::P_PID,
                    pid as libc::id_t,
                    &mut info,
                    libc::WEXITED | libc::WNOHANG | libc::WNOWAIT,
                )
            };
            if rc == 0 {
                // POSIX: com `WNOHANG` e nenhum filho pronto, `si_signo`
                // fica zero.
                return if info.si_signo != 0 {
                    Lider::Saiu
                } else {
                    Lider::Rodando
                };
            }
            if io::Error::last_os_error().raw_os_error() != Some(libc::EINTR) {
                return Lider::Indefinido;
            }
        }
    }

    /// Ha algum processo (zumbi incluso) no grupo `pgid`?
    #[cfg(not(target_os = "linux"))]
    pub(super) fn grupo_tem_alguem(pgid: i32) -> bool {
        // SAFETY: sinal 0 nao entrega nada, so consulta.
        unsafe { libc::kill(-pgid, 0) == 0 }
    }

    /// Vigia do lider (ver [`super::Dominio::novo`]). Para sozinho quando o
    /// lider e colhido por outro caminho, e e abortado no `Drop` do dominio.
    ///
    /// O sinal sai com a [`Colheita`] na mao e so se ela ainda estiver
    /// aberta: quem colhe o lider fecha a trava ANTES de colher, entao o
    /// vigia nunca sinaliza um numero que ja pode ter sido reciclado.
    pub(super) async fn vigiar(pid: i32, colheita: Colheita) {
        loop {
            tokio::time::sleep(INTERVALO_DO_VIGIA).await;
            let acabou = colheita.com(|colhido| {
                if *colhido {
                    return true;
                }
                match olhar_lider(pid) {
                    Lider::Rodando => false,
                    Lider::Saiu => {
                        // Lider zumbi e trava aberta: o numero ainda e nosso.
                        // SAFETY: como em `sinalizar_grupo`; `pid > 1`
                        // (conferido em `Dominio::novo`).
                        unsafe { libc::kill(-pid, libc::SIGKILL) };
                        true
                    }
                    Lider::Indefinido => true,
                }
            });
            if acabou {
                return;
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
                    libc::SIGKILL as libc::c_long,
                    std::ptr::null::<libc::siginfo_t>(),
                    zero,
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
            .0
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
            unix::olhar_lider(pgid),
            unix::Lider::Indefinido,
            "o lider ja foi colhido: nao e mais filho"
        );
    }

    /// O `kill` derruba o grupo inteiro, nao so o lider. **Mutacao**: troque
    /// `start_kill` pelo passthrough (`self.inner.start_kill()`) e o neto
    /// `sleep` sobrevive.
    #[tokio::test]
    async fn kill_derruba_o_grupo_inteiro() {
        let mut filho = envolver(dorminhoco(), &novo_marcador())
            .0
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

    /// O vigia so sinaliza com a trava aberta: fechada (o lider foi, ou esta
    /// sendo, colhido), ele sai sem mandar nada, mesmo com o lider zumbi a
    /// vista. **Mutacao**: tire o `if *colhido { return true; }` de `vigiar`
    /// e o neto e morto.
    #[tokio::test]
    async fn vigia_nao_sinaliza_com_a_colheita_fechada() {
        let mut cmd = Command::new("sh");
        // O lider sai na hora e deixa um neto no grupo.
        cmd.args(["-c", "sleep 300 & exit 0"])
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .process_group(0);
        let mut filho = cmd.spawn().expect("spawn");
        let pgid = filho.id().expect("pid") as i32;
        let comeco = Instant::now();
        while unix::olhar_lider(pgid) != unix::Lider::Saiu {
            assert!(comeco.elapsed() < Duration::from_secs(5), "lider nao saiu");
            tokio::time::sleep(INTERVALO).await;
        }

        let fechada = Colheita(std::sync::Arc::new(std::sync::Mutex::new(true)));
        tokio::time::timeout(Duration::from_secs(5), unix::vigiar(pgid, fechada))
            .await
            .expect("com a trava fechada o vigia sai na primeira olhada");
        // SAFETY: sinal 0 so consulta.
        assert_eq!(
            unsafe { libc::kill(-pgid, 0) },
            0,
            "o vigia sinalizou o grupo com a colheita fechada"
        );

        // Com a trava aberta, o mesmo vigia derruba o grupo.
        tokio::time::timeout(
            Duration::from_secs(5),
            unix::vigiar(pgid, Colheita::default()),
        )
        .await
        .expect("vigia");
        filho.wait().await.expect("colher o lider");
        let comeco = Instant::now();
        while unsafe { libc::kill(-pgid, 0) } == 0 {
            assert!(comeco.elapsed() < Duration::from_secs(5), "neto sobreviveu");
            tokio::time::sleep(INTERVALO).await;
        }
    }

    /// O marcador casa por igualdade exata da entrada — um token que e
    /// prefixo de outro nao pode casar com o processo errado.
    #[cfg(target_os = "linux")]
    #[tokio::test]
    async fn marcador_casa_so_por_igualdade() {
        let marcador = novo_marcador();
        let mut filho = envolver(dorminhoco(), &marcador).0.spawn().expect("spawn");
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
        assert!(
            linux::com_marcador(&marcador)
                .expect("/proc")
                .contains(&pid)
        );
        assert!(!linux::tem_marcador(std::process::id(), &marcador));
        Box::into_pin(filho.kill()).await.expect("kill");
    }
}
