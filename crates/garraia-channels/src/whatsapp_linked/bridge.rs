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

/// Teto de caracteres por linha de stderr repassada ao usuario.
///
/// Stack trace de Node cabe folgado; despejo de credencial, nao.
const STDERR_TAIL_LINE_CHARS: usize = 240;

/// A partir de quantos caracteres uma sequencia continua deixa de parecer
/// identificador e passa a parecer material cifrado.
///
/// 40 e folgado para nome de funcao, de modulo e de segmento de caminho.
const BASE64_RUN_MIN: usize = 40;

/// Faz parte da sequencia continua que a redacao julga como UMA unidade?
///
/// `.` e `@` ficam **de fora**: eles sao separadores, e e essa a regra que
/// decide o que a isencao cobre.
///
/// # A medicao que trocou a regra, duas vezes
///
/// **A barra entra na sequencia.** A versao original varria `[A-Za-z0-9+=]` —
/// sem a barra, para nao redigir um caminho inteiro. So que a barra e 1 em 64
/// caracteres do alfabeto base64 padrao, entao ela quebrava a propria
/// credencial em pedacos curtos. Medido sobre 2000 chaves de 32 B: o maior
/// pedaco em claro tinha **17 dos 44 caracteres** em media, e em ~35% dos
/// casos a chave inteira sobrevivia. O teto de 240 caracteres nao salvava nada
/// — 240 caracteres tambem sao 240 caracteres de credencial. O
/// `crash-with-secret` da fixture e exatamente esse caso, e ele chegava a tela
/// inteiro.
///
/// **O ponto sai da sequencia.** A versao seguinte pos `/`, `.`, `@`, `-` e
/// `_` **dentro** da sequencia e isentava quem tivesse `.` ou `@` — marcas que
/// nenhum alfabeto base64 produz. O erro estava em julgar a sequencia
/// INTEIRA: `.` e `=` estavam os dois dentro dela, entao `state.creds=<chave>`
/// era UMA sequencia, o ponto do nome vizinho a isentava, e a chave saia
/// junto. E o mesmo mecanismo ja descrito abaixo para `-`/`_` — "um nome de
/// chave vizinho cola na sequencia" —, que so nao tinha sido aplicado ao `.`.
///
/// Medido sobre 2000 chaves de 32 B por celula, nos dois alfabetos (fracao de
/// casos em que a chave INTEIRA chega a tela):
///
/// | contexto | antes | depois |
/// |---|---|---|
/// | `noiseKey=<k>` | 0,0% | 0,0% |
/// | `npm ERR! _auth=<k>` | 0,0% | 0,0% |
/// | `creds.noiseKey=<k>` | **100,0%** | 0,0% |
/// | `at state.creds=<k>` | **100,0%** | 0,0% |
///
/// O maior pedaco em claro cai de 44,00 para ~1,3 caracteres — ruido, nao
/// credencial. O conserto e nao deixar `.` e `@` entrarem na sequencia: eles
/// viram separadores, e a isencao passa a valer por SEGMENTO. Como um segmento
/// nunca contem `.` nem `@`, "isento" vira simplesmente "curto demais para ser
/// credencial", e nao ha mais teste de isencao nenhum em [`redact_tail_line`].
///
/// # Por que a lista de separadores nao cresce
///
/// Todo caractere que vira separador quebra a credencial junto. Medido sobre
/// 2000 chaves de 32 B por celula, chave inteira em claro:
///
/// | se tambem separassem | base64 padrao | base64 url-safe |
/// |---|---|---|
/// | `-` e `_` | 0,0% | **~62%** |
/// | `/` | **~35%** | 0,0% |
///
/// Cada um cobre um alfabeto inteiro: `-`/`_` sao o url-safe, `/` e o padrao.
/// A decisao de nao isentar `/` "junto de" `-`/`_` — combinacao que nenhum
/// alfabeto base64 produz sozinha — ja foi medida tres vezes por agentes
/// independentes e converge no mesmo lugar (~48% contra 0%; analitico
/// `1 - (63/64)^43` ~= 49%), porque a chave e o nome vizinho entram no MESMO
/// segmento e basta a chave conter uma barra para a combinacao se formar.
/// Esta fechada.
///
/// `%` e `\` tambem ficam de fora — `100%` e o caminho do Windows dependem
/// disso —, mas as CODIFICACOES `%2B`/`%2F`/`%3D` e `\/` nao quebram o
/// segmento: quem decide isso e [`segment_unit`], desde a #1276.
///
/// # O preco, medido
///
/// Um segmento longo **sem `.` e sem `@`** vira `<redigido: N caracteres>`,
/// mesmo sendo caminho legitimo. Sobre um corpus de 14 linhas reais de erro de
/// Node/npm, 10 saem identicas ao que saiam antes — `npm ERR! code ELIFECYCLE`,
/// o 404 do registry, o `EACCES … mkdir`, o `ECONNREFUSED` — e 4 perdem UM
/// segmento do meio do caminho. Nas tres primeiras o que se perde e o prefixo
/// de instalacao e o que fica e a parte informativa:
///
/// ```text
/// at /home/user/.<redigido: 40 caracteres>@whiskeysockets/baileys/lib/index.js:42:7
/// ```
///
/// Na quarta o corte cai do lado ruim: em
/// `(/app/node_modules/@whiskeysockets/baileys/lib/Socket/socket.js:118:23)` o
/// segmento que segue o `@` tem exatamente 40 caracteres, e e ELE que some —
/// sobra `@<redigido: 40 caracteres>.js:118:23`. A linha continua dizendo que
/// houve um caminho, qual a extensao e qual a posicao, mas o nome do modulo
/// vai junto. E o preco aceito: do outro lado da troca esta 100% da chave.
fn is_run_char(ch: char) -> bool {
    ch.is_ascii_alphanumeric() || matches!(ch, '+' | '/' | '=' | '_' | '-')
}

/// Os tres caracteres do base64 padrao que uma URL nao aceita crus — `+`, `/`
/// e `=` — na forma percent-encoded. Comparados sem distinguir caixa.
const PERCENT_ENCODED_BASE64: &[&[u8; 2]] = &[b"2B", b"2F", b"3D"];

/// Uma unidade de segmento no comeco de `rest`: quantos bytes ela ocupa no
/// texto. `None` quando o que vem a seguir e separador.
///
/// Alem do caractere solto que [`is_run_char`] aceita, duas CODIFICACOES de um
/// caractere base64 contam como um caractere so — e e o comprimento
/// decodificado que [`redact_tail_line`] compara com [`BASE64_RUN_MIN`] e
/// escreve no marcador:
///
/// - `%2B`, `%2F` e `%3D` (qualquer caixa), o percent-encoding de `+`, `/` e
///   `=`. E a forma em que uma chave aparece na URL de um `FetchError` do
///   undici: `https://mmg.whatsapp.net/v/t62.7118-24/<chave>?ccb=11-4`.
/// - `\/`, a barra escapada de um `JSON.stringify`.
///
/// # A medicao que criou esta funcao (#1276, item 2)
///
/// `%` e `\` nao sao caracteres de segmento e nao podem virar: `%` separa
/// `100%` de qualquer coisa e `\` e o separador de caminho do Windows. So que
/// eles caiam no MEIO da credencial codificada — `%2F` no lugar de cada `/`,
/// `%3D` no lugar do `=` final — e cada ocorrencia quebrava a sequencia em
/// pedacos curtos demais para o teto de 40. Medido sobre chaves aleatorias de
/// 32 B na linha do `FetchError`: **62,9%** das chaves percent-encoded e
/// **40,8%** das chaves com `\/` chegavam INTEIRAS a tela — bastava um
/// url-decode (ou um unescape) do texto ja redigido para remonta-las. O corpus
/// de `encoded_credentials_in_a_fetch_error_url_are_never_recoverable`
/// reproduz a medicao (61,0% e 37,6% sobre 500 chaves com semente fixa) e
/// exige zero.
///
/// As saidas obvias foram medidas na issue e descartadas: mexer em
/// [`BASE64_RUN_MIN`] nao muda a fracao, porque o problema e onde o segmento
/// QUEBRA e nao o tamanho dele; exigir mistura de classes de caractere deixa
/// 0,07% das chaves em claro. O conserto e o tokenizador enxergar a
/// codificacao: `%2F` e UM caractere do segmento, e nao tres, e um segmento
/// abaixo do teto sai como entrou — codificado, sem decodificar nada. Um `%`
/// seguido de qualquer outra coisa (`%20`, `%25`, `100%`, fim de linha) e um
/// `\` seguido de qualquer coisa que nao `/` continuam separadores.
///
/// O preco novo e pequeno e da mesma natureza do que ja se paga: um segmento
/// de URL percent-encoded com 40+ caracteres decodificados e sem `.` vira
/// `<redigido>`. `@whiskeysockets%2Fbaileys` tem 23 e continua legivel.
fn segment_unit(rest: &str) -> Option<usize> {
    let bytes = rest.as_bytes();
    match *bytes.first()? {
        b'%' => bytes
            .get(1..3)
            .filter(|hex| {
                PERCENT_ENCODED_BASE64
                    .iter()
                    .any(|enc| hex.eq_ignore_ascii_case(&enc[..]))
            })
            .map(|_| 3),
        b'\\' => (bytes.get(1) == Some(&b'/')).then_some(2),
        c if c.is_ascii() && is_run_char(char::from(c)) => Some(1),
        _ => None,
    }
}

/// Redige o que parece material cifrado numa linha de stderr do filho.
///
/// # Por que existe
///
/// `stderr_hint` repassa a cauda do stderr do Node **verbatim** para o
/// terminal, e ela e a unica saida crua de ferramenta externa deste fluxo. O
/// lado JS e cuidadoso — `pino` silencioso, `describeError` redigindo JID e
/// digitos —, mas `describeError` nao cobre base64 de credencial, e um
/// `throw` de dentro do Baileys, de um `JSON.stringify` de estado ou de um
/// modulo de terceiros nao passa por ele. Nada mais redigia este caminho.
///
/// A regra e conservadora nos dois sentidos: corta **cada segmento** longo
/// demais para ser nome **e** limita o comprimento da linha, porque nenhuma
/// das duas sozinha fecha o caso — uma linha truncada em 240 caracteres ainda
/// seriam 240 caracteres de credencial. Onde um segmento comeca e acaba e
/// decisao de [`is_run_char`] (o caractere solto) e de [`segment_unit`] (as
/// codificacoes `%2B`/`%2F`/`%3D` e `\/`, que contam como um caractere); o que
/// se compara com [`BASE64_RUN_MIN`] e o que vai no marcador e o comprimento
/// DECODIFICADO do segmento, e um segmento curto sai como entrou, sem
/// decodificar.
///
/// # O que fica de fora, por escrito
///
/// Credencial com menos de [`BASE64_RUN_MIN`] caracteres decodificados nao e
/// redigida. E o limite declarado da regra, e nao um residual a fechar: uma
/// chave de 32 B tem 43 ou 44, e baixar o teto passa a redigir nome de funcao,
/// de modulo e de segmento de caminho — o que a cauda existe para mostrar.
///
/// Ela nao e o unico controle, e nao pode ser testada so como funcao pura: os
/// dois call sites — [`BridgeConnection::stderr_hint`] e [`npm_ci`] — sao o
/// que de fato leva a redacao a tela, e ha teste de ponta a ponta para cada
/// um. Neutraliza-los deixava a suite inteira verde.
fn redact_tail_line(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    // Segmento em curso: onde comeca (indice de byte em `line`) e quantos
    // caracteres DECODIFICADOS ja tem. E esse segundo numero, e nao o tamanho
    // do texto, que se compara com o teto e que vai para o marcador.
    let mut run: Option<(usize, usize)> = None;
    let flush = |run: &mut Option<(usize, usize)>, end: usize, out: &mut String| {
        let Some((start, decoded)) = run.take() else {
            return;
        };
        if decoded >= BASE64_RUN_MIN {
            out.push_str(&format!("<redigido: {decoded} caracteres>"));
        } else {
            out.push_str(&line[start..end]);
        }
    };

    // Anda por bytes, mas so para em fronteira de caractere: cada unidade que
    // `segment_unit` devolve e ASCII inteira (1, 2 ou 3 bytes) e o separador
    // avanca o tamanho UTF-8 do proprio caractere.
    let mut i = 0;
    while let Some(ch) = line[i..].chars().next() {
        if let Some(width) = segment_unit(&line[i..]) {
            let (_, decoded) = run.get_or_insert((i, 0));
            *decoded += 1;
            i += width;
        } else {
            flush(&mut run, i, &mut out);
            out.push(ch);
            i += ch.len_utf8();
        }
    }
    flush(&mut run, line.len(), &mut out);

    if out.chars().count() > STDERR_TAIL_LINE_CHARS {
        let cut: String = out.chars().take(STDERR_TAIL_LINE_CHARS).collect();
        return format!("{cut}… (linha truncada)");
    }
    out
}

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

    /// Nome de asset que nao e um unico segmento simples de arquivo —
    /// recusado pela allowlist de [`validar_nome_de_asset`] antes de
    /// qualquer escrita em disco (alerta CodeQL 174, path-injection).
    #[error("nome de asset invalido: {0:?}")]
    AssetName(String),
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

/// Accept-list de nome de asset: um unico segmento simples de arquivo.
///
/// `/`, `\`, `..`, `.` sozinho, NUL, espaco e Unicode fora de `[A-Za-z0-9._-]`
/// saem todos pela recusa — o que fecha traversal, absoluto e subdiretorio de
/// uma vez, no padrao do `validate_account` da sessao (`session.rs`). O teto
/// de 255 bytes e o limite de nome de arquivo dos filesystems de sempre.
///
/// Por que validacao e nao supressao: o alerta 174 aponta para o sink de
/// [`materialize`], e o trait `BridgeAssets` existe para aceitar futuras
/// fontes de bridge — o sink tem de ser seguro para QUALQUER implementacao,
/// nao apenas para a de hoje.
fn validar_nome_de_asset(name: &str) -> Result<(), BridgeError> {
    let simples = !name.is_empty()
        && name != "."
        && name != ".."
        && name.len() <= 255
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b'_'));
    if simples {
        Ok(())
    } else {
        Err(BridgeError::AssetName(name.to_string()))
    }
}

/// Escreve os assets em `dir` quando faltam ou quando o hash embutido mudou.
///
/// O carimbo (`.garraia-bridge-sha256`) e o que evita reescrever e reinstalar a
/// cada execucao — e o que garante que uma CLI atualizada substitui um bridge
/// velho, em vez de conviver com ele em silencio.
pub fn materialize(dir: &Path, assets: &dyn BridgeAssets) -> Result<Materialized, BridgeError> {
    // Alerta CodeQL 174 (path-injection): o sink `dir.join(file.name)` esta
    // logo abaixo, e o trait `BridgeAssets` e publico justamente para um dia
    // servir a um bridge vindo de fora do binario. A unica impl de hoje embute
    // os nomes com `include_str!`, mas o sink valida por si: todos os nomes
    // passam pela allowlist ANTES de qualquer efeito colateral — nenhum byte
    // toca o disco enquanto um nome nao passar.
    for file in assets.files() {
        validar_nome_de_asset(file.name)?;
    }

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
        .stderr(Stdio::piped())
        // Sem isto, o `timeout` abaixo devolve `NpmTimeout` e larga o `npm`
        // rodando: depois de 600 s o usuario ficava com um processo orfao
        // mexendo no mesmo `node_modules` que a proxima tentativa vai recriar.
        .kill_on_drop(true);
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
    // Mesma redacao do [`BridgeConnection::stderr_hint`]: e a mesma classe de
    // saida crua indo para a mesma tela.
    let tail: Vec<String> = stderr
        .lines()
        .rev()
        .take(STDERR_TAIL_LINES)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .map(redact_tail_line)
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
    ///
    /// Cada linha passa por [`redact_tail_line`] **na entrada**, e nao na
    /// formatacao: assim nem a copia guardada carrega o material, e nao ha
    /// segundo caminho por onde ela possa sair sem redacao.
    pub fn stderr_hint(&mut self) -> String {
        while let Ok(line) = self.stderr_tail.try_recv() {
            if self.tail.len() == STDERR_TAIL_LINES {
                self.tail.pop_front();
            }
            self.tail.push_back(redact_tail_line(&line));
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
        BridgeEvent::Pong => "pong",
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

    /// A cauda do stderr do Node nao pode carregar credencial para a tela.
    ///
    /// L2 da auditoria R4: este era o **unico** caminho do fluxo que repassava
    /// saida crua de ferramenta externa verbatim, e nada o redigia. O lado JS
    /// redige JID e digitos, mas nao base64 de creds — e um `throw` de dentro
    /// do Baileys nem passa por la.
    #[test]
    fn the_stderr_tail_never_carries_a_credential_to_the_screen() {
        // Um blob de sessao caindo num stack trace.
        let creds = "e".repeat(64) + &"Z".repeat(80);
        let linha = format!("Error: failed to persist {creds} at Object.<anonymous>");
        let redigida = redact_tail_line(&linha);
        assert!(
            !redigida.contains(&creds),
            "o material saiu inteiro: {redigida}"
        );
        assert!(
            redigida.contains("<redigido:"),
            "e precisa dizer que cortou: {redigida}"
        );
        assert!(
            redigida.starts_with("Error: failed to persist"),
            "o diagnostico em volta tem de sobreviver: {redigida}"
        );

        // **O caso que a versao anterior deixava passar inteiro.** Base64
        // PADRAO tem `/`, e com a barra fora da classe a chave quebrava em
        // pedacos curtos: 17 dos 44 caracteres em claro, em media. Esta e uma
        // `noiseKey` de 32 B na forma exata em que o Baileys a imprime.
        let chave = "c2VjcmV0/Y3JlZGVudGlhbCtub2lzZUtleUJBU0U2ND0=";
        let linha = format!("Error: connection failed noiseKey={chave} at Object.<anonymous>");
        let redigida = redact_tail_line(&linha);
        assert!(
            !redigida.contains(chave),
            "a chave saiu inteira: {redigida}"
        );
        // E nenhum pedaco util dela pode sobrar: meia chave e uma chave
        // vazada pela metade, nao uma chave protegida.
        for janela in chave.as_bytes().windows(12) {
            let pedaco = std::str::from_utf8(janela).expect("ascii");
            assert!(
                !redigida.contains(pedaco),
                "sobrou o pedaco {pedaco:?} da chave: {redigida}"
            );
        }
        assert!(
            redigida.contains("at Object."),
            "o contexto em volta continua legivel: {redigida}"
        );

        // Base64 **url-safe**: `-` e `_` no lugar de `+` e `/`. E por isso que
        // `-` e `_` nao podem virar separadores de segmento: separa-los quebra
        // o alfabeto url-safe inteiro (medido: ~62% das chaves saem inteiras).
        let url_safe = "c2VjcmV0-Y3JlZGVudGlhbCtub2lzZUtleUJBU0U2ND0_";
        let redigida = redact_tail_line(&format!("at connect ({url_safe})"));
        assert!(
            !redigida.contains(url_safe),
            "base64 url-safe saiu inteiro: {redigida}"
        );

        // **A isencao vale por SEGMENTO, e nao pela sequencia inteira.** `.` e
        // `=` estavam ambos DENTRO da sequencia, entao `state.creds=<chave>`
        // era uma sequencia so: o ponto do nome vizinho isentava a chave
        // junto, e ela chegava inteira a tela em 100% dos casos medidos. E o
        // mesmo mecanismo do `_auth` abaixo, que so nao tinha sido aplicado ao
        // `.`. A forma vem de um `throw` de dentro do Baileys ou de um
        // template literal com caminho de propriedade.
        for linha in [
            format!("Error: failed to persist creds.noiseKey={chave} at Object.<anonymous>"),
            format!("TypeError: cannot read at state.creds={chave} (index.js:42:7)"),
            format!("    at Object.<anonymous> (creds.keys.noiseKey={chave})"),
        ] {
            let redigida = redact_tail_line(&linha);
            assert!(
                !redigida.contains(chave),
                "um ponto no nome vizinho nao pode isentar a credencial: {redigida}"
            );
            for janela in chave.as_bytes().windows(12) {
                let pedaco = std::str::from_utf8(janela).expect("ascii");
                assert!(
                    !redigida.contains(pedaco),
                    "sobrou o pedaco {pedaco:?} da chave: {redigida}"
                );
            }
            assert!(
                redigida.contains("<redigido:"),
                "e precisa dizer que cortou: {redigida}"
            );
        }

        // **O motivo de `-` e `_` NAO separarem**, medido: se separassem, o
        // alfabeto url-safe inteiro se quebraria em pedacos curtos. E a linha
        // exata do `fake_npm.py`, e ela ja era pega antes desta rodada.
        let auth = format!("npm ERR! _auth={chave}");
        let redigida = redact_tail_line(&auth);
        assert!(
            !redigida.contains(chave),
            "um nome de chave vizinho com `_` nao pode isentar a credencial: {redigida}"
        );

        // O que a cauda existe para mostrar continua legivel — **o nome do
        // modulo, a extensao e a posicao**. O preco medido da isencao por
        // segmento e que UM segmento longo do meio do caminho (aqui o prefixo
        // de instalacao, de exatamente 40 caracteres) cai; o resto fica. Sobre
        // um corpus de 14 linhas reais de erro de Node/npm, 10 saem identicas
        // ao que saiam antes e 4 perdem um segmento.
        let util = "Error: Cannot find module '/home/user/.local/share/garraia/bridge/node_modules/@whiskeysockets/baileys/lib/index.js'";
        let redigida = redact_tail_line(util);
        assert_eq!(
            redigida,
            "Error: Cannot find module '/home/user/.<redigido: 40 caracteres>@whiskeysockets/baileys/lib/index.js'",
            "o nome do modulo e a extensao sao o que a cauda existe para dizer"
        );
        // Caminho longo com traco e sublinhado: a mesma linha que o
        // `fake_npm.py` cospe, e o `@whiskeysockets/baileys/package.json` que
        // o teste de ponta a ponta do `npm_ci` vigia continua inteiro.
        let pacote = "npm ERR! at /home/user/.local/share/garraia/bridge/node_modules/@whiskeysockets/baileys/package.json";
        let redigida = redact_tail_line(pacote);
        assert!(
            redigida.contains("@whiskeysockets/baileys/package.json"),
            "o pacote e o arquivo tem de continuar legiveis: {redigida}"
        );

        // O falso positivo conhecido e aceito: caminho longo SEM ponto e sem
        // `@`. Erro real de Node nomeia arquivo com extensao ou pacote com
        // escopo; este teste existe para que a troca fique escrita, e nao
        // descoberta por acidente por quem mexer aqui depois.
        let sem_marca = "/home/user/projects/diretorio/muito/longo/sem/ponto/nenhum/entrada";
        assert!(
            redact_tail_line(sem_marca).contains("<redigido:"),
            "esta e a troca medida: sem `.` e sem `@` a sequencia cai inteira"
        );

        // E uma linha absurdamente longa e cortada, porque redigir sequencias
        // sozinho nao fecha o caso: uma linha de milhares de pedacos curtos
        // nao tem nenhuma sequencia longa para redigir.
        let longa = "ab cd ".repeat(200);
        let cortada = redact_tail_line(&longa);
        assert!(
            cortada.chars().count() <= STDERR_TAIL_LINE_CHARS + 20,
            "a linha nao foi truncada: {} caracteres",
            cortada.chars().count()
        );
        assert!(cortada.contains("truncada"), "e precisa dizer que truncou");
    }

    /// PRNG deterministico para o corpus sintetico: xorshift64*, semente fixa.
    ///
    /// Sem dependencia nova e sem aleatoriedade entre execucoes — o teste mede
    /// sempre as MESMAS chaves, entao um numero que mude e mudanca na redacao,
    /// nao no sorteio.
    struct Xorshift64Star(u64);

    impl Xorshift64Star {
        fn next_u64(&mut self) -> u64 {
            let mut x = self.0;
            x ^= x >> 12;
            x ^= x << 25;
            x ^= x >> 27;
            self.0 = x;
            x.wrapping_mul(0x2545_F491_4F6C_DD1D)
        }

        /// 32 bytes aleatorios: o tamanho de uma `noiseKey` do Baileys.
        fn key_32(&mut self) -> [u8; 32] {
            let mut out = [0u8; 32];
            for chunk in out.chunks_mut(8) {
                let word = self.next_u64().to_le_bytes();
                chunk.copy_from_slice(&word[..chunk.len()]);
            }
            out
        }
    }

    /// O que `decodeURIComponent` devolve para a linha do corpus: so `+`, `/`
    /// e `=` sao percent-encoded nela, nas duas caixas.
    fn url_decode(s: &str) -> String {
        s.replace("%2B", "+")
            .replace("%2b", "+")
            .replace("%2F", "/")
            .replace("%2f", "/")
            .replace("%3D", "=")
            .replace("%3d", "=")
    }

    /// O que `JSON.parse` faz com a barra escapada.
    fn unescape_json_slashes(s: &str) -> String {
        s.replace("\\/", "/")
    }

    /// **Item 2 da #1276, reproduzido como corpus.** A chave inteira chegava a
    /// tela em 62,9% dos casos quando vinha percent-encoded (`%2B`/`%2F`/`%3D`,
    /// a forma da URL de um `FetchError` do undici) e em 40,8% quando vinha com
    /// a barra escapada (`\/`, a forma de um `JSON.stringify`), porque `%` e
    /// `\` quebravam o segmento em pedacos curtos demais para o teto — e
    /// bastava url-decode ou unescape do texto ja redigido para remontar a
    /// chave. Aqui a medicao e refeita sobre um corpus deterministico e o
    /// numero exigido e ZERO, nas duas codificacoes; base64 padrao e url-safe
    /// entram como regressao e tem de seguir 100% redigidos.
    #[test]
    fn encoded_credentials_in_a_fetch_error_url_are_never_recoverable() {
        use base64::Engine;

        const CHAVES: usize = 500;

        // A linha que o undici produz quando o download de midia do WhatsApp
        // falha; a chave e o segmento de caminho.
        fn linha(forma: &str) -> String {
            format!(
                "FetchError: request to https://mmg.whatsapp.net/v/t62.7118-24/{forma}?ccb=11-4 failed"
            )
        }

        struct Variante {
            nome: &'static str,
            /// Do base64 padrao para a forma em que a chave aparece na linha.
            codifica: fn(&str) -> String,
            /// O que quem le a tela faria para remontar a chave.
            decodifica: fn(&str) -> String,
        }
        let variantes = [
            Variante {
                nome: "base64 padrao",
                codifica: str::to_owned,
                decodifica: str::to_owned,
            },
            Variante {
                nome: "percent-encoded, hex maiusculo",
                codifica: |k| {
                    k.replace('+', "%2B")
                        .replace('/', "%2F")
                        .replace('=', "%3D")
                },
                decodifica: url_decode,
            },
            Variante {
                nome: "percent-encoded, hex minusculo",
                codifica: |k| {
                    k.replace('+', "%2b")
                        .replace('/', "%2f")
                        .replace('=', "%3d")
                },
                decodifica: url_decode,
            },
            Variante {
                nome: "barra escapada como \\/",
                codifica: |k| k.replace('/', "\\/"),
                decodifica: unescape_json_slashes,
            },
            Variante {
                nome: "base64 url-safe",
                codifica: |k| k.trim_end_matches('=').replace('+', "-").replace('/', "_"),
                decodifica: str::to_owned,
            },
        ];

        let mut rng = Xorshift64Star(0x1276_C0DE_D00D_0001);
        let chaves: Vec<String> = (0..CHAVES)
            .map(|_| base64::engine::general_purpose::STANDARD.encode(rng.key_32()))
            .collect();
        // O corpus precisa exercitar o caso: chave sem `+` nem `/` nao muda de
        // forma ao ser codificada e ja era redigida antes. Esperado ~75% com
        // pelo menos um dos dois (1 - (62/64)^43).
        let com_marca = chaves.iter().filter(|k| k.contains(['+', '/'])).count();
        assert!(
            com_marca * 10 >= CHAVES * 6,
            "corpus fraco: so {com_marca}/{CHAVES} chaves tem `+` ou `/`"
        );

        let mut falhas = Vec::new();
        for v in &variantes {
            let mut recuperaveis = 0usize;
            let mut sem_marcador = 0usize;
            for chave in &chaves {
                let forma = (v.codifica)(chave);
                let redigida = redact_tail_line(&linha(&forma));
                if !redigida.contains("<redigido:") {
                    sem_marcador += 1;
                }
                if (v.decodifica)(&redigida).contains(&(v.decodifica)(&forma)) {
                    recuperaveis += 1;
                }
            }
            if recuperaveis > 0 || sem_marcador > 0 {
                falhas.push(format!(
                    "{}: {recuperaveis}/{CHAVES} chaves ({:.1}%) recuperaveis por inteiro \
apos decodificar a tela; {sem_marcador} linhas sem marcador",
                    v.nome,
                    100.0 * recuperaveis as f64 / CHAVES as f64
                ));
            }
        }
        assert!(
            falhas.is_empty(),
            "a medicao da #1276 ainda reproduz:\n{}",
            falhas.join("\n")
        );
    }

    /// As duas codificacoes que a #1276 mediu contam como UM caractere do
    /// segmento, o `N` do marcador e o comprimento DECODIFICADO, e tudo o mais
    /// que comeca com `%` ou `\` continua separando — `100%`, `%20`, `\n`, o
    /// caminho do Windows.
    #[test]
    fn percent_encoded_and_escaped_base64_count_as_one_char_each() {
        // A mesma imitacao de `noiseKey` da fixture: 45 caracteres, com uma `/`
        // no meio e o `=` final — os dois lugares em que as codificacoes tocam
        // uma chave real com mais frequencia (o `+` entra pelo corpus).
        let chave = "c2VjcmV0/Y3JlZGVudGlhbCtub2lzZUtleUJBU0U2ND0=";
        assert_eq!(chave.len(), 45);
        assert_eq!(chave.matches(['/', '=']).count(), 2);

        // As quatro formas da mesma chave caem no MESMO marcador, com o N
        // decodificado (45) e nao o tamanho do texto (49 ou 46). Os parenteses
        // isolam a chave: com `noiseKey=` colado o N seria 54, porque o nome
        // vizinho entra no segmento — e esse e o comportamento documentado.
        let pct = chave
            .replace('+', "%2B")
            .replace('/', "%2F")
            .replace('=', "%3D");
        let pct_min = chave
            .replace('+', "%2b")
            .replace('/', "%2f")
            .replace('=', "%3d");
        let esc = chave.replace('/', "\\/");
        assert_eq!(pct.len(), 49);
        assert_eq!(esc.len(), 46);
        for forma in [chave.to_owned(), pct, pct_min, esc] {
            assert_eq!(
                redact_tail_line(&format!("at connect ({forma})")),
                "at connect (<redigido: 45 caracteres>)",
                "forma: {forma}"
            );
        }

        // Abaixo do teto a linha sai VERBATIM — a codificacao e preservada, nao
        // decodificada. 39 caracteres decodificados: um a menos que o teto,
        // isolados por parenteses (com `id=` colado seriam 42 e cairiam).
        let curta = &chave[..39];
        let curta_pct = curta.replace('/', "%2F");
        assert!(
            curta_pct.contains("%2F"),
            "o recorte precisa ter a barra codificada: {curta_pct}"
        );
        for forma in [curta_pct, curta.replace('/', "\\/")] {
            let linha = format!("id ({forma}) ok");
            assert_eq!(redact_tail_line(&linha), linha);
        }

        // `%` ou `\` seguidos de qualquer outra coisa continuam separadores:
        // dois segmentos de 30 nao viram um de 60...
        let a = "a".repeat(30);
        let x = "x".repeat(30);
        for sep in [
            "%20", "%25", "%zz", "%2G", "%", "%2", "\\n", "\\\\", "\\", "\\x2F",
        ] {
            let linha = format!("{a}{sep}{x}");
            assert_eq!(redact_tail_line(&linha), linha, "separador {sep:?}");
        }
        // ... e as codificacoes de base64 unem: os mesmos 30+30 viram 61.
        for uniao in ["%2B", "%2F", "%3D", "%2b", "%2f", "%3d", "\\/"] {
            assert_eq!(
                redact_tail_line(&format!("{a}{uniao}{x}")),
                "<redigido: 61 caracteres>",
                "uniao {uniao:?}"
            );
        }

        // Fim de linha no meio de uma codificacao possivel: nem une, nem entra
        // em panico.
        for cauda in ["%", "%2", "%3", "\\"] {
            let linha = format!("{a}{cauda}");
            assert_eq!(redact_tail_line(&linha), linha);
        }

        // Texto fora do ASCII em volta de `%` e `\`: o tokenizador anda por
        // bytes e nao pode cortar um caractere pela metade.
        let acentos = "conexão falhou: 100%é \\ção %2Fé %é2F fim";
        assert_eq!(redact_tail_line(acentos), acentos);

        // O preco NAO subiu para o que a cauda existe para mostrar: o caminho
        // do Windows (`\` seguido de letra separa, como sempre separou) e o
        // pacote com escopo percent-encoded na URL do registry.
        let win = r"Error: Cannot find module 'C:\Users\michel\AppData\Roaming\garraia\bridge\node_modules\@whiskeysockets\baileys\lib\index.js'";
        assert_eq!(redact_tail_line(win), win);
        let registry =
            "npm ERR! 404 Not Found - GET https://registry.npmjs.org/@whiskeysockets%2Fbaileys";
        assert_eq!(redact_tail_line(registry), registry);
    }

    struct FakeAssets<'a>(&'a [Asset]);
    impl BridgeAssets for FakeAssets<'_> {
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
    fn materialize_recusa_nome_de_asset_que_escapa_do_dir() {
        let dir = tempfile::tempdir().expect("tempdir");
        let target = dir.path().join("bridge");

        // Cada nome tenta uma saida diferente do diretorio de destino:
        // traversal relativa, path absoluto, separador do Windows, componente
        // inteiro `..`, subdiretorio que `join` nao achatou o bastante e o
        // NUL que nenhum nome de arquivo honesto carrega. (alerta 174)
        const MALICIOSOS: &[&str] = &[
            "../escapou",
            "/etc/passwd",
            "a\\b",
            "..",
            "sub/dir.mjs",
            "nul\0x",
        ];

        for &name in MALICIOSOS {
            let assets = [Asset {
                name,
                contents: "conteudo\n",
            }];
            let res = materialize(&target, &FakeAssets(&assets));
            assert!(
                matches!(res, Err(BridgeError::AssetName(_))),
                "nome {name:?} tem de ser recusado com AssetName, veio {res:?}"
            );
        }

        // Nada pode ter sido criado antes da recusa: a validacao vem antes de
        // qualquer efeito colateral, entao o dir de destino nem nasce.
        assert!(!target.exists(), "o dir nao pode nascer sem nomes validos");
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
    /// **Por que subconjunto, e nao canarios.** A versao anterior plantava
    /// tres variaveis com `unsafe { std::env::set_var }` e afirmava que elas
    /// nao chegavam ao filho. O `SAFETY` que a acompanhava estava errado: a
    /// condicao de soundness do `setenv` nao e "nenhum outro teste le ESTAS
    /// variaveis" — e que nenhuma outra thread esteja no ambiente, porque o
    /// `setenv` pode realocar o `environ` enquanto outra faz `getenv`, e todo
    /// `tempfile::tempdir()` deste binario le `TMPDIR`. Perguntar ao filho
    /// quais chaves ele tem e comparar com a allowlist nao escreve no
    /// ambiente, nao precisa de `unsafe`, e e mais forte: um `cargo test` ja
    /// carrega dezenas de `CARGO_*` e `RUST*` fora da lista, entao qualquer
    /// vazamento aparece — inclusive os que canario nenhum cobria.
    #[cfg(unix)]
    #[tokio::test]
    async fn the_child_only_gets_what_the_allowlist_names() {
        let env_bin = ["/usr/bin/env", "/bin/env"]
            .into_iter()
            .map(Path::new)
            .find(|p| p.is_file())
            .expect(
                "sem `env(1)` nao ha como perguntar ao filho o que ele herdou. \
Este e um teste de contencao: ele falha em vez de sumir em silencio, que era \
o que o `return` anterior fazia.",
            );

        let mut cmd = Command::new(env_bin);
        apply_child_env(&mut cmd);
        let output = cmd.output().await.expect("env(1)");
        assert!(
            output.status.success(),
            "env(1) falhou: {:?}",
            output.status
        );

        let permitidas: Vec<&str> = garraia_common::safety_gate::R3_ENV_ALLOWLIST
            .iter()
            .copied()
            .chain(EXTRA_CHILD_ENV.iter().copied())
            .collect();

        let seen = String::from_utf8_lossy(&output.stdout);
        let mut intrusas = Vec::new();
        for line in seen.lines() {
            // Linha sem `=` nao e atribuicao: e a continuacao de um valor
            // multilinha. Ignorar isso e o unico jeito de nao confundir o
            // corpo de um valor com o nome de uma variavel.
            let Some((name, _)) = line.split_once('=') else {
                continue;
            };
            if name.is_empty() || !name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_') {
                continue;
            }
            if !permitidas.contains(&name) {
                intrusas.push(name.to_string());
            }
        }
        assert!(
            intrusas.is_empty(),
            "o filho herdou variaveis fora da allowlist — `env_clear()` nao \
esta sendo aplicado: {intrusas:?}"
        );

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
