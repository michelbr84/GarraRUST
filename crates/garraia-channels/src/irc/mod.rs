//! IRC channel implementation for GarraIA.
//!
//! Provides an `IrcChannel` struct that implements the `Channel` trait,
//! using raw TCP/TLS connections with tokio for IRC protocol handling.

pub mod config;

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use async_trait::async_trait;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpStream;
use tokio::sync::{Mutex, mpsc, watch};
use tracing::{error, info};

use crate::traits::{Channel, ChannelStatus};
use garraia_common::{Error, Message, MessageContent, Result};

pub use config::IrcConfig;

/// Com quem o bot fala numa mensagem recebida (#2011).
///
/// O alvo de um `PRIVMSG` recebido e o canal ou, no privado, o **nick do
/// bot** — o mesmo para todo mundo. Quem decide a conversa e este tipo, nao o
/// alvo cru: antes o gateway montava a sessao do alvo, e as conversas
/// privadas de todas as pessoas caiam juntas em `irc-<nick do bot>`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConversaIrc {
    /// Num canal (`#garra`, `&local`): quem esta no canal divide a conversa.
    /// O nome vem como o servidor mandou.
    Canal(String),
    /// No privado: a conversa e com quem mandou. O nick vem normalizado pelo
    /// `CASEMAPPING` do servidor, para a mesma pessoa ser a mesma conversa
    /// (`Ana` e `ana`).
    Privado(String),
}

/// Callback invoked when a PRIVMSG is received from IRC.
///
/// Arguments: `(conversa, nick, user_name, text, delta_tx)`.
/// Return `Err("__blocked__")` to silently drop unauthorized messages.
pub type IrcOnMessageFn = Arc<
    dyn Fn(
            ConversaIrc,
            String,
            String,
            String,
            Option<mpsc::Sender<String>>,
        ) -> Pin<Box<dyn Future<Output = std::result::Result<String, String>> + Send>>
        + Send
        + Sync,
>;

/// Type alias for the shared writer half of the TCP connection.
type SharedWriter = Arc<Mutex<Option<tokio::io::WriteHalf<TcpStream>>>>;

/// IRC channel implementation.
///
/// Uses tokio TCP for persistent connection, handles IRC protocol commands
/// (NICK, USER, JOIN, PRIVMSG, PING/PONG).
pub struct IrcChannel {
    config: IrcConfig,
    status: ChannelStatus,
    on_message: IrcOnMessageFn,
    shutdown_tx: Option<watch::Sender<bool>>,
    writer: SharedWriter,
}

impl IrcChannel {
    /// Create a new `IrcChannel` from config and callback.
    pub fn new(config: IrcConfig, on_message: IrcOnMessageFn) -> Self {
        Self {
            config,
            status: ChannelStatus::Disconnected,
            on_message,
            shutdown_tx: None,
            writer: Arc::new(Mutex::new(None)),
        }
    }

    /// Access the current config.
    pub fn config(&self) -> &IrcConfig {
        &self.config
    }

    /// Send a raw IRC line (appends \r\n).
    ///
    /// #2007: a linha nunca carrega `\r`, `\n` nem `\0` embutidos — o
    /// servidor leria o resto como OUTRO comando (varios servidores separam
    /// em `\r` ou `\n` isolados, nao so em `\r\n`). Quem monta texto vindo
    /// do modelo passa por [`Self::enviar_privmsg`]; esta recusa e a ultima
    /// barreira para qualquer chamador futuro.
    async fn send_raw(writer: &SharedWriter, line: &str) -> Result<()> {
        if line.contains(['\r', '\n', '\0']) {
            return Err(Error::Channel(
                "irc: linha com terminador ou NUL embutido recusada".into(),
            ));
        }
        let mut guard = writer.lock().await;
        let w = guard
            .as_mut()
            .ok_or_else(|| Error::Channel("irc: not connected".into()))?;
        let data = format!("{}\r\n", line);
        w.write_all(data.as_bytes())
            .await
            .map_err(|e| Error::Channel(format!("irc write failed: {e}")))?;
        w.flush()
            .await
            .map_err(|e| Error::Channel(format!("irc flush failed: {e}")))?;
        Ok(())
    }

    /// Send a PRIVMSG to a target (channel or user).
    pub async fn send_privmsg(&self, target: &str, text: &str) -> Result<()> {
        Self::enviar_privmsg(&self.writer, target, text).await
    }

    /// O UNICO caminho de texto do modelo ate o socket (#2007): o
    /// [`Channel::send_message`] e a resposta do turno passam por aqui.
    ///
    /// O texto vira linhas seguras ([`linhas_do_privmsg`]) e cada uma sai como
    /// `PRIVMSG <alvo> :<linha>`; um texto com quebra de linha nunca vira
    /// outro comando (`JOIN`, `QUIT`, `PRIVMSG NickServ ...`). Alvo malformado
    /// ([`alvo_valido`]) e recusado antes de qualquer escrita.
    async fn enviar_privmsg(writer: &SharedWriter, alvo: &str, texto: &str) -> Result<()> {
        if !alvo_valido(alvo) {
            return Err(Error::Channel("irc: alvo de PRIVMSG invalido".into()));
        }
        for linha in linhas_do_privmsg(texto, corpo_max(alvo)) {
            Self::send_raw(writer, &format!("PRIVMSG {alvo} :{linha}")).await?;
        }
        Ok(())
    }
}

#[async_trait]
impl Channel for IrcChannel {
    fn channel_type(&self) -> &str {
        "irc"
    }

    fn display_name(&self) -> &str {
        "IRC"
    }

    async fn connect(&mut self) -> Result<()> {
        if matches!(self.status, ChannelStatus::Connected) {
            return Ok(());
        }

        self.status = ChannelStatus::Connecting;

        let addr = format!("{}:{}", self.config.server, self.config.port);
        info!("irc: connecting to {}", addr);

        // Note: TLS support would require tokio-native-tls or tokio-rustls.
        // For now, plain TCP only. Set use_tls=false.
        if self.config.use_tls {
            return Err(Error::Channel(
                "irc: TLS not yet implemented, set use_tls=false".into(),
            ));
        }

        let stream = TcpStream::connect(&addr)
            .await
            .map_err(|e| Error::Channel(format!("irc: connect failed: {e}")))?;

        let (reader, writer) = tokio::io::split(stream);

        // Store the writer for sending messages
        {
            let mut guard = self.writer.lock().await;
            *guard = Some(writer);
        }

        let writer_ref = Arc::clone(&self.writer);

        // Send NICK and USER
        let nick = self.config.nick.clone();
        Self::send_raw(&writer_ref, &format!("NICK {}", nick)).await?;
        Self::send_raw(&writer_ref, &format!("USER {} 0 * :GarraIA Bot", nick)).await?;

        // Join channels after registration
        let channels = self.config.channels.clone();
        let join_writer = Arc::clone(&writer_ref);

        let (shutdown_tx, mut shutdown_rx) = watch::channel(false);
        self.shutdown_tx = Some(shutdown_tx);

        let on_message = Arc::clone(&self.on_message);
        let ping_writer = Arc::clone(&writer_ref);

        // Spawn the read loop
        tokio::spawn(async move {
            let mut buf_reader = BufReader::new(reader);
            let mut line = String::new();
            let mut registered = false;
            // O que o servidor anuncia no 005; chega no registro, antes de
            // qualquer PRIVMSG.
            let mut suporte = Suporte::default();

            loop {
                line.clear();

                tokio::select! {
                    result = buf_reader.read_line(&mut line) => {
                        match result {
                            Ok(0) => {
                                info!("irc: connection closed by server");
                                return;
                            }
                            Ok(_) => {
                                let trimmed = line.trim_end();

                                // Handle PING/PONG
                                if let Some(token) = trimmed.strip_prefix("PING ") {
                                    // #2007 (auditoria): o token vem do
                                    // servidor; um `\r` ou `\0` no meio dele
                                    // faria o `send_raw` recusar o PONG e o
                                    // laco morrer. Sai sem controle.
                                    let token: String =
                                        token.chars().filter(|c| !c.is_control()).collect();
                                    let pong = format!("PONG {token}");
                                    if let Err(e) = IrcChannel::send_raw(&ping_writer, &pong).await {
                                        error!("irc: failed to send PONG: {e}");
                                        return;
                                    }
                                    continue;
                                }

                                suporte.aplicar_isupport(trimmed);

                                // Detect end of MOTD (RPL_ENDOFMOTD or ERR_NOMOTD)
                                if !registered && (trimmed.contains(" 376 ") || trimmed.contains(" 422 ")) {
                                    registered = true;
                                    // Join configured channels
                                    for ch in &channels {
                                        if let Err(e) = IrcChannel::send_raw(
                                            &join_writer,
                                            &format!("JOIN {}", ch),
                                        ).await {
                                            error!("irc: failed to join {}: {e}", ch);
                                        } else {
                                            info!("irc: joined {}", ch);
                                        }
                                    }
                                }

                                // Handle PRIVMSG
                                // Format: :nick!user@host PRIVMSG #channel :message text
                                if let Some(privmsg_data) = parse_privmsg(trimmed) {
                                    let (source_nick, target, text) = privmsg_data;
                                    if text.trim().is_empty() || source_nick.is_empty() {
                                        continue;
                                    }

                                    info!("irc: PRIVMSG from {} in {}: {} chars", source_nick, target, text.len());

                                    let cb = Arc::clone(&on_message);
                                    let reply_writer = Arc::clone(&ping_writer);
                                    // #2011: canal ou privado se decide num
                                    // ponto so, e a resposta e a sessao seguem
                                    // a mesma decisao.
                                    let conversa = suporte.conversa(&target, &source_nick);
                                    let reply_target = match &conversa {
                                        ConversaIrc::Canal(canal) => canal.clone(),
                                        ConversaIrc::Privado(_) => source_nick.clone(),
                                    };

                                    tokio::spawn(async move {
                                        match cb(conversa, source_nick.clone(), source_nick, text, None).await {
                                            Ok(reply) => {
                                                // #2007: o mesmo saneamento do
                                                // `send_privmsg` — `str::lines()`
                                                // deixava passar `\r` isolado.
                                                if let Err(e) = IrcChannel::enviar_privmsg(&reply_writer, &reply_target, &reply).await {
                                                    error!("irc: failed to send reply: {e}");
                                                }
                                            }
                                            Err(e) if e == "__blocked__" => {}
                                            Err(e) => {
                                                error!("irc: callback error: {e}");
                                            }
                                        }
                                    });
                                }
                            }
                            Err(e) => {
                                error!("irc: read error: {e}");
                                return;
                            }
                        }
                    }
                    _ = shutdown_rx.changed() => {
                        if *shutdown_rx.borrow() {
                            info!("irc: shutdown requested");
                            let _ = IrcChannel::send_raw(&ping_writer, "QUIT :GarraIA shutting down").await;
                            return;
                        }
                    }
                }
            }
        });

        self.status = ChannelStatus::Connected;
        info!("irc channel connected");
        Ok(())
    }

    async fn disconnect(&mut self) -> Result<()> {
        if let Some(tx) = self.shutdown_tx.take() {
            let _ = tx.send(true);
        }

        // Clear the writer
        {
            let mut guard = self.writer.lock().await;
            *guard = None;
        }

        self.status = ChannelStatus::Disconnected;
        info!("irc channel disconnected");
        Ok(())
    }

    async fn send_message(&self, message: &Message) -> Result<()> {
        let target = message
            .metadata
            .get("irc_target")
            .and_then(|v| v.as_str())
            .ok_or_else(|| Error::Channel("missing irc_target in metadata".into()))?;

        let text = match &message.content {
            MessageContent::Text(t) => t.clone(),
            _ => {
                return Err(Error::Channel(
                    "only text messages are supported for irc send".into(),
                ));
            }
        };

        self.send_privmsg(target, &text).await
    }

    fn status(&self) -> ChannelStatus {
        self.status.clone()
    }
}

/// Parse a PRIVMSG line into (nick, target, text).
///
/// Format: `:nick!user@host PRIVMSG #channel :message text`
fn parse_privmsg(line: &str) -> Option<(String, String, String)> {
    if !line.starts_with(':') {
        return None;
    }

    let parts: Vec<&str> = line.splitn(4, ' ').collect();
    if parts.len() < 4 {
        return None;
    }

    // parts[0] = ":nick!user@host"
    // parts[1] = "PRIVMSG"
    // parts[2] = "#channel" or "nick"
    // parts[3] = ":message text"

    if parts[1] != "PRIVMSG" {
        return None;
    }

    // O prefixo pode ser `nick!user@host`, `nick@host` ou so `nick`: o nick
    // termina no primeiro `!` ou `@` (#2011 — senao `ana@h` viraria o nick).
    let nick = parts[0]
        .trim_start_matches(':')
        .split(['!', '@'])
        .next()?
        .to_string();

    let target = parts[2].to_string();
    let text = parts[3].trim_start_matches(':').to_string();

    Some((nick, target, text))
}

/// Prefixos de canal que este cliente aceita do `CHANTYPES`. Um alvo com
/// outro prefixo e conversa privada: assim nenhum canal comeca com `@`, que o
/// gateway usa para separar a sessao privada da sessao de canal.
const CHANTYPES_CONHECIDOS: [char; 4] = ['#', '&', '!', '+'];
/// `CHANTYPES` sem anuncio do servidor (o padrao do `RPL_ISUPPORT`).
const CHANTYPES_PADRAO: [char; 2] = ['#', '&'];

/// Como o servidor compara nicks (`CASEMAPPING`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Casemapping {
    /// So `A-Z` equivale a `a-z`.
    Ascii,
    /// `ascii` mais `[]\~` equivalendo a `{}|^`.
    Rfc1459,
    /// `ascii` mais `[]\` equivalendo a `{}|` (o `~` fica).
    Rfc1459Strict,
}

/// O que o servidor anunciou no `RPL_ISUPPORT` (005) e decide de quem e uma
/// mensagem recebida (#2011).
#[derive(Debug, Clone, PartialEq, Eq)]
struct Suporte {
    chantypes: Vec<char>,
    casemapping: Casemapping,
}

impl Default for Suporte {
    /// Sem anuncio, `CASEMAPPING` fica `ascii`, e nao o `rfc1459` do padrao do
    /// protocolo: `ascii` so junta o que todo servidor junta. Juntar a mais
    /// poria duas pessoas numa conversa; separar a mais so parte a conversa
    /// de uma pessoa em duas.
    fn default() -> Self {
        Self {
            chantypes: CHANTYPES_PADRAO.to_vec(),
            casemapping: Casemapping::Ascii,
        }
    }
}

impl Suporte {
    /// Le uma linha `:<servidor> 005 <nick> TOKEN[=valor] ... :<texto>`; as
    /// demais linhas passam sem efeito.
    fn aplicar_isupport(&mut self, linha: &str) {
        if !linha.starts_with(':') {
            return;
        }
        let mut partes = linha.split(' ').filter(|p| !p.is_empty());
        let (Some(_), Some("005"), Some(_)) = (partes.next(), partes.next(), partes.next()) else {
            return;
        };
        for token in partes {
            if token.starts_with(':') {
                break;
            }
            // `-TOKEN`: o servidor retirou o anuncio, volta ao padrao.
            if let Some(nome) = token.strip_prefix('-') {
                match nome {
                    "CHANTYPES" => self.chantypes = CHANTYPES_PADRAO.to_vec(),
                    "CASEMAPPING" => self.casemapping = Casemapping::Ascii,
                    _ => {}
                }
                continue;
            }
            let (nome, valor) = token.split_once('=').unwrap_or((token, ""));
            match nome {
                "CHANTYPES" => {
                    let conhecidos: Vec<char> = valor
                        .chars()
                        .filter(|c| CHANTYPES_CONHECIDOS.contains(c))
                        .collect();
                    // Anuncio sem nenhum prefixo conhecido (vazio, ou so
                    // `@`/lixo de um servidor degenerado) fica no padrao:
                    // lista vazia faria todo alvo, inclusive canal de
                    // verdade, virar privado (auditoria do #2011).
                    self.chantypes = if conhecidos.is_empty() {
                        CHANTYPES_PADRAO.to_vec()
                    } else {
                        conhecidos
                    };
                }
                "CASEMAPPING" => {
                    self.casemapping = match valor {
                        "rfc1459" => Casemapping::Rfc1459,
                        "rfc1459-strict" | "strict-rfc1459" => Casemapping::Rfc1459Strict,
                        // `ascii`, e o que nao conhecemos (`rfc7613`...):
                        // `ascii` e subconjunto de todos.
                        _ => Casemapping::Ascii,
                    };
                }
                _ => {}
            }
        }
    }

    /// O nick como o servidor o compara.
    fn normalizar_nick(&self, nick: &str) -> String {
        let rfc1459 = matches!(
            self.casemapping,
            Casemapping::Rfc1459 | Casemapping::Rfc1459Strict
        );
        nick.chars()
            .map(|c| match c {
                'A'..='Z' => c.to_ascii_lowercase(),
                '[' if rfc1459 => '{',
                ']' if rfc1459 => '}',
                '\\' if rfc1459 => '|',
                '~' if self.casemapping == Casemapping::Rfc1459 => '^',
                _ => c,
            })
            .collect()
    }

    /// De quem e a mensagem que `remetente` mandou para `alvo`: o canal, se o
    /// alvo tem prefixo de canal; senao, a conversa privada com o remetente.
    ///
    /// De proposito, alvo que nao e canal nem o nick do bot tambem e privado:
    /// `STATUSMSG` (`@#canal`, so para os ops do canal) e broadcast de
    /// servidor (`$*`). A mensagem vai para a sessao de quem mandou e a
    /// resposta, para o privado dele — nunca para o canal inteiro, que nao
    /// recebeu a mensagem original.
    fn conversa(&self, alvo: &str, remetente: &str) -> ConversaIrc {
        if alvo.starts_with(|c| self.chantypes.contains(&c)) {
            ConversaIrc::Canal(alvo.to_string())
        } else {
            ConversaIrc::Privado(self.normalizar_nick(remetente))
        }
    }
}

/// Teto do corpo de um `PRIVMSG`, em bytes. O corpo efetivo e menor quando o
/// alvo e longo: ver [`corpo_max`].
const MAX_CORPO_PRIVMSG: usize = 400;
/// Uma linha do protocolo IRC tem no maximo 512 bytes, `\r\n` incluso.
const MAX_LINHA: usize = 510;
/// Folga para o prefixo `:nick!user@host ` que o servidor acrescenta ao
/// repassar a mensagem a quem esta no canal: sem ela, quem recebe ve o fim
/// cortado.
const RESERVA_DO_PREFIXO: usize = 100;

/// O corpo que cabe num `PRIVMSG <alvo> :<corpo>` sem a linha passar de
/// [`MAX_LINHA`] menos [`RESERVA_DO_PREFIXO`] (achado da auditoria do #2007:
/// com o teto fixo de 400 e um alvo de 200 bytes, a linha tinha 610). Com o
/// alvo mais longo que [`alvo_valido`] aceita, sobram 200 bytes de corpo.
fn corpo_max(alvo: &str) -> usize {
    (MAX_LINHA - RESERVA_DO_PREFIXO)
        .saturating_sub("PRIVMSG ".len() + alvo.len() + " :".len())
        .min(MAX_CORPO_PRIVMSG)
}

/// Controles que sao FORMATACAO de texto do IRC (negrito, cor, cor hex,
/// reset, monoespacado, inverso, italico, riscado, sublinhado) e o tab: nao
/// sao protocolo, e o texto continua legivel com eles.
const FORMATACAO: [char; 10] = [
    '\x02', '\x03', '\x04', '\x0f', '\x11', '\x16', '\x1d', '\x1e', '\x1f', '\t',
];

/// O texto (vindo do modelo) em linhas seguras para `PRIVMSG <alvo> :<linha>`
/// (#2007).
///
/// - quebra em `\r\n`, `\n` **e** `\r` isolado — varios servidores tratam
///   qualquer um dos dois como fim de mensagem, e o `str::lines()` de antes
///   so separava em `\n`;
/// - tira os controles que nao sao formatacao: `\0` (o protocolo nao aceita)
///   e `\x01`, o delimitador de CTCP — um `\x01DCC SEND ...\x01` vindo do
///   modelo seria um pedido CTCP em nome do bot;
/// - descarta linha vazia;
/// - corta cada linha em `max_bytes` na fronteira de CARACTERE (o corte por
///   bytes de antes partia um caractere multibyte e o `from_utf8_lossy` o
///   trocava por `U+FFFD`).
fn linhas_do_privmsg(texto: &str, max_bytes: usize) -> Vec<String> {
    // Qualquer caractere UTF-8 cabe em 4 bytes: um teto menor nao avancaria.
    let max = max_bytes.max(4);
    texto
        .split(['\r', '\n'])
        .map(|linha| {
            linha
                .chars()
                .filter(|c| !c.is_control() || FORMATACAO.contains(c))
                .collect::<String>()
        })
        .filter(|linha| !linha.trim().is_empty())
        .flat_map(|linha| cortar_em_caractere(&linha, max))
        .collect()
}

/// `linha` em pedacos de no maximo `max` bytes, sem partir caractere.
fn cortar_em_caractere(linha: &str, max: usize) -> Vec<String> {
    let mut pedacos = Vec::new();
    let mut atual = String::new();
    for c in linha.chars() {
        if !atual.is_empty() && atual.len() + c.len_utf8() > max {
            pedacos.push(std::mem::take(&mut atual));
        }
        atual.push(c);
    }
    if !atual.is_empty() {
        pedacos.push(atual);
    }
    pedacos
}

/// Alvo aceitavel de um `PRIVMSG` (#2007): canal ou nick, um so. Espaco
/// encerraria o parametro e o resto viraria corpo; virgula vira MULTIPLOS
/// alvos; `:` no inicio vira o parametro final; controle (inclusive `\r`,
/// `\n`, `\0`) quebra a linha.
fn alvo_valido(alvo: &str) -> bool {
    !alvo.is_empty()
        && alvo.len() <= 200
        && !alvo.starts_with(':')
        && !alvo
            .chars()
            .any(|c| c.is_whitespace() || c.is_control() || c == ',')
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- saneamento (#2007) ------------------------------------------------

    fn sem_quebra_nem_nul(linhas: &[String]) -> bool {
        linhas
            .iter()
            .all(|l| !l.contains(['\r', '\n', '\0']) && !l.is_empty())
    }

    #[test]
    fn quebras_de_linha_viram_linhas_separadas() {
        let linhas = linhas_do_privmsg("a\nJOIN #x\r\nQUIT :tchau\rPART #y", MAX_CORPO_PRIVMSG);
        assert_eq!(linhas, ["a", "JOIN #x", "QUIT :tchau", "PART #y"]);
        assert!(sem_quebra_nem_nul(&linhas), "{linhas:?}");
    }

    #[test]
    fn linhas_vazias_somem_e_nul_e_ctcp_saem() {
        let linhas = linhas_do_privmsg("\n\n  \r\nok\0fim\n\x01DCC SEND x\x01", MAX_CORPO_PRIVMSG);
        assert_eq!(linhas, ["okfim", "DCC SEND x"]);
    }

    /// Formatacao de texto do IRC (negrito, cor, italico...) nao e protocolo:
    /// fica.
    #[test]
    fn codigos_de_formatacao_ficam() {
        let texto = "\x02negrito\x02 \x0304cor\x0f \x1ditalico\x1d";
        assert_eq!(linhas_do_privmsg(texto, MAX_CORPO_PRIVMSG), [texto]);
    }

    #[test]
    fn corte_por_tamanho_respeita_caractere() {
        let texto = "ç".repeat(300); // 600 bytes
        let linhas = linhas_do_privmsg(&texto, MAX_CORPO_PRIVMSG);
        assert!(linhas.len() >= 2, "{}", linhas.len());
        assert!(linhas.iter().all(|l| l.len() <= MAX_CORPO_PRIVMSG));
        assert!(!linhas.concat().contains('\u{FFFD}'));
        assert_eq!(linhas.concat(), texto, "nada se perde no corte");
    }

    #[test]
    fn alvo_com_espaco_virgula_ou_controle_e_recusado() {
        for bom in ["#garraia", "&local", "nick", "Nick_[1]"] {
            assert!(alvo_valido(bom), "{bom}");
        }
        for ruim in [
            "",
            "#a #b",
            "#a,#b",
            "#a\r\nQUIT",
            "nick\0",
            ":nick",
            "#a\x07",
        ] {
            assert!(!alvo_valido(ruim), "{ruim:?}");
        }
    }

    // --- protocolo: o que o servidor recebe (#2007) ------------------------

    use tokio::io::AsyncReadExt;
    use tokio::net::TcpListener;

    /// Tudo o que chegou ao "servidor" ate `fim`, separado como varios
    /// servidores IRC separam: em `\r` OU `\n`, nao so em `\r\n`.
    async fn linhas_recebidas(servidor: &mut TcpStream, fim: &str) -> Vec<String> {
        let mut recebido = Vec::new();
        let mut buf = [0u8; 4096];
        let prazo = tokio::time::Instant::now() + std::time::Duration::from_secs(5);
        while !String::from_utf8_lossy(&recebido).contains(fim) {
            let n = tokio::time::timeout_at(prazo, servidor.read(&mut buf))
                .await
                .expect("o bot parou de escrever antes do fim")
                .expect("leitura do socket");
            assert!(n > 0, "conexao fechada antes do fim");
            recebido.extend_from_slice(&buf[..n]);
        }
        String::from_utf8_lossy(&recebido)
            .split(['\r', '\n'])
            .filter(|l| !l.is_empty())
            .map(str::to_string)
            .collect()
    }

    fn canal_de_teste(porta: u16, resposta: &'static str) -> IrcChannel {
        let on_msg: IrcOnMessageFn = Arc::new(move |_ch, _nick, _user, _text, _delta_tx| {
            Box::pin(async move { Ok(resposta.to_string()) })
        });
        IrcChannel::new(
            IrcConfig {
                server: "127.0.0.1".into(),
                port: porta,
                nick: "garrabot".into(),
                channels: vec![],
                use_tls: false,
            },
            on_msg,
        )
    }

    const INJECAO: &str = "pronto\nJOIN #outro\r\nPRIVMSG NickServ :IDENTIFY x\rQUIT :tchau";

    /// `send_privmsg` (o caminho do `Channel::send_message`): nenhuma linha
    /// que chega ao servidor e outro comando.
    #[tokio::test]
    async fn send_privmsg_nao_injeta_comando() {
        let ouvinte = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let porta = ouvinte.local_addr().expect("addr").port();
        let mut canal = canal_de_teste(porta, "x");
        let conexao = tokio::spawn(async move { ouvinte.accept().await.map(|(s, _)| s) });
        canal.connect().await.expect("conecta");
        let mut servidor = conexao.await.expect("join").expect("accept");

        canal
            .send_privmsg("#canal", &format!("{INJECAO}\nFIM"))
            .await
            .expect("envia");

        let linhas = linhas_recebidas(&mut servidor, "FIM").await;
        let corpo: Vec<_> = linhas
            .iter()
            .filter(|l| !l.starts_with("NICK ") && !l.starts_with("USER "))
            .collect();
        assert!(!corpo.is_empty());
        for l in &corpo {
            assert!(l.starts_with("PRIVMSG #canal :"), "comando injetado: {l:?}");
        }
    }

    /// Achado da auditoria do #2007: um PING com `\r` ou `\0` no meio (servidor
    /// hostil, ou quem esta no meio da conexao, que e texto puro) ecoava no
    /// PONG, a barreira do `send_raw` recusava, e o laco de leitura morria sem
    /// reconexao. O PONG sai limpo e a conexao segue respondendo.
    #[tokio::test]
    async fn ping_com_controle_embutido_nao_derruba_a_conexao() {
        let ouvinte = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let porta = ouvinte.local_addr().expect("addr").port();
        let mut canal = canal_de_teste(porta, "FIM");
        let conexao = tokio::spawn(async move { ouvinte.accept().await.map(|(s, _)| s) });
        canal.connect().await.expect("conecta");
        let mut servidor = conexao.await.expect("join").expect("accept");

        servidor
            .write_all(b"PING :tok\rQUIT\r\nPING :a\0b\r\n:ana!a@h PRIVMSG #canal :oi\r\n")
            .await
            .expect("escreve");
        let linhas = linhas_recebidas(&mut servidor, "FIM").await;
        assert!(linhas.iter().any(|l| l == "PONG :tokQUIT"), "{linhas:?}");
        assert!(linhas.iter().any(|l| l == "PONG :ab"), "{linhas:?}");
        assert!(
            linhas.iter().any(|l| l == "PRIVMSG #canal :FIM"),
            "a conexao continuou viva: {linhas:?}"
        );
        assert!(linhas.iter().all(|l| l != "QUIT"), "{linhas:?}");
        canal.disconnect().await.expect("desconecta");
    }

    /// A linha inteira cabe no limite do protocolo com folga para o prefixo
    /// `:nick!user@host ` que o servidor acrescenta ao repassar — tambem com
    /// o alvo mais longo que [`alvo_valido`] aceita.
    #[tokio::test]
    async fn linha_cabe_no_limite_mesmo_com_alvo_longo() {
        let ouvinte = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let porta = ouvinte.local_addr().expect("addr").port();
        let mut canal = canal_de_teste(porta, "x");
        let conexao = tokio::spawn(async move { ouvinte.accept().await.map(|(s, _)| s) });
        canal.connect().await.expect("conecta");
        let mut servidor = conexao.await.expect("join").expect("accept");

        let alvo = format!("#{}", "a".repeat(199));
        assert!(alvo_valido(&alvo));
        canal
            .send_privmsg(&alvo, &format!("{}FIM", "ç".repeat(500)))
            .await
            .expect("envia");
        let linhas = linhas_recebidas(&mut servidor, "FIM").await;
        for l in linhas.iter().filter(|l| l.starts_with("PRIVMSG ")) {
            assert!(
                l.len() <= MAX_LINHA - RESERVA_DO_PREFIXO,
                "{} bytes",
                l.len()
            );
        }
    }

    /// A ultima linha de defesa: `send_raw` nunca escreve uma linha com
    /// terminador ou NUL embutido, venha o texto de onde vier.
    #[tokio::test]
    async fn send_raw_recusa_linha_com_terminador_embutido() {
        let ouvinte = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let porta = ouvinte.local_addr().expect("addr").port();
        let mut canal = canal_de_teste(porta, "x");
        let conexao = tokio::spawn(async move { ouvinte.accept().await.map(|(s, _)| s) });
        canal.connect().await.expect("conecta");
        let mut servidor = conexao.await.expect("join").expect("accept");

        for ruim in [
            "PRIVMSG #a :x\nQUIT",
            "PRIVMSG #a :x\rQUIT",
            "PRIVMSG #a :x\0",
        ] {
            assert!(
                IrcChannel::send_raw(&canal.writer, ruim).await.is_err(),
                "{ruim:?}"
            );
        }
        IrcChannel::send_raw(&canal.writer, "PRIVMSG #a :FIM")
            .await
            .expect("linha limpa");
        let linhas = linhas_recebidas(&mut servidor, "FIM").await;
        assert!(linhas.iter().all(|l| !l.starts_with("QUIT")), "{linhas:?}");
    }

    /// Alvo malformado nao chega ao socket — com a conexao de pe, para o erro
    /// nao vir so de "nao conectado".
    #[tokio::test]
    async fn send_privmsg_recusa_alvo_malformado() {
        let ouvinte = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let porta = ouvinte.local_addr().expect("addr").port();
        let mut canal = canal_de_teste(porta, "x");
        let conexao = tokio::spawn(async move { ouvinte.accept().await.map(|(s, _)| s) });
        canal.connect().await.expect("conecta");
        let mut servidor = conexao.await.expect("join").expect("accept");

        assert!(canal.send_privmsg("#a\r\nQUIT", "oi").await.is_err());
        assert!(canal.send_privmsg("#a,#b", "oi").await.is_err());
        assert!(canal.send_privmsg("#a #b", "oi").await.is_err());
        canal.send_privmsg("#ok", "FIM").await.expect("alvo valido");

        let linhas = linhas_recebidas(&mut servidor, "FIM").await;
        assert_eq!(
            linhas
                .iter()
                .filter(|l| !l.starts_with("NICK ") && !l.starts_with("USER "))
                .collect::<Vec<_>>(),
            ["PRIVMSG #ok :FIM"],
            "so o envio valido chegou"
        );
    }

    /// A resposta do turno (o laco de leitura do `connect`) passa pelo mesmo
    /// saneamento: inclusive o `\r` isolado, que o `str::lines()` deixava
    /// passar.
    #[tokio::test]
    async fn resposta_do_turno_nao_injeta_comando() {
        let ouvinte = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let porta = ouvinte.local_addr().expect("addr").port();
        let mut canal = canal_de_teste(
            porta,
            "pronto\rQUIT :tchau\nJOIN #outro\r\nPRIVMSG NickServ :x\nFIM",
        );
        let conexao = tokio::spawn(async move { ouvinte.accept().await.map(|(s, _)| s) });
        canal.connect().await.expect("conecta");
        let mut servidor = conexao.await.expect("join").expect("accept");

        servidor
            .write_all(b":ana!a@h PRIVMSG #canal :oi bot\r\n")
            .await
            .expect("escreve");
        let linhas = linhas_recebidas(&mut servidor, "FIM").await;
        let corpo: Vec<_> = linhas
            .iter()
            .filter(|l| !l.starts_with("NICK ") && !l.starts_with("USER "))
            .collect();
        assert!(!corpo.is_empty());
        for l in &corpo {
            assert!(l.starts_with("PRIVMSG #canal :"), "comando injetado: {l:?}");
        }
        canal.disconnect().await.expect("desconecta");
    }

    #[test]
    fn channel_type_is_irc() {
        let on_msg: IrcOnMessageFn = Arc::new(|_ch, _nick, _user, _text, _delta_tx| {
            Box::pin(async { Ok("test".to_string()) })
        });
        let config = IrcConfig {
            server: "irc.libera.chat".into(),
            port: 6667,
            nick: "garrabot".into(),
            channels: vec!["#garraia".into()],
            use_tls: false,
        };
        let channel = IrcChannel::new(config, on_msg);
        assert_eq!(channel.channel_type(), "irc");
        assert_eq!(channel.display_name(), "IRC");
        assert_eq!(channel.status(), ChannelStatus::Disconnected);
    }

    #[test]
    fn parse_privmsg_valid() {
        let line = ":user!name@host PRIVMSG #channel :hello world";
        let result = parse_privmsg(line);
        assert!(result.is_some());
        let (nick, target, text) = result.expect("should parse");
        assert_eq!(nick, "user");
        assert_eq!(target, "#channel");
        assert_eq!(text, "hello world");
    }

    #[test]
    fn parse_privmsg_private() {
        let line = ":user!name@host PRIVMSG botname :hello";
        let result = parse_privmsg(line);
        assert!(result.is_some());
        let (nick, target, text) = result.expect("should parse");
        assert_eq!(nick, "user");
        assert_eq!(target, "botname");
        assert_eq!(text, "hello");
    }

    #[test]
    fn parse_privmsg_not_privmsg() {
        let line = ":server NOTICE * :Welcome";
        assert!(parse_privmsg(line).is_none());
    }

    #[test]
    fn parse_privmsg_invalid() {
        let line = "not a valid irc line";
        assert!(parse_privmsg(line).is_none());
    }

    // --- de quem e a mensagem (#2011) ---------------------------------------

    /// O prefixo `nick@host` (sem `!user`) e valido no protocolo: o nick
    /// termina no `@`.
    #[test]
    fn parse_privmsg_tira_o_host_sem_user() {
        let (nick, _, _) = parse_privmsg(":ana@host PRIVMSG garrabot :oi").expect("parse");
        assert_eq!(nick, "ana");
        let (nick, _, _) = parse_privmsg(":ana PRIVMSG garrabot :oi").expect("parse");
        assert_eq!(nick, "ana");
    }

    #[test]
    fn isupport_le_chantypes_e_casemapping() {
        let mut s = Suporte::default();
        s.aplicar_isupport(
            ":srv 005 garrabot PREFIX=(ov)@+ CHANTYPES=#&! CASEMAPPING=rfc1459 :are supported",
        );
        assert_eq!(s.chantypes, ['#', '&', '!']);
        assert_eq!(s.casemapping, Casemapping::Rfc1459);

        s.aplicar_isupport(":srv 005 garrabot CASEMAPPING=strict-rfc1459 :are supported");
        assert_eq!(s.casemapping, Casemapping::Rfc1459Strict);
        s.aplicar_isupport(":srv 005 garrabot CASEMAPPING=rfc7613 :are supported");
        assert_eq!(s.casemapping, Casemapping::Ascii, "desconhecido vira ascii");

        s.aplicar_isupport(":srv 005 garrabot -CHANTYPES :are supported");
        assert_eq!(s.chantypes, CHANTYPES_PADRAO);
    }

    /// Prefixo de canal fora dos conhecidos e descartado: um canal nunca
    /// comeca com `@`, que o gateway usa no id da sessao privada.
    #[test]
    fn isupport_descarta_chantypes_desconhecido() {
        let mut s = Suporte::default();
        s.aplicar_isupport(":srv 005 garrabot CHANTYPES=#@a :are supported");
        assert_eq!(s.chantypes, ['#']);
        assert_eq!(s.conversa("@x", "ana"), ConversaIrc::Privado("ana".into()));

        // Sem nenhum prefixo conhecido, o padrao: canal continua canal.
        for degenerado in ["CHANTYPES=", "CHANTYPES", "CHANTYPES=@$"] {
            let mut s = Suporte::default();
            s.aplicar_isupport(&format!(":srv 005 garrabot {degenerado} :are supported"));
            assert_eq!(s.chantypes, CHANTYPES_PADRAO, "{degenerado}");
            assert_eq!(
                s.conversa("#garra", "ana"),
                ConversaIrc::Canal("#garra".into()),
                "{degenerado}"
            );
        }
    }

    /// `STATUSMSG` e broadcast nao sao canal nem o privado com o bot: ficam
    /// com quem mandou, e a resposta nao vai ao canal inteiro.
    #[test]
    fn statusmsg_e_broadcast_ficam_com_quem_mandou() {
        let s = Suporte::default();
        assert_eq!(
            s.conversa("@#garra", "Ana"),
            ConversaIrc::Privado("ana".into())
        );
        assert_eq!(s.conversa("$*", "Ana"), ConversaIrc::Privado("ana".into()));
    }

    /// Texto que imita o 005 dentro de outra linha nao muda nada.
    #[test]
    fn isupport_ignora_outras_linhas() {
        let mut s = Suporte::default();
        s.aplicar_isupport(":ana!a@h PRIVMSG garrabot :005 x CHANTYPES=+ CASEMAPPING=rfc1459");
        s.aplicar_isupport(":srv 001 garrabot :Welcome CHANTYPES=+");
        s.aplicar_isupport("005 garrabot CHANTYPES=+");
        assert_eq!(s, Suporte::default());
    }

    #[test]
    fn nick_normalizado_pelo_casemapping() {
        let mut s = Suporte::default();
        assert_eq!(s.normalizar_nick("Ana[x]\\~"), "ana[x]\\~", "ascii");
        s.casemapping = Casemapping::Rfc1459;
        assert_eq!(s.normalizar_nick("Ana[x]\\~"), "ana{x}|^");
        s.casemapping = Casemapping::Rfc1459Strict;
        assert_eq!(s.normalizar_nick("Ana[x]\\~"), "ana{x}|~");
    }

    #[test]
    fn conversa_separa_canal_de_privado() {
        let mut s = Suporte::default();
        assert_eq!(
            s.conversa("#Garra", "Ana"),
            ConversaIrc::Canal("#Garra".into()),
            "o nome do canal vem como o servidor mandou"
        );
        assert_eq!(
            s.conversa("&local", "ana"),
            ConversaIrc::Canal("&local".into())
        );
        // No privado o alvo e o nick do bot, o mesmo para todos: a conversa
        // e com quem mandou.
        assert_eq!(
            s.conversa("garrabot", "Ana"),
            ConversaIrc::Privado("ana".into())
        );
        assert_eq!(
            s.conversa("garrabot", "bruno"),
            ConversaIrc::Privado("bruno".into())
        );
        // Com `CHANTYPES=#` o `&` nao e canal.
        s.aplicar_isupport(":srv 005 garrabot CHANTYPES=# :are supported");
        assert_eq!(
            s.conversa("&local", "ana"),
            ConversaIrc::Privado("ana".into())
        );
    }

    /// O callback recebe a conversa, nao o alvo cru: no privado, quem mandou,
    /// e nunca o nick do bot.
    #[tokio::test]
    async fn callback_recebe_a_conversa_e_nao_o_alvo() {
        let ouvinte = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let porta = ouvinte.local_addr().expect("addr").port();
        let (tx, mut rx) = mpsc::unbounded_channel::<ConversaIrc>();
        let on_msg: IrcOnMessageFn = Arc::new(move |conversa, _nick, _user, _text, _delta_tx| {
            let _ = tx.send(conversa);
            Box::pin(async { Err("__blocked__".to_string()) })
        });
        let mut canal = IrcChannel::new(
            IrcConfig {
                server: "127.0.0.1".into(),
                port: porta,
                nick: "garrabot".into(),
                channels: vec![],
                use_tls: false,
            },
            on_msg,
        );
        let conexao = tokio::spawn(async move { ouvinte.accept().await.map(|(s, _)| s) });
        canal.connect().await.expect("conecta");
        let mut servidor = conexao.await.expect("join").expect("accept");

        servidor
            .write_all(
                b":srv 005 garrabot CASEMAPPING=rfc1459 :are supported\r\n\
                  :Ana!a@h PRIVMSG garrabot :oi\r\n\
                  :[bia]@h PRIVMSG garrabot :oi\r\n\
                  :ana!a@h PRIVMSG #garra :oi\r\n",
            )
            .await
            .expect("escreve");

        let mut recebidas = Vec::new();
        for _ in 0..3 {
            let c = tokio::time::timeout(std::time::Duration::from_secs(5), rx.recv())
                .await
                .expect("callback chamado")
                .expect("canal aberto");
            recebidas.push(c);
        }
        // Cada PRIVMSG vira uma tarefa: a ordem das chamadas nao e garantida.
        for esperada in [
            ConversaIrc::Privado("ana".into()),
            ConversaIrc::Privado("{bia}".into()),
            ConversaIrc::Canal("#garra".into()),
        ] {
            assert!(
                recebidas.contains(&esperada),
                "{esperada:?} em {recebidas:?}"
            );
        }
        canal.disconnect().await.expect("desconecta");
    }

    #[test]
    fn initial_status_is_disconnected() {
        let on_msg: IrcOnMessageFn = Arc::new(|_ch, _nick, _user, _text, _delta_tx| {
            Box::pin(async { Ok("test".to_string()) })
        });
        let config = IrcConfig {
            server: "irc.example.com".into(),
            port: 6667,
            nick: "garrabot".into(),
            channels: vec!["#test".into()],
            use_tls: false,
        };
        let channel = IrcChannel::new(config, on_msg);
        assert_eq!(channel.status(), ChannelStatus::Disconnected);
        assert_eq!(channel.channel_type(), "irc");
    }

    #[test]
    fn display_name_is_irc() {
        let on_msg: IrcOnMessageFn = Arc::new(|_ch, _nick, _user, _text, _delta_tx| {
            Box::pin(async { Ok("test".to_string()) })
        });
        let config = IrcConfig {
            server: "irc.example.com".into(),
            port: 6667,
            nick: "bot".into(),
            channels: vec![],
            use_tls: false,
        };
        let channel = IrcChannel::new(config, on_msg);
        assert_eq!(channel.display_name(), "IRC");
    }

    #[tokio::test]
    async fn send_message_without_connection_fails() {
        let on_msg: IrcOnMessageFn = Arc::new(|_ch, _nick, _user, _text, _delta_tx| {
            Box::pin(async { Ok("test".to_string()) })
        });
        let config = IrcConfig {
            server: "irc.example.com".into(),
            port: 6667,
            nick: "garrabot".into(),
            channels: vec!["#test".into()],
            use_tls: false,
        };
        let channel = IrcChannel::new(config, on_msg);
        let msg = Message::text(
            garraia_common::types::SessionId::from_string("test-session"),
            garraia_common::types::ChannelId::from_string("test-channel"),
            garraia_common::types::UserId::from_string("test-user"),
            garraia_common::MessageDirection::Outgoing,
            "hello",
        );
        let result = channel.send_message(&msg).await;
        assert!(result.is_err());
    }
}
