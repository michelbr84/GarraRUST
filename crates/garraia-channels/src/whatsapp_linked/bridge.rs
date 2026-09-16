//! Materializacao, supervisao e I/O do bridge Node/Baileys.
//!
//! # Quem manda em quem
//!
//! O bridge e um processo filho burro: ele nao escreve no disco, nao desenha
//! QR e nao decide nada. Ele fala NDJSON no stdout e escuta NDJSON no stdin. O
//! Rust e quem persiste, quem renderiza e quem decide — e, principalmente, quem
//! e responsavel por o processo nao sobreviver a esta CLI.
//!
//! # Contencao do filho
//!
//! Vale a mesma politica dos servidores MCP (`garraia-agents::mcp::manager`),
//! porque o risco e o mesmo — um processo de terceiros nascendo com o ambiente
//! deste processo:
//!
//! - **`env_clear()` + [`garraia_common::safety_gate::allowed_child_env`]**. O
//!   filho nao herda `ANTHROPIC_API_KEY`, `GARRAIA_JWT_SECRET` nem a passphrase
//!   do cofre. A allowlist R3 (`PATH`, `HOME`, `LANG`, `LC_ALL`, `TERM`,
//!   `USER`) e o piso; a esta soma-se `TMPDIR` e as demais `LC_*`, que o Node
//!   usa para locale e para arquivos temporarios do npm — e nada mais.
//! - **PDEATHSIG** (linux/android): gateway morto de `SIGKILL` nao deixa o
//!   Node orfao segurando a sessao.
//! - **`Drop` mata o filho**, como no `supervise.rs` do desktop-core: nao
//!   depende de ninguem lembrar de chamar `shutdown` no caminho de saida certo,
//!   inclusive quando o caminho de saida e um panic.
//!
//! **`RLIMIT_AS` NAO e aplicado aqui**, ao contrario do que o MCP faz. Foi
//! tentado e reprovado na bancada: um teto de 2 GiB de espaco de enderecamento
//! mata o filho com `SIGABRT` antes de ele fazer qualquer coisa util. O motivo
//! e estrutural, nao um teto mal escolhido — o V8 do Node 20+ **reserva** uma
//! "cage" de varios GiB de memoria VIRTUAL no boot (ponteiros comprimidos), e
//! `RLIMIT_AS` limita exatamente memoria virtual reservada, nao residente. O
//! mesmo aconteceu com o `python3` da fixture. Medir consumo real exige
//! `RLIMIT_RSS` (que o Linux ignora desde sempre) ou um cgroup, que e outra
//! ordem de complexidade. A contencao aqui e a que funciona: ambiente
//! limpo, PDEATHSIG, `kill_on_drop` e `Drop`.
//!
//! Windows nao precisa do wrapper `cmd /c` que o MCP usa: la o alvo e
//! `node.exe`, um executavel de verdade, e nao um `.cmd` de shim do npm.
//!
//! # Enquadramento
//!
//! Linha terminada em `\n`, teto de [`MAX_FRAME_BYTES`]. Estourar o teto e erro
//! de protocolo e **derruba a conexao**: um leitor que cresce sem limite e um
//! OOM esperando uma linha sem `\n`.

use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, ChildStdout, Command};
use tokio::sync::mpsc;

use super::protocol::{BridgeCommand, BridgeEvent, MAX_FRAME_BYTES, PROTOCOL_VERSION};

/// Nome do arquivo que guarda o hash dos assets ja materializados.
const STAMP_FILE: &str = ".garraia-bridge-sha256";

/// Teto do `npm install`. Rede ruim leva minutos; uma hora e travamento.
pub const NPM_INSTALL_TIMEOUT: Duration = Duration::from_secs(600);

/// Quantas linhas de stderr do filho ficam guardadas para o diagnostico.
const STDERR_TAIL_LINES: usize = 30;

/// Falhas do bridge.
#[derive(Debug, thiserror::Error)]
pub enum BridgeError {
    /// `node` ou `npm` nao estao na PATH. Mapeado para `EX_UNAVAILABLE` (69).
    #[error("{0} nao encontrado na PATH")]
    ToolMissing(&'static str),

    #[error("falha ao lancar `{program}`: {source}")]
    Spawn {
        program: String,
        #[source]
        source: std::io::Error,
    },

    #[error("{path}: {source}")]
    Io {
        path: String,
        #[source]
        source: std::io::Error,
    },

    /// Linha maior que o teto, JSON invalido, ou texto livre no stdout.
    #[error("erro de protocolo: {0}")]
    Protocol(String),

    /// `started.protocol` diferente do que esta CLI fala.
    #[error(
        "o bridge fala o protocolo {found}, esta versao do GarraIA fala o {PROTOCOL_VERSION}. \
Atualize com `garra update` ou apague {dir} para reinstalar o bridge."
    )]
    ProtocolVersion { found: u32, dir: String },

    #[error("`npm ci` falhou (codigo {code}):\n{tail}")]
    NpmInstall { code: String, tail: String },

    #[error("`npm ci` passou de {}s sem terminar", NPM_INSTALL_TIMEOUT.as_secs())]
    NpmTimeout,

    /// O bridge nao respondeu dentro do prazo do driver.
    #[error("o bridge parou de responder")]
    Timeout,

    /// O processo terminou.
    #[error("o bridge encerrou (codigo {code:?})")]
    Exited { code: Option<i32> },
}

impl BridgeError {
    fn io(path: &Path, source: std::io::Error) -> Self {
        Self::Io {
            path: path.display().to_string(),
            source,
        }
    }
}

// ---------------------------------------------------------------------------
// Descoberta de ferramentas
// ---------------------------------------------------------------------------

/// `which` sem crate externo: varre `PATH` procurando um executavel.
///
/// Mesma forma do `which_in_path` de `garraia-cli::agents` — copiada e nao
/// compartilhada porque `garraia-channels` nao depende (e nao deve depender) da
/// CLI, e porque sao 15 linhas sem estado.
pub fn find_executable(bin: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path) {
        let candidate = dir.join(bin);
        if candidate.is_file() {
            return Some(candidate);
        }
        for ext in ["exe", "cmd", "bat"] {
            let with_ext = dir.join(format!("{bin}.{ext}"));
            if with_ext.is_file() {
                return Some(with_ext);
            }
        }
    }
    None
}

/// Node e npm resolvidos.
#[derive(Debug, Clone)]
pub struct NodeRuntime {
    pub node: PathBuf,
    pub npm: PathBuf,
}

impl NodeRuntime {
    /// Localiza os dois. Falta de qualquer um vira [`BridgeError::ToolMissing`],
    /// que a CLI traduz em exit 69 com uma frase de instalacao.
    pub fn detect() -> Result<Self, BridgeError> {
        let node = find_executable("node").ok_or(BridgeError::ToolMissing("node"))?;
        let npm = find_executable("npm").ok_or(BridgeError::ToolMissing("npm"))?;
        Ok(Self { node, npm })
    }
}

// ---------------------------------------------------------------------------
// Assets embutidos
// ---------------------------------------------------------------------------

/// Um arquivo do bridge embutido no binario.
#[derive(Debug, Clone, Copy)]
pub struct Asset {
    pub name: &'static str,
    pub contents: &'static str,
}

/// De onde vem os arquivos do bridge.
///
/// Trait e nao constante para o teste poder materializar um bridge falso sem
/// tocar o real, e para o slice do gateway poder, um dia, apontar para um
/// bridge instalado por pacote do sistema.
pub trait BridgeAssets: Send + Sync {
    fn files(&self) -> &[Asset];
}

/// Os arquivos de `bridge/whatsapp/`, embutidos com `include_str!`.
///
/// Embutir (e nao depender de um checkout) e o que faz `garra whatsapp`
/// funcionar numa instalacao por `install.sh`, onde so existe o binario. E o
/// mesmo movimento do Hermes, que copia a bridge para `~/.hermes/scripts`.
#[derive(Debug, Clone, Copy, Default)]
pub struct EmbeddedAssets;

impl BridgeAssets for EmbeddedAssets {
    fn files(&self) -> &[Asset] {
        const FILES: &[Asset] = &[
            Asset {
                name: "bridge.mjs",
                contents: include_str!("../../../../bridge/whatsapp/bridge.mjs"),
            },
            Asset {
                name: "package.json",
                contents: include_str!("../../../../bridge/whatsapp/package.json"),
            },
            Asset {
                name: "package-lock.json",
                contents: include_str!("../../../../bridge/whatsapp/package-lock.json"),
            },
        ];
        FILES
    }
}

/// Hash estavel do conjunto de assets: `sha256(name \0 contents \0 …)`.
///
/// Inclui o **nome** para que renomear um arquivo conte como mudanca, e nao
/// apenas reordenar bytes.
pub fn assets_digest(assets: &dyn BridgeAssets) -> String {
    let mut ctx = ring::digest::Context::new(&ring::digest::SHA256);
    for file in assets.files() {
        ctx.update(file.name.as_bytes());
        ctx.update(b"\0");
        ctx.update(file.contents.as_bytes());
        ctx.update(b"\0");
    }
    hex(ctx.finish().as_ref())
}

fn hex(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        out.push_str(&format!("{b:02x}"));
    }
    out
}

/// O que a materializacao fez.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Materialized {
    /// Ja estava la, com o mesmo hash.
    UpToDate,
    /// Escrito agora (primeira vez, ou hash diferente).
    Written,
}

/// Escreve os assets em `dir` quando faltam ou quando o hash embutido mudou.
///
/// O carimbo (`.garraia-bridge-sha256`) e o que evita reescrever e reinstalar a
/// cada execucao — e o que garante que uma CLI atualizada substitui um bridge
/// velho, em vez de conviver com ele em silencio.
pub fn materialize(dir: &Path, assets: &dyn BridgeAssets) -> Result<Materialized, BridgeError> {
    let digest = assets_digest(assets);
    let stamp = dir.join(STAMP_FILE);

    let current = std::fs::read_to_string(&stamp).ok();
    let all_present = assets.files().iter().all(|f| dir.join(f.name).is_file());
    if all_present && current.as_deref() == Some(digest.as_str()) {
        return Ok(Materialized::UpToDate);
    }

    garraia_common::fs_perms::create_secret_dir(dir).map_err(|e| BridgeError::io(dir, e))?;
    for file in assets.files() {
        let path = dir.join(file.name);
        std::fs::write(&path, file.contents).map_err(|e| BridgeError::io(&path, e))?;
    }
    std::fs::write(&stamp, &digest).map_err(|e| BridgeError::io(&stamp, e))?;
    Ok(Materialized::Written)
}

/// `node_modules` ja existe em `dir`?
pub fn deps_installed(dir: &Path) -> bool {
    dir.join("node_modules").is_dir()
}

/// Roda `npm ci --no-fund --no-audit --progress=false` em `dir`.
///
/// **`ci`, e nao `install`** (F2 da auditoria R4). O `package-lock.json` e
/// versionado e o pin exato do Baileys faz parte do contrato — e o que o
/// bridge reporta em `started.baileys_version`, e e a arvore cujo `integrity`
/// o CI audita. `npm install` pode resolver para outra coisa, o que jogaria
/// fora essa garantia exatamente no unico lugar onde ela protege alguem: a
/// maquina do usuario. `ci` e seguro aqui porque [`materialize`] escreve
/// `package.json` **e** `package-lock.json` do mesmo commit embutido, entao os
/// dois nunca ficam dessincronizados — que e a unica pre-condicao do `ci`.
///
/// Efeito colateral desejado: `npm ci` **apaga e recria** o `node_modules`, o
/// que torna o upgrade de dependencia correto por construcao (F3).
///
/// Este e o **unico** lugar do fluxo em que saida crua de ferramenta externa
/// aparece para o usuario, e so no caminho de falha: as ultimas
/// [`STDERR_TAIL_LINES`] linhas de stderr. Sem isso, "instalacao falhou" e um
/// beco sem saida; com o log inteiro, e ruido que esconde a linha que importa.
pub async fn npm_ci(npm: &Path, dir: &Path) -> Result<(), BridgeError> {
    let mut cmd = Command::new(npm);
    cmd.args(["ci", "--no-fund", "--no-audit", "--progress=false"])
        .current_dir(dir)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped());
    apply_child_env(&mut cmd);

    let child = cmd.spawn().map_err(|e| BridgeError::Spawn {
        program: npm.display().to_string(),
        source: e,
    })?;

    let output = tokio::time::timeout(NPM_INSTALL_TIMEOUT, child.wait_with_output())
        .await
        .map_err(|_| BridgeError::NpmTimeout)?
        .map_err(|e| BridgeError::Spawn {
            program: npm.display().to_string(),
            source: e,
        })?;

    if output.status.success() {
        return Ok(());
    }
    let stderr = String::from_utf8_lossy(&output.stderr);
    let tail: Vec<&str> = stderr
        .lines()
        .rev()
        .take(STDERR_TAIL_LINES)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    Err(BridgeError::NpmInstall {
        code: output
            .status
            .code()
            .map(|c| c.to_string())
            .unwrap_or_else(|| "sinal".into()),
        tail: tail.join("\n"),
    })
}

// ---------------------------------------------------------------------------
// Lancamento
// ---------------------------------------------------------------------------

/// Quem sabe montar o comando do bridge.
///
/// A costura injetavel e o **comando**, nao a pilha de I/O: o teste roda a
/// fixture Python pelo mesmo caminho de `spawn`, de enquadramento e de
/// contencao que o Node usaria. Abstrair mais alto deixaria justamente esse
/// caminho sem cobertura.
pub trait BridgeLauncher: Send + Sync {
    /// Comando pronto (programa, args, cwd). O ambiente e aplicado por
    /// [`BridgeConnection::spawn`], nao aqui.
    fn command(&self) -> Result<Command, BridgeError>;
    /// Linha de comando legivel, para mensagem de erro.
    fn describe(&self) -> String;
    /// Diretorio do bridge, citado em erro de versao de protocolo.
    fn dir(&self) -> PathBuf;
}

/// `node bridge.mjs` no diretorio materializado.
#[derive(Debug, Clone)]
pub struct NodeLauncher {
    node: PathBuf,
    dir: PathBuf,
}

impl NodeLauncher {
    pub fn new(node: impl Into<PathBuf>, dir: impl Into<PathBuf>) -> Self {
        Self {
            node: node.into(),
            dir: dir.into(),
        }
    }
}

impl BridgeLauncher for NodeLauncher {
    fn command(&self) -> Result<Command, BridgeError> {
        let mut cmd = Command::new(&self.node);
        cmd.arg("bridge.mjs").current_dir(&self.dir);
        Ok(cmd)
    }

    fn describe(&self) -> String {
        format!("{} bridge.mjs", self.node.display())
    }

    fn dir(&self) -> PathBuf {
        self.dir.clone()
    }
}

/// Variaveis extras que o Node precisa e que nao estao na allowlist R3.
///
/// `TMPDIR` porque o npm e o Node escrevem temporarios; as demais `LC_*`
/// porque o locale do usuario nao deve mudar so por atravessar este processo.
/// A lista e fechada e curta de proposito — cada nome aqui e uma decisao
/// auditavel, e nenhum deles carrega segredo.
const EXTRA_CHILD_ENV: &[&str] = &[
    "TMPDIR",
    "TEMP",
    "TMP",
    "LC_CTYPE",
    "LC_MESSAGES",
    "LC_NUMERIC",
    "LC_TIME",
    "LANGUAGE",
];

fn apply_child_env(cmd: &mut Command) {
    cmd.env_clear();
    for (key, value) in garraia_common::safety_gate::allowed_child_env() {
        cmd.env(key, value);
    }
    for key in EXTRA_CHILD_ENV {
        if let Some(value) = std::env::var_os(key) {
            cmd.env(key, value);
        }
    }
}

#[cfg(any(target_os = "linux", target_os = "android"))]
fn apply_parent_death_signal(cmd: &mut Command) {
    // SAFETY: `prctl` e async-signal-safe e so afeta o filho.
    unsafe {
        cmd.pre_exec(|| {
            libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGTERM);
            Ok(())
        });
    }
}

/// Conexao viva com o bridge.
pub struct BridgeConnection {
    child: Child,
    /// `Option` porque **fechar o stdin e parte do protocolo**: EOF no stdin
    /// significa `shutdown` (flush final e saida 0). Segurar a ponta de
    /// escrita ate o fim do processo prende o filho num `read` que nunca
    /// retorna — foi exatamente assim que a fixture Python travou no
    /// encerramento do interpretador.
    stdin: Option<ChildStdin>,
    stdout: BufReader<ChildStdout>,
    stderr_tail: mpsc::UnboundedReceiver<String>,
    tail: VecDeque<String>,
    dir: PathBuf,
}

impl BridgeConnection {
    /// Lanca o bridge com a contencao descrita no topo do modulo.
    pub async fn spawn(launcher: &dyn BridgeLauncher) -> Result<Self, BridgeError> {
        let mut cmd = launcher.command()?;
        cmd.stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            // O filho nao deve sobreviver a este processo nem quando ele morre
            // de forma que nao roda `Drop`.
            .kill_on_drop(true);
        apply_child_env(&mut cmd);
        #[cfg(any(target_os = "linux", target_os = "android"))]
        apply_parent_death_signal(&mut cmd);

        let mut child = cmd.spawn().map_err(|e| BridgeError::Spawn {
            program: launcher.describe(),
            source: e,
        })?;

        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| BridgeError::Protocol("stdin do bridge indisponivel".into()))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| BridgeError::Protocol("stdout do bridge indisponivel".into()))?;
        let stderr = child
            .stderr
            .take()
            .ok_or_else(|| BridgeError::Protocol("stderr do bridge indisponivel".into()))?;

        // stderr e drenado numa task para o filho nunca bloquear num pipe
        // cheio. Guardamos so a cauda: stderr do bridge e reservado a erro
        // fatal de inicializacao, e o resto do diagnostico vem por evento `log`.
        let (tx, rx) = mpsc::unbounded_channel();
        tokio::spawn(async move {
            let mut lines = BufReader::new(stderr).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                if tx.send(line).is_err() {
                    break;
                }
            }
        });

        Ok(Self {
            child,
            stdin: Some(stdin),
            stdout: BufReader::new(stdout),
            stderr_tail: rx,
            tail: VecDeque::with_capacity(STDERR_TAIL_LINES),
            dir: launcher.dir(),
        })
    }

    /// Envia um comando como uma linha NDJSON.
    pub async fn send(&mut self, command: &BridgeCommand) -> Result<(), BridgeError> {
        let mut line = serde_json::to_string(command)
            .map_err(|e| BridgeError::Protocol(format!("comando nao serializa: {e}")))?;
        line.push('\n');
        let stdin = self
            .stdin
            .as_mut()
            .ok_or_else(|| BridgeError::Protocol("stdin do bridge ja foi fechado".into()))?;
        stdin
            .write_all(line.as_bytes())
            .await
            .map_err(|e| BridgeError::Spawn {
                program: "stdin do bridge".into(),
                source: e,
            })?;
        stdin.flush().await.map_err(|e| BridgeError::Spawn {
            program: "stdin do bridge".into(),
            source: e,
        })?;
        Ok(())
    }

    /// Fecha o stdin do filho — o `shutdown` implicito do protocolo.
    pub fn close_stdin(&mut self) {
        self.stdin = None;
    }

    /// Le o proximo evento. `Ok(None)` = o bridge fechou o stdout.
    ///
    /// Linha maior que [`MAX_FRAME_BYTES`] e erro de protocolo e encerra a
    /// leitura: o leitor nao pode crescer sem teto esperando um `\n`.
    pub async fn next_event(&mut self) -> Result<Option<BridgeEvent>, BridgeError> {
        loop {
            let mut buf = Vec::new();
            let read = read_line_capped(&mut self.stdout, &mut buf, MAX_FRAME_BYTES).await?;
            if read == 0 {
                return Ok(None);
            }
            let text = String::from_utf8_lossy(&buf);
            let trimmed = text.trim();
            if trimmed.is_empty() {
                continue;
            }
            let event: BridgeEvent = serde_json::from_str(trimmed).map_err(|e| {
                // A mensagem NAO inclui a linha: ela pode carregar a sessao.
                BridgeError::Protocol(format!(
                    "linha de {} bytes no stdout nao e um evento valido: {e}",
                    trimmed.len()
                ))
            })?;
            return Ok(Some(event));
        }
    }

    /// Valida o `started` inicial. Deve ser a primeira leitura.
    pub async fn expect_started(&mut self) -> Result<BridgeEvent, BridgeError> {
        match self.next_event().await? {
            Some(ev @ BridgeEvent::Started { protocol, .. }) => {
                if protocol != PROTOCOL_VERSION {
                    return Err(BridgeError::ProtocolVersion {
                        found: protocol,
                        dir: self.dir.display().to_string(),
                    });
                }
                Ok(ev)
            }
            Some(other) => Err(BridgeError::Protocol(format!(
                "o bridge mandou {} antes do `started`",
                event_name(&other)
            ))),
            None => Err(BridgeError::Protocol(format!(
                "o bridge fechou sem mandar `started`{}",
                self.stderr_hint()
            ))),
        }
    }

    /// Cauda do stderr, ja formatada para mensagem de erro.
    pub fn stderr_hint(&mut self) -> String {
        while let Ok(line) = self.stderr_tail.try_recv() {
            if self.tail.len() == STDERR_TAIL_LINES {
                self.tail.pop_front();
            }
            self.tail.push_back(line);
        }
        if self.tail.is_empty() {
            String::new()
        } else {
            format!(
                "\nUltimas linhas do bridge:\n{}",
                self.tail.iter().cloned().collect::<Vec<_>>().join("\n")
            )
        }
    }

    /// Espera o filho terminar e devolve o codigo.
    ///
    /// Fecha o stdin antes: sem isso o filho fica preso num `read` que nunca
    /// retorna e este `wait` nunca volta.
    pub async fn wait(&mut self) -> Result<Option<i32>, BridgeError> {
        self.close_stdin();
        let status = self.child.wait().await.map_err(|e| BridgeError::Spawn {
            program: "wait".into(),
            source: e,
        })?;
        Ok(status.code())
    }

    /// Mata o filho. Idempotente.
    pub async fn kill(&mut self) {
        self.close_stdin();
        let _ = self.child.start_kill();
        let _ = self.child.wait().await;
    }
}

impl Drop for BridgeConnection {
    fn drop(&mut self) {
        // Rede de seguranca sobre o `kill_on_drop`: um filho que sobrevive a
        // esta CLI continua segurando a sessao do WhatsApp aberta.
        let _ = self.child.start_kill();
    }
}

fn event_name(ev: &BridgeEvent) -> &'static str {
    match ev {
        BridgeEvent::Started { .. } => "started",
        BridgeEvent::Qr { .. } => "qr",
        BridgeEvent::Status { .. } => "status",
        BridgeEvent::Authenticated => "authenticated",
        BridgeEvent::Connected { .. } => "connected",
        BridgeEvent::Disconnected { .. } => "disconnected",
        BridgeEvent::LoggedOut => "logged_out",
        BridgeEvent::SessionUpdate { .. } => "session_update",
        BridgeEvent::Message(_) => "message",
        BridgeEvent::Sent { .. } => "sent",
        BridgeEvent::Error { .. } => "error",
        BridgeEvent::Log { .. } => "log",
        BridgeEvent::Unknown => "<desconhecido>",
    }
}

/// `read_until(b'\n')` com teto. Devolve o numero de bytes lidos (0 = EOF).
///
/// `tokio::io::Lines` nao tem teto; um bridge com defeito (ou hostil) que
/// nunca mande `\n` faria o leitor alocar ate o OOM.
async fn read_line_capped(
    reader: &mut BufReader<ChildStdout>,
    buf: &mut Vec<u8>,
    cap: usize,
) -> Result<usize, BridgeError> {
    let mut total = 0usize;
    loop {
        let available = reader.fill_buf().await.map_err(|e| BridgeError::Spawn {
            program: "stdout do bridge".into(),
            source: e,
        })?;
        if available.is_empty() {
            return Ok(total);
        }
        match available.iter().position(|&b| b == b'\n') {
            Some(idx) => {
                buf.extend_from_slice(&available[..idx]);
                total += idx + 1;
                reader.consume(idx + 1);
                if buf.len() > cap {
                    return Err(BridgeError::Protocol(format!(
                        "linha de {} bytes excede o teto de {cap}",
                        buf.len()
                    )));
                }
                return Ok(total);
            }
            None => {
                let len = available.len();
                buf.extend_from_slice(available);
                total += len;
                reader.consume(len);
                if buf.len() > cap {
                    return Err(BridgeError::Protocol(format!(
                        "linha passou do teto de {cap} bytes sem terminar"
                    )));
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct FakeAssets(&'static [Asset]);
    impl BridgeAssets for FakeAssets {
        fn files(&self) -> &[Asset] {
            self.0
        }
    }

    const A: &[Asset] = &[
        Asset {
            name: "bridge.mjs",
            contents: "console.log('a')\n",
        },
        Asset {
            name: "package.json",
            contents: "{\"name\":\"a\"}\n",
        },
    ];
    const B: &[Asset] = &[
        Asset {
            name: "bridge.mjs",
            contents: "console.log('b')\n",
        },
        Asset {
            name: "package.json",
            contents: "{\"name\":\"a\"}\n",
        },
    ];

    #[test]
    fn digest_changes_with_contents_and_with_names() {
        let a = assets_digest(&FakeAssets(A));
        let b = assets_digest(&FakeAssets(B));
        assert_ne!(a, b);
        assert_eq!(a.len(), 64, "sha256 em hex");
        assert_eq!(a, assets_digest(&FakeAssets(A)), "estavel");

        const RENAMED: &[Asset] = &[
            Asset {
                name: "outro.mjs",
                contents: "console.log('a')\n",
            },
            Asset {
                name: "package.json",
                contents: "{\"name\":\"a\"}\n",
            },
        ];
        assert_ne!(a, assets_digest(&FakeAssets(RENAMED)));
    }

    #[test]
    fn materialize_writes_once_then_is_a_noop() {
        let dir = tempfile::tempdir().expect("tempdir");
        let target = dir.path().join("bridge");

        assert_eq!(
            materialize(&target, &FakeAssets(A)).expect("first"),
            Materialized::Written
        );
        assert_eq!(
            std::fs::read_to_string(target.join("bridge.mjs")).expect("read"),
            "console.log('a')\n"
        );
        assert_eq!(
            materialize(&target, &FakeAssets(A)).expect("second"),
            Materialized::UpToDate
        );
    }

    #[test]
    fn a_changed_embedded_hash_rewrites_the_directory() {
        let dir = tempfile::tempdir().expect("tempdir");
        let target = dir.path().join("bridge");
        materialize(&target, &FakeAssets(A)).expect("first");

        assert_eq!(
            materialize(&target, &FakeAssets(B)).expect("upgrade"),
            Materialized::Written
        );
        assert_eq!(
            std::fs::read_to_string(target.join("bridge.mjs")).expect("read"),
            "console.log('b')\n"
        );
    }

    #[test]
    fn a_deleted_file_is_restored_even_when_the_stamp_matches() {
        let dir = tempfile::tempdir().expect("tempdir");
        let target = dir.path().join("bridge");
        materialize(&target, &FakeAssets(A)).expect("first");
        std::fs::remove_file(target.join("bridge.mjs")).expect("rm");

        assert_eq!(
            materialize(&target, &FakeAssets(A)).expect("repair"),
            Materialized::Written
        );
        assert!(target.join("bridge.mjs").is_file());
    }

    #[cfg(unix)]
    #[test]
    fn the_bridge_directory_is_owner_only() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().expect("tempdir");
        let target = dir.path().join("bridge");
        materialize(&target, &FakeAssets(A)).expect("write");
        let mode = std::fs::metadata(&target)
            .expect("stat")
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o700);
    }

    #[test]
    fn deps_installed_tracks_node_modules() {
        let dir = tempfile::tempdir().expect("tempdir");
        assert!(!deps_installed(dir.path()));
        std::fs::create_dir(dir.path().join("node_modules")).expect("mkdir");
        assert!(deps_installed(dir.path()));
    }

    /// A allowlist e o contrato de seguranca mais importante do modulo: nenhum
    /// segredo do processo pai pode estar nela.
    #[test]
    fn the_child_env_allowlist_carries_no_secret() {
        let all: Vec<&str> = garraia_common::safety_gate::R3_ENV_ALLOWLIST
            .iter()
            .copied()
            .chain(EXTRA_CHILD_ENV.iter().copied())
            .collect();
        for name in &all {
            let upper = name.to_uppercase();
            for forbidden in ["KEY", "SECRET", "TOKEN", "PASS", "CREDENTIAL", "GARRAIA"] {
                assert!(
                    !upper.contains(forbidden),
                    "{name} parece carregar segredo e nao pode estar na allowlist"
                );
            }
        }

        // F4 da auditoria R4: a varredura por segredo acima nao pega o vetor
        // que de fato importa aqui. Estas variaveis nao VAZAM nada — elas dao
        // **execucao de codigo** dentro do filho, carregando biblioteca ou
        // modulo escolhido por quem conseguir escreve-las no ambiente do
        // gateway. Nenhuma delas casa com "KEY|SECRET|TOKEN|...", entao
        // precisam ser proibidas pelo nome.
        const CODE_EXECUTION_VECTORS: &[&str] = &[
            "NODE_OPTIONS",
            "NODE_PATH",
            "NODE_REPL_EXTERNAL_MODULE",
            "LD_PRELOAD",
            "LD_LIBRARY_PATH",
            "LD_AUDIT",
            "DYLD_INSERT_LIBRARIES",
            "DYLD_LIBRARY_PATH",
        ];
        for vector in CODE_EXECUTION_VECTORS {
            assert!(
                !all.iter().any(|name| name.eq_ignore_ascii_case(vector)),
                "{vector} da execucao de codigo no filho e nao pode ser herdada"
            );
        }

        // `NPM_CONFIG_*` e familia inteira, nao nome unico: `NPM_CONFIG_SCRIPT_SHELL`
        // troca o shell dos lifecycle scripts e `NPM_CONFIG_REGISTRY` troca de onde
        // o pacote vem. Prefixo, entao.
        for name in &all {
            assert!(
                !name.to_uppercase().starts_with("NPM_CONFIG_"),
                "{name} reconfigura o npm (registry, script-shell) e nao pode ser herdada"
            );
        }
        assert!(all.contains(&"PATH"), "node precisa de PATH");
        assert!(all.contains(&"HOME"), "node precisa de HOME");
    }

    /// O teste acima afirma o **conteudo** de duas constantes. Este afirma o
    /// **comportamento**, e e o unico do modulo que morre se alguem apagar a
    /// linha `cmd.env_clear()` de [`apply_child_env`]: sem ela a allowlist
    /// deixa de ser allowlist e vira lista decorativa — o filho herda tudo,
    /// inclusive os oito vetores de execucao de codigo que o teste acima
    /// proibe por nome.
    ///
    /// **Por que nao `Command::get_envs()`**: ele e cego a `env_clear()`. O
    /// `CommandEnv` guarda o flag `clear` separado do mapa de variaveis e o
    /// iterador so expoe o mapa, entao `get_envs()` devolve exatamente a mesma
    /// coisa com e sem `env_clear()` — uma asserção sobre ele passaria nos dois
    /// lados da mutacao. So um filho de verdade responde a pergunta.
    ///
    /// Um unico teste, com a escrita do ambiente imediatamente antes do
    /// `spawn`: `set_var` e global ao processo e nenhum outro teste deste
    /// binario lanca processo.
    #[cfg(unix)]
    #[tokio::test]
    async fn the_child_does_not_inherit_the_parent_environment() {
        // Canarios: um segredo e dois vetores de execucao de codigo. Nenhum
        // deles esta na allowlist, entao nenhum pode chegar ao filho.
        const CANARIES: &[(&str, &str)] = &[
            ("GARRAIA_JWT_SECRET", "canario-jwt"),
            ("NODE_OPTIONS", "--require=/tmp/canario.js"),
            ("NPM_CONFIG_REGISTRY", "http://canario.invalido"),
        ];

        let Some(env_bin) = ["/usr/bin/env", "/bin/env"]
            .into_iter()
            .map(Path::new)
            .find(|p| p.is_file())
        else {
            // Sem `env(1)` nao ha como perguntar ao filho o que ele herdou.
            return;
        };

        // SAFETY: `set_var` e global ao processo de teste; nenhum outro teste
        // deste binario lanca processo filho nem le estas variaveis.
        unsafe {
            for (key, value) in CANARIES {
                std::env::set_var(key, value);
            }
        }

        let mut cmd = Command::new(env_bin);
        apply_child_env(&mut cmd);
        let output = cmd.output().await.expect("env(1)");

        // SAFETY: idem.
        unsafe {
            for (key, _) in CANARIES {
                std::env::remove_var(key);
            }
        }

        let seen = String::from_utf8_lossy(&output.stdout);
        for (key, _) in CANARIES {
            assert!(
                !seen
                    .lines()
                    .any(|line| line.starts_with(&format!("{key}="))),
                "{key} chegou ao filho — `env_clear()` nao esta sendo aplicado:\n{seen}"
            );
        }
        // E o outro lado: a allowlist tem de continuar entregando o minimo.
        assert!(
            seen.lines().any(|line| line.starts_with("PATH=")),
            "o filho ficou sem PATH:\n{seen}"
        );
    }

    #[test]
    fn a_missing_tool_is_reported_by_name() {
        let err = BridgeError::ToolMissing("node");
        assert!(err.to_string().contains("node"));
    }
}
