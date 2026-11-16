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
    /// Um `connect` sobre um nome ja conectado trocou a conexao.
    Troca,
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
            Self::Troca => "troca",
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
    /// `true` so quando houve conferencia de verdade (a varredura do Linux
    /// leu `/proc` e terminou dentro do teto). Fora do Linux e sempre
    /// `false`: o grupo foi sinalizado, mas nada confere quem sobrou.
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
    #[cfg_attr(not(target_os = "linux"), allow(dead_code))]
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
        let mut mortos: Vec<u32> = Vec::new();
        let mut vivos: Vec<u32> = Vec::new();
        let mut verificado = false;
        let mut intervalo = INTERVALO;
        loop {
            let resta = TETO_DA_CONFIRMACAO.saturating_sub(comeco.elapsed());
            if resta.is_zero() {
                // Teto estourado com gente viva: `vivos` guarda a ultima
                // passada, que e o que o relatorio tem de mostrar.
                break;
            }
            // A passada le `/proc` inteiro e o conteudo dele e ditado por
            // processo nao confiavel: roda fora das threads da runtime e com
            // o que resta do teto como prazo.
            let marcador = self.marcador.clone();
            let lider = self.lider;
            let ja_mortos = mortos.clone();
            let passada =
                tokio::task::spawn_blocking(move || sobreviventes(&marcador, lider, &ja_mortos));
            let Ok(Ok(passada)) = tokio::time::timeout(resta, passada).await else {
                // A passada nao terminou: o que se sabia antes continua
                // valendo, mas nao e mais prova.
                verificado = false;
                break;
            };
            mortos.extend(&passada.mortos);
            vivos = passada.vivos;
            verificado = passada.verificado;
            if vivos.is_empty() {
                break;
            }
            tokio::time::sleep(intervalo.min(resta)).await;
            intervalo = (intervalo * 2).min(INTERVALO_MAXIMO);
        }
        let (remanescentes, mortos_por_marcador) = (vivos, mortos.len());

        let relatorio = Relatorio {
            motivo,
            vida: self.nascimento.elapsed(),
            duracao: comeco.elapsed(),
            mortos_por_marcador,
            remanescentes,
            verificado,
        };
        let vida_ms = relatorio.vida.as_millis() as u64;
        let duracao_ms = relatorio.duracao.as_millis() as u64;
        if !relatorio.remanescentes.is_empty() {
            warn!(
                server = %servidor,
                motivo = motivo.as_str(),
                vida_ms,
                duracao_ms,
                verificado = relatorio.verificado,
                remanescentes = ?relatorio.remanescentes,
                "MCP server '{servidor}': processos da arvore ainda vivos apos o encerramento"
            );
        } else if cfg!(target_os = "linux") && !relatorio.verificado {
            // No Linux a conferencia existe; se ela nao aconteceu, "nao
            // sobrou nada" nao foi provado e nao pode sair como sucesso.
            warn!(
                server = %servidor,
                motivo = motivo.as_str(),
                vida_ms,
                duracao_ms,
                "MCP server '{servidor}': arvore de processos encerrada SEM conferencia"
            );
        } else {
            info!(
                server = %servidor,
                motivo = motivo.as_str(),
                vida_ms,
                duracao_ms,
                mortos_por_marcador = relatorio.mortos_por_marcador,
                verificado = relatorio.verificado,
                "MCP server '{servidor}': arvore de processos encerrada"
            );
        }
        relatorio
    }
}

/// O que uma passada de conferencia viu.
struct Passada {
    /// Quem ainda existia no dominio (ja com o sinal enviado).
    vivos: Vec<u32>,
    /// Processos fora do grupo mortos pelo marcador nesta passada.
    mortos: Vec<u32>,
    /// `false` quando nao houve como conferir.
    verificado: bool,
}

/// Uma passada: colhe os orfaos ja mortos, mata o que der e devolve quem
/// ainda existe. `ja_mortos` sao os que passadas anteriores mataram pelo
/// marcador, para serem colhidos se ficaram orfaos deste processo.
#[cfg(target_os = "linux")]
fn sobreviventes(marcador: &str, lider: Option<u32>, ja_mortos: &[u32]) -> Passada {
    linux::colher_orfaos(lider, ja_mortos);
    let Some(vivos) = linux::com_marcador(marcador) else {
        // `/proc` ilegivel: nao ha prova nenhuma, e o relatorio diz isso.
        return Passada {
            vivos: Vec::new(),
            mortos: Vec::new(),
            verificado: false,
        };
    };
    let mortos = vivos
        .iter()
        .copied()
        .filter(|pid| linux::matar_fixado(*pid, marcador) && Some(*pid) != lider)
        .collect();
    Passada {
        vivos,
        mortos,
        verificado: true,
    }
}

/// Fora do Linux nao ha varredura: o grupo ja foi sinalizado por
/// [`unix::FilhoDoGrupo`] (onde ha grupo), e nada aqui confere o resultado.
#[cfg(not(target_os = "linux"))]
fn sobreviventes(_marcador: &str, _lider: Option<u32>, _ja_mortos: &[u32]) -> Passada {
    Passada {
        vivos: Vec::new(),
        mortos: Vec::new(),
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
            if erro.raw_os_error() != Some(libc::ESRCH) {
                return Err(erro);
            }
            // Grupo vazio. Ou acabou todo mundo, ou o lider se mudou para
            // outro grupo (`setpgid`) — e ai ele so e alcancavel pelo proprio
            // PID, que continua nosso enquanto ele nao for colhido.
            // SAFETY: `pgid > 1` e o PID do lider, ainda nao colhido.
            Ok(unsafe { libc::kill(self.pgid, sinal) } == 0)
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

        /// Atencao: nao e consulta pura. Visto que o lider saiu, o resto do
        /// grupo e derrubado ANTES de ele ser colhido — e a colheita que
        /// libera o numero do grupo, entao e a ultima chance segura.
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
mod linux;

#[cfg(all(test, unix))]
mod tests;
