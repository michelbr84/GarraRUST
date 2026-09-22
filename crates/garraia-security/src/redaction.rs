use tracing_subscriber::fmt::MakeWriter;

/// A writer that redacts sensitive tokens (API keys, bot tokens) from log output.
pub struct RedactingWriter<W> {
    inner: W,
}

impl RedactingWriter<std::io::Stderr> {
    pub fn stderr() -> Self {
        Self {
            inner: std::io::stderr(),
        }
    }
}

impl<W: std::io::Write> std::io::Write for RedactingWriter<W> {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        let original = String::from_utf8_lossy(buf);
        let sem_segredo = redact_secrets(&original);
        let redacted = mascarar_numeros_longos(&sem_segredo);
        self.inner.write_all(redacted.as_bytes())?;
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.inner.flush()
    }
}

impl<'a> MakeWriter<'a> for RedactingWriter<std::io::Stderr> {
    type Writer = RedactingWriter<std::io::Stderr>;

    fn make_writer(&'a self) -> Self::Writer {
        RedactingWriter {
            inner: std::io::stderr(),
        }
    }
}

/// Adapta **qualquer** [`MakeWriter`] para aplicar [`redact_secrets`] na saída.
///
/// Existe porque [`RedactingWriter::stderr`] só cobria o stderr. O
/// `garraia-cli` compõe `file_appender.and(RedactingWriter::stderr())`, e a
/// metade do arquivo saía **crua** — ou seja, `~/.garraia/logs/garraia.log`
/// recebia em claro exatamente os segredos que o stderr redigia. Quem lê o log
/// de arquivo é justamente quem está depurando um incidente.
///
/// Envolva o appender: `RedactingMakeWriter::new(file_appender)`.
pub struct RedactingMakeWriter<M>(M);

impl<M> RedactingMakeWriter<M> {
    pub fn new(inner: M) -> Self {
        Self(inner)
    }
}

impl<'a, M> MakeWriter<'a> for RedactingMakeWriter<M>
where
    M: MakeWriter<'a>,
{
    type Writer = RedactingWriter<M::Writer>;

    fn make_writer(&'a self) -> Self::Writer {
        RedactingWriter {
            inner: self.0.make_writer(),
        }
    }
}

/// Replace known API key patterns with `[REDACTED]`.
///
/// A lista cresceu no #937, e vale registrar por que: ate ali o redactor so
/// via log, onde o que aparece sao as chaves que o *proprio* GarraIA usa
/// (Anthropic, OpenAI, Slack, Discord). Com os eventos de ferramenta, o mesmo
/// redactor passou a ver **comando que o agente monta** — e ali entra
/// credencial de terceiro que o usuario deu no contexto: PAT do GitHub, JWT,
/// chave da AWS, senha embutida em connection string. Sao esses os padroes
/// novos.
///
/// O que ele **nao** cobre, e continua sendo verdade: segredo sem formato
/// reconhecivel. `--password minhasenha` ou `-u admin:123456` nao tem prefixo
/// nem forma que os distinga de texto comum, e um regex que tentasse pegar
/// "o argumento depois de --password" erraria mais do que acertaria. Quem
/// olha a tela ve o comando como o agente o montou.
pub fn redact_secrets(input: &str) -> String {
    static PATTERNS: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(|| {
        regex::Regex::new(
            r"(?x)
            # --- credenciais do proprio GarraIA (vistas em log) ---
              sk-ant-api\S{10,}    # Anthropic API keys
            | sk-\S{20,}           # OpenAI-style keys
            | xoxb-\S{10,}         # Slack bot tokens
            | xapp-\S{10,}         # Slack app tokens
            | xoxp-\S{10,}         # Slack user tokens
            | Bot\s+[A-Za-z0-9_\-]{30,}  # Discord bot tokens

            # --- credenciais de terceiro que entram por comando de ferramenta (#937) ---
            | github_pat_[A-Za-z0-9_]{22,}          # GitHub fine-grained PAT
            | gh[pousr]_[A-Za-z0-9.\-_]{20,}        # GitHub PAT/OAuth/user/server/refresh (ghs_ stateless incluso)
            | eyJ[A-Za-z0-9_\-]{10,}\.[A-Za-z0-9_\-]{10,}\.[A-Za-z0-9_\-]{10,}  # JWT
            | (?:AKIA|ASIA)[A-Z0-9]{16}             # AWS access key id (fixa e temporaria)
            | [0-9]{8,10}:AA[A-Za-z0-9_\-]{32,}    # Telegram bot token
            | ://[^:@/\s]+:[^@/\s]{4,}@           # senha embutida em connection string
            ",
        )
        .expect("redaction regex should compile")
    });

    PATTERNS.replace_all(input, "[REDACTED]").into_owned()
}

/// Menor sequencia de digitos que o log trata como identificador de pessoa.
///
/// Telefone E.164 tem de 8 a 15 digitos, mas os de 8-9 se confundem com
/// contagem, porta e tamanho de arquivo; a partir de 10 ja sao celular com DDD
/// (Brasil), numero com codigo do pais, LID do WhatsApp, id de chat do
/// Telegram ou snowflake do Discord — todos identificam uma pessoa.
const DIGITOS_DE_IDENTIFICADOR: usize = 10;

/// A sequencia `inicio..fim` e um grupo de um UUID (`8-4-4-4-12` hexa)? O id
/// de sessao do Telegram e da web e UUID, e o ultimo grupo sai so com digitos
/// de vez em quando; mascara-lo esconderia a sessao sem proteger ninguem.
fn parte_de_uuid(bytes: &[u8], inicio: usize, fim: usize) -> bool {
    let do_token = |b: u8| b.is_ascii_hexdigit() || b == b'-';
    let mut a = inicio;
    while a > 0 && do_token(bytes[a - 1]) {
        a -= 1;
    }
    let mut z = fim;
    while z < bytes.len() && do_token(bytes[z]) {
        z += 1;
    }
    // O token pode trazer hexa colado antes (`linked-` termina em `ed-`):
    // procura um UUID de 36 bytes que contenha a sequencia.
    (a..=inicio).any(|ini| {
        let fim_uuid = ini + 36;
        fim_uuid <= z
            && fim_uuid >= fim
            && bytes[ini..fim_uuid]
                .iter()
                .enumerate()
                .all(|(k, b)| match k {
                    8 | 13 | 18 | 23 => *b == b'-',
                    _ => b.is_ascii_hexdigit(),
                })
    })
}

/// Mascara, no texto que vai para o log, toda sequencia de pelo menos
/// [`DIGITOS_DE_IDENTIFICADOR`] digitos que nao esteja colada a letra ou a
/// outro digito, deixando so os 4 ultimos (`…4321`).
///
/// Existe porque o id de sessao carrega o remetente em varios canais —
/// `whatsapp-linked-<jid>`, `whatsapp-<telefone>`, `signal-<telefone>` — e os
/// spans do `AgentRuntime` gravam `session_id` em todo evento do turno. O canal
/// `whatsapp_linked` ja so loga `phone_last4` por conta propria, mas o span do
/// runtime passava o numero inteiro por baixo (achado da revisao da #1343).
/// Mascarar aqui, no writer, cobre todo call site de uma vez, inclusive os
/// que ainda nao existem.
///
/// "Colada a letra" fica de fora de proposito: um hash hexadecimal
/// (`a3f1234567890b`) nao e telefone, e mascarar pedaco dele so atrapalharia
/// quem depura. Um `+` antes do numero entra na mascara. Nao e usado no
/// `redact_secrets`: la o texto e resultado de ferramenta, onde numero longo e
/// conteudo legitimo.
pub fn mascarar_numeros_longos(input: &str) -> std::borrow::Cow<'_, str> {
    let bytes = input.as_bytes();
    let mut saida: Option<String> = None;
    let mut copiado = 0;
    let mut i = 0;
    while i < bytes.len() {
        if !bytes[i].is_ascii_digit() {
            i += 1;
            continue;
        }
        let inicio = i;
        while i < bytes.len() && bytes[i].is_ascii_digit() {
            i += 1;
        }
        let fim = i;
        let colado_antes = inicio > 0 && bytes[inicio - 1].is_ascii_alphabetic();
        let colado_depois = fim < bytes.len() && bytes[fim].is_ascii_alphabetic();
        if fim - inicio < DIGITOS_DE_IDENTIFICADOR
            || colado_antes
            || colado_depois
            || parte_de_uuid(bytes, inicio, fim)
        {
            continue;
        }
        let corte = if inicio > 0 && bytes[inicio - 1] == b'+' {
            inicio - 1
        } else {
            inicio
        };
        let texto = saida.get_or_insert_with(|| String::with_capacity(input.len()));
        // `corte`, `inicio` e `fim` caem em fronteira de byte ASCII, entao
        // sao fronteira de char: o fatiamento nao pode entrar num UTF-8.
        texto.push_str(&input[copiado..corte]);
        texto.push('\u{2026}');
        texto.push_str(&input[fim - 4..fim]);
        copiado = fim;
    }
    match saida {
        None => std::borrow::Cow::Borrowed(input),
        Some(mut texto) => {
            texto.push_str(&input[copiado..]);
            std::borrow::Cow::Owned(texto)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mascara_telefone_jid_e_lid_e_deixa_os_4_ultimos() {
        let casos = [
            (
                "session_id=whatsapp-linked-5511987654321@s.whatsapp.net",
                "session_id=whatsapp-linked-\u{2026}4321@s.whatsapp.net",
            ),
            (
                "remetente +5511987654321 pediu",
                "remetente \u{2026}4321 pediu",
            ),
            ("lid 123456789012345@lid", "lid \u{2026}2345@lid"),
            (
                "5511999990000:1@s.whatsapp.net",
                "\u{2026}0000:1@s.whatsapp.net",
            ),
            ("signal-+5511912345678 e line", "signal-\u{2026}5678 e line"),
            (
                "a 5511987654321 e b 5511912345678",
                "a \u{2026}4321 e b \u{2026}5678",
            ),
        ];
        for (entrada, esperado) in casos {
            assert_eq!(mascarar_numeros_longos(entrada), esperado, "{entrada}");
        }
    }

    #[test]
    fn numero_curto_hash_e_texto_comum_passam_intactos() {
        for intacto in [
            "porta 3888, 200 OK em 123456789 ns",
            "sha a3f1234567890b e 1234567890abcdef",
            "2026-09-22T05:44:36.123456789Z",
            "uuid 550e8400-e29b-41d4-a716-446655440000",
            "sem numero nenhum",
            "acentuação e emoji 🦀 sem digito",
        ] {
            assert!(
                matches!(
                    mascarar_numeros_longos(intacto),
                    std::borrow::Cow::Borrowed(_)
                ),
                "nao devia mexer em: {intacto}"
            );
        }
    }

    #[test]
    fn writer_mascara_o_numero_e_redige_o_segredo() {
        use std::io::Write;
        let mut w = RedactingWriter { inner: Vec::new() };
        w.write_all(
            b"INFO process_message{session_id=whatsapp-5511987654321}: chave sk-abcdefghijklmnopqrstuvwxyz\n",
        )
        .expect("write");
        let saida = String::from_utf8(w.inner).expect("utf8");
        assert!(!saida.contains("5511987654321"), "{saida}");
        assert!(saida.contains("whatsapp-\u{2026}4321"), "{saida}");
        assert!(saida.contains("[REDACTED]"), "{saida}");
    }

    /// #937: o redactor passou a ver comando de ferramenta, entao passou a
    /// precisar dos segredos de terceiro que aparecem ali.
    #[test]
    fn redacts_github_tokens() {
        let classico = format!("ghp_{}", "a".repeat(36));
        let fine = format!("github_pat_{}", "b".repeat(82));
        for token in [&classico, &fine] {
            let saida = redact_secrets(&format!("curl -H 'Authorization: Bearer {token}'"));
            assert!(!saida.contains(token.as_str()), "vazou: {saida}");
            assert!(saida.contains("[REDACTED]"), "{saida}");
        }
    }

    /// Formato stateless dos installation tokens do GitHub (`ghs_`, ~520
    /// chars): o corpo tem pontos. A classe do padrao precisa engolir o token
    /// inteiro — se ela nao cobre `.`/`-`/`_`, o redactor para no primeiro
    /// ponto e o restante segue cru para o log.
    #[test]
    fn redacts_stateless_installation_token() {
        let token = format!(
            "ghs_{}.{}.{}",
            "a".repeat(20),
            "b".repeat(20),
            "c".repeat(20)
        );
        let saida = redact_secrets(&format!("curl -H 'Authorization: Bearer {token}'"));
        // assert_eq de proposito: se so o pedaco antes do primeiro ponto for
        // redigido, o `.bbb...ccc...` restante aparece na saida e o teste falha.
        assert_eq!(saida, "curl -H 'Authorization: Bearer [REDACTED]'");
    }

    #[test]
    fn redacts_jwt() {
        // Montado em pedacos de proposito. Um JWT literal aqui e um achado
        // legitimo do `gitleaks`, e foi o que ele pegou na primeira versao
        // deste teste. As duas saidas eram alargar a allowlist do
        // `.gitleaks.toml` ou nao escrever o literal — e trocar a forca de um
        // gate de seguranca pela legibilidade de um vetor de teste e o lado
        // errado da troca. **Nao junte estas partes numa string so.**
        let jwt = format!(
            "{}.{}.{}",
            "eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9",
            "eyJzdWIiOiIxMjM0NTY3ODkwIn0",
            "dBjftJeZ4CVPmB92K27uhbUJU1p1r_wW1gFWFOEjXk"
        );
        let saida = redact_secrets(&format!("Authorization: Bearer {jwt}"));
        assert!(!saida.contains(&jwt), "vazou: {saida}");
    }

    #[test]
    fn redacts_aws_access_key_id() {
        for prefixo in ["AKIA", "ASIA"] {
            let chave = format!("{prefixo}IOSFODNN7EXAMPLE");
            let saida = redact_secrets(&format!("aws --access-key {chave} s3 ls"));
            assert!(!saida.contains(&chave), "vazou: {saida}");
        }
    }

    #[test]
    fn redacts_telegram_bot_token() {
        let token = format!("7654321098:AA{}", "F".repeat(33));
        let saida = redact_secrets(&format!("https://api.telegram.org/bot{token}/sendMessage"));
        assert!(!saida.contains(&token), "vazou: {saida}");
    }

    #[test]
    fn redacts_password_in_connection_string() {
        let saida = redact_secrets("psql postgres://admin:s3cr3tpass@db.prod.example.com/mydb");
        assert!(!saida.contains("s3cr3tpass"), "vazou: {saida}");
        // O host sobrevive: e diagnostico util e nao e o segredo.
        assert!(saida.contains("db.prod.example.com"), "{saida}");
    }

    /// Uma URL comum nao pode ser redigida — o redactor roda em todo log.
    #[test]
    fn deixa_url_sem_credencial_em_paz() {
        for url in [
            "https://api.github.com/repos/x/y",
            "https://garraia.org/install.sh",
            "http://127.0.0.1:11434/api/embeddings",
            "git@github.com:michelbr84/GarraRUST.git",
        ] {
            assert_eq!(redact_secrets(url), url, "redigiu URL inocente: {url}");
        }
    }

    /// Texto comum tambem nao pode virar `[REDACTED]`.
    #[test]
    fn deixa_texto_comum_em_paz() {
        for texto in [
            "cargo test --workspace",
            "crates/garraia-cli/src/chat.rs",
            "148 passed em 6.3s",
            "erro: exit 101",
        ] {
            assert_eq!(redact_secrets(texto), texto, "redigiu texto comum: {texto}");
        }
    }

    #[test]
    fn redacts_anthropic_key() {
        let input = "key=sk-ant-api03-abcdefghij";
        assert_eq!(redact_secrets(input), "key=[REDACTED]");
    }

    #[test]
    fn redacts_openai_key() {
        let input = "key=sk-1234567890123456789012345";
        assert_eq!(redact_secrets(input), "key=[REDACTED]");
    }

    #[test]
    fn redacts_slack_bot_token() {
        let input = "token=xoxb-1234567890-abc";
        assert_eq!(redact_secrets(input), "token=[REDACTED]");
    }

    #[test]
    fn leaves_normal_text_unchanged() {
        let input = "hello world";
        assert_eq!(redact_secrets(input), "hello world");
    }

    /// Guard do bug corrigido em 2026-08-29: `RedactingMakeWriter` precisa
    /// redigir a saída de um `MakeWriter` arbitrário, não só do stderr. Antes
    /// disso o `garraia-cli` compunha `file_appender.and(RedactingWriter::
    /// stderr())` e a metade do arquivo saía crua.
    #[test]
    fn make_writer_adapter_redacts_an_arbitrary_sink() {
        use std::io::Write;
        use std::sync::{Arc, Mutex};

        /// Sink em memória que grava tudo que recebe, para inspeção.
        #[derive(Clone, Default)]
        struct Buf(Arc<Mutex<Vec<u8>>>);

        impl Write for Buf {
            fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
                self.0.lock().expect("buf lock").extend_from_slice(buf);
                Ok(buf.len())
            }
            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }

        impl<'a> MakeWriter<'a> for Buf {
            type Writer = Buf;
            fn make_writer(&'a self) -> Self::Writer {
                self.clone()
            }
        }

        let sink = Buf::default();
        let adapter = RedactingMakeWriter::new(sink.clone());

        let mut w = adapter.make_writer();
        w.write_all(b"authorization: sk-ant-api03-deadbeefcafe1234\n")
            .expect("write");

        let written = String::from_utf8(sink.0.lock().expect("buf lock").clone()).expect("utf8");
        assert!(
            !written.contains("sk-ant-api03-deadbeefcafe1234"),
            "segredo chegou cru ao sink: {written:?}"
        );
        assert!(
            written.contains("[REDACTED]"),
            "esperava marcador: {written:?}"
        );
    }
}
