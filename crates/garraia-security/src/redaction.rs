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

/// Separadores aceitos **entre** grupos de digitos de um telefone formatado
/// (espaco, hifen, parenteses, underscore). Ponto de proposito fica de fora:
/// e o mesmo caractere do separador decimal de timestamp (`36.123456789Z`) e
/// de preco (`1.234,56`), e vira-lo ponte teria criado exatamente o falso
/// positivo que o #1514 pede para evitar.
fn e_separador_de_telefone(b: u8) -> bool {
    matches!(b, b' ' | b'-' | b'(' | b')' | b'_')
}

/// Quantidade maxima de caracteres de separador entre dois grupos de digitos
/// para ainda contar como a mesma ponte (`" ("`, `") "` tem 2; um hifen so, 1).
/// Alem disso e texto solto, nao formatacao de telefone.
const MAX_SEPARADORES_NA_PONTE: usize = 2;

/// Quantidade maxima de grupos extras que a ponte anexa ao grupo ancora.
/// Precisa cobrir o celular brasileiro com o nono digito separado
/// (`"+55 11 9 8765 4321"` = ancora + 4 grupos), a forma mais comum do
/// locale do projeto — um teto de 3 (achado da auditoria de seguranca do
/// #1514) deixava esse formato inteiro sem mascara, porque nem a metade
/// nem a ancora sozinhas batiam o limiar. O teto mais alto sozinho reabriria
/// a lista de numeros curtos (`"1 2 3 4 5 6 7 8 9 10"`); quem impede isso e
/// o discriminante em [`tem_grupo_de_telefone_plausivel`], nao este numero.
const MAX_GRUPOS_ADICIONAIS: usize = 4;

/// Ao menos um dos grupos mascarados tem 4+ digitos (prefixo ou sufixo de
/// telefone real, nunca so DDD/pais)? Uma lista solta de numeros curtos
/// (`"1 2 3 4 5 6 7 8 9 10"`, `"10 20 30 40 50"`) so tem grupos de 1-3
/// digitos e nunca passa aqui, mesmo somando 10+ no total.
fn tem_grupo_de_telefone_plausivel(grupos: &[usize]) -> bool {
    grupos.iter().any(|&g| g >= 4)
}

/// `grupos` e exatamente dois grupos do mesmo tamanho, grandes o bastante
/// para ser um intervalo numerico (`"100000-200000"`, contagem de linhas ou
/// bytes) em vez de telefone — um telefone real quase nunca tem os dois
/// blocos do mesmo tamanho quando ambos passam de 4 digitos.
fn parece_intervalo(grupos: &[usize]) -> bool {
    matches!(grupos, [a, b] if a == b && *a >= 5)
}

/// `grupos` (ja **completo** — so chamada depois que a ponte parou de
/// crescer) e um numero grande com separador de milhar (`"1 234 567 890"`,
/// `1.234.567,89` sem os pontos)? O primeiro grupo e o resto que sobra da
/// direita (1 a 3 digitos) e **todo** grupo depois dele e um bloco cheio de
/// exatamente 3 digitos. Precisa ser avaliada so no final: um celular NANP
/// com DDI separado (`"+1 555 123 4567"`) tem o **prefixo** `[1,3,3]` igual
/// ao de um numero de verdade com separador de milhar, e so o ultimo grupo
/// (`4567`, nao `3` digitos) desfaz a ambiguidade — checar a cada grupo
/// novo, em vez de no final, apagava todo celular NANP (achado da segunda
/// rodada da auditoria de seguranca do #1514).
fn parece_numero_com_milhar(grupos: &[usize]) -> bool {
    matches!(grupos, [primeiro, resto @ ..] if *primeiro <= 3
        && resto.len() >= 2
        && resto.iter().all(|&g| g == 3))
}

/// `grupos` fecha numa forma de data (`AAAA-MM-DD`, `DD-MM-AAAA` ou
/// `DD-MM-AA`, com qualquer separador de telefone entre os grupos — o
/// `CLAUDE.md` do projeto manda a primeira forma para toda data narrativa,
/// e a segunda e como um usuario BR escreve data em texto livre)? So
/// reconhecida com exatamente 3 grupos: com menos e inconclusivo, com mais a
/// ponte ja passou da data.
fn parece_data(grupos: &[usize]) -> bool {
    matches!(grupos, [4, 2, 2] | [2, 2, 4] | [2, 2, 2])
}

/// A ponte, gastando ate `grupos_restantes` grupos a mais a partir de `fim`,
/// alcanca algum grupo de 4+ digitos? Usada para decidir se um trecho que
/// **agora** parece uma data deve mesmo parar ali: espiar so o proximo
/// grupo nao bastava — um numero real com DDI separado
/// (`"55-11-98-76-5432"`) fecha a forma de data (`[2,2,2]`) com o proximo
/// grupo (`"76"`, 2 digitos) tambem curto, e so o **seguinte** (`"5432"`)
/// prova que era telefone (achado da segunda rodada da auditoria de
/// seguranca do #1514).
fn ponte_alcanca_grupo_grande(bytes: &[u8], fim: usize, grupos_restantes: usize) -> bool {
    let mut fim = fim;
    let mut restantes = grupos_restantes;
    while restantes > 0 {
        let Some((novo_fim, digitos)) = estender_sobre_ponte(bytes, fim) else {
            return false;
        };
        if digitos >= 4 {
            return true;
        }
        fim = novo_fim;
        restantes -= 1;
    }
    false
}

/// A partir de `fim` (logo apos um grupo de digitos ja aceito), tenta casar
/// uma ponte: separadores de telefone seguidos de outro grupo de digitos que
/// **nao** termine colado a letra. Devolve o novo fim e quantos digitos esse
/// grupo novo acrescenta; `None` quando nao ha ponte, ela e comprida demais,
/// ou o grupo encontrado seria ele mesmo um falso-positivo (`"...-9abc"`
/// nao pode fazer o `9` engolir um telefone valido que vem antes dele).
///
/// So faz o **calculo**, nao aplica nada — quem chama decide se aceita
/// (permite espiar antes de comprometer o guarda de data em
/// [`mascarar_numeros_longos`]).
fn estender_sobre_ponte(bytes: &[u8], fim: usize) -> Option<(usize, usize)> {
    let mut j = fim;
    while j < bytes.len() && e_separador_de_telefone(bytes[j]) {
        j += 1;
    }
    let tamanho_ponte = j - fim;
    if tamanho_ponte == 0 || tamanho_ponte > MAX_SEPARADORES_NA_PONTE {
        return None;
    }
    let grupo_inicio = j;
    while j < bytes.len() && bytes[j].is_ascii_digit() {
        j += 1;
    }
    if j == grupo_inicio || (j < bytes.len() && bytes[j].is_ascii_alphabetic()) {
        return None;
    }
    Some((j, j - grupo_inicio))
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
///
/// Desde o #1514, tambem soma digitos atraves de espaco/hifen/parenteses
/// (`"55 11 98765 4321"`, `"55-11-98765-4321"`, `"+55 (11) 98765-4321"`): a
/// contagem original so via sequencia contigua, e um telefone com separador
/// comum atravessava inteiro. O grupo ancora precisa passar no proprio
/// `colado_antes` (letra colada antes) e no proprio [`parte_de_uuid`]
/// **antes** de a ponte ser tentada — do contrario um id curto colado a letra
/// (`"req5-5511987654321"`) ou o ultimo grupo, so-digitos, de um UUID
/// (`"...-a716-446655440000 200 ok"`) arrastaria consigo, para a rejeicao (ou
/// para fora da protecao de UUID), um trecho que nao devia ser mascarado.
/// A ponte para de crescer assim que o total ja bate o limiar — sem isso, a
/// mascara "so os 4 ultimos" vira "os 4 ultimos **bytes**" quando um numero
/// curto e irrelevante gruda depois de um telefone que ja bastava sozinho
/// (achado da revisao de codigo do #1514).
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
        let fim_ancora = i;
        let mut fim = fim_ancora;
        let mut total_digitos = fim - inicio;
        let mut grupos = vec![total_digitos];
        let colado_antes = inicio > 0 && bytes[inicio - 1].is_ascii_alphabetic();
        let veio_de_uuid = parte_de_uuid(bytes, inicio, fim_ancora);
        if !colado_antes && !veio_de_uuid {
            while total_digitos < DIGITOS_DE_IDENTIFICADOR && grupos.len() <= MAX_GRUPOS_ADICIONAIS
            {
                if parece_data(&grupos) {
                    // So protege a data se a ponte, gastando o que resta do
                    // teto de grupos, nao alcanca um grupo de 4+ digitos —
                    // senao um numero real com essa forma casual
                    // (`"55-11-98-76-5432"`) escaparia inteiro so por parecer
                    // data no comeco. Olhar so o proximo grupo nao bastava: o
                    // grupo seguinte a "76" ainda e curto, e so o de depois
                    // (`"5432"`) desfaz a ambiguidade.
                    let restantes = MAX_GRUPOS_ADICIONAIS.saturating_sub(grupos.len() - 1);
                    if !ponte_alcanca_grupo_grande(bytes, fim, restantes) {
                        break;
                    }
                }
                let Some((novo_fim, digitos)) = estender_sobre_ponte(bytes, fim) else {
                    break;
                };
                fim = novo_fim;
                total_digitos += digitos;
                grupos.push(digitos);
            }
        }
        i = fim;
        let colado_depois = fim < bytes.len() && bytes[fim].is_ascii_alphabetic();
        if total_digitos < DIGITOS_DE_IDENTIFICADOR
            || colado_antes
            || colado_depois
            || veio_de_uuid
            || !tem_grupo_de_telefone_plausivel(&grupos)
            || parece_intervalo(&grupos)
            || parece_numero_com_milhar(&grupos)
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

    /// #1514: telefone com separador comum (espaco, hifen, parenteses)
    /// atravessava inteiro porque a contagem original so via sequencia
    /// contigua de digitos. Os tres casos sao os do relato original.
    #[test]
    fn mascara_telefone_com_separadores_comuns() {
        let casos = [
            ("ligue 55 11 98765 4321 agora", "ligue \u{2026}4321 agora"),
            ("ligue 55-11-98765-4321 agora", "ligue \u{2026}4321 agora"),
            (
                "ligue +55 (11) 98765-4321 agora",
                "ligue \u{2026}4321 agora",
            ),
        ];
        for (entrada, esperado) in casos {
            assert_eq!(mascarar_numeros_longos(entrada), esperado, "{entrada}");
        }
    }

    /// #1514, achado ALTO da auditoria de seguranca: celular brasileiro com o
    /// nono digito separado em grupo proprio e a forma mais comum do locale
    /// do projeto, e um teto de 3 grupos adicionais deixava passar inteiro
    /// (ancora+3 pontes parava em 9 digitos, abaixo do limiar).
    #[test]
    fn mascara_celular_br_com_nono_digito_separado() {
        let casos = [
            ("ligue +55 11 9 8765 4321 agora", "ligue \u{2026}4321 agora"),
            (
                "ligue +55 (11) 9 8765-4321 agora",
                "ligue \u{2026}4321 agora",
            ),
        ];
        for (entrada, esperado) in casos {
            assert_eq!(mascarar_numeros_longos(entrada), esperado, "{entrada}");
        }
    }

    /// #1514, achado da revisao de codigo: o ultimo grupo de um UUID, todo em
    /// digitos, e seguido de um numero curto e irrelevante — a ponte nao pode
    /// "roubar" a protecao de UUID so porque o `fim` foi estendido para alem
    /// dela antes de `parte_de_uuid` ser avaliada.
    #[test]
    fn uuid_seguido_de_numero_curto_continua_protegido() {
        assert_eq!(
            mascarar_numeros_longos("sessao 550e8400-e29b-41d4-a716-446655440000 200 ok"),
            "sessao 550e8400-e29b-41d4-a716-446655440000 200 ok"
        );
    }

    /// #1514, achado da revisao de codigo: quando o grupo ancora sozinho ja
    /// bate o limiar, a ponte nao pode tentar esticar mais — senao "os 4
    /// ultimos digitos" vira "os 4 ultimos bytes", que podem nem ser digito.
    #[test]
    fn telefone_que_ja_basta_sozinho_nao_engole_numero_seguinte() {
        assert_eq!(
            mascarar_numeros_longos("whatsapp-5511987654321 200"),
            "whatsapp-\u{2026}4321 200"
        );
        assert_eq!(
            mascarar_numeros_longos("id 1234567890 12"),
            "id \u{2026}7890 12"
        );
    }

    /// #1514, achado da auditoria de seguranca: um intervalo numerico
    /// (contagem de linha, faixa de byte) com dois blocos iguais e grandes
    /// nao e telefone.
    #[test]
    fn intervalo_numerico_nao_e_mascarado() {
        assert_eq!(
            mascarar_numeros_longos("linhas 100000-200000 lidas"),
            "linhas 100000-200000 lidas"
        );
    }

    /// #1514, achado da revisao de codigo: numero grande com separador de
    /// milhar (blocos de exatamente 3 digitos apos o primeiro) nao e
    /// telefone, mesmo somando 10+ digitos e tendo 4 grupos.
    #[test]
    fn numero_com_separador_de_milhar_nao_e_mascarado() {
        assert_eq!(
            mascarar_numeros_longos("total 1 234 567 890 reais"),
            "total 1 234 567 890 reais"
        );
    }

    /// #1514, achado da revisao de codigo: `parece_data` cobre `DD-MM-AAAA`
    /// e `DD-MM-AA` alem de `AAAA-MM-DD`, do jeito que um usuario BR escreve
    /// data em texto livre — nao so a convencao de data narrativa do
    /// `CLAUDE.md`.
    #[test]
    fn data_dd_mm_aaaa_seguida_de_numero_curto_nao_e_mascarada() {
        assert_eq!(
            mascarar_numeros_longos("reuniao em 22-09-2026 05 min"),
            "reuniao em 22-09-2026 05 min"
        );
    }

    /// #1514, achado MEDIO da auditoria de seguranca: o guarda de data so
    /// protege quando o que vem depois **nao** completaria um telefone
    /// plausivel. Um numero real que por acaso comeca com a forma de uma
    /// data (`"5511-98-76-5432"`, grupos 4-2-2 seguidos de um grupo de 4)
    /// nao pode escapar so por isso.
    #[test]
    fn numero_real_com_prefixo_de_data_nao_escapa_a_mascara() {
        assert_eq!(
            mascarar_numeros_longos("callback 5511-98-76-5432 recebido"),
            "callback \u{2026}5432 recebido"
        );
    }

    /// #1514, achado da segunda rodada da auditoria de seguranca: espiar so
    /// o proximo grupo nao bastava para desfazer a ambiguidade com data —
    /// com DDI separado, o grupo logo apos a forma de data ainda e curto
    /// (`"76"`), e so o **seguinte** (`"5432"`) prova que era telefone.
    #[test]
    fn numero_real_com_ddi_separado_e_prefixo_de_data_nao_escapa() {
        assert_eq!(
            mascarar_numeros_longos("callback 55-11-98-76-5432 recebido"),
            "callback \u{2026}5432 recebido"
        );
    }

    /// #1514, regressao achada na segunda rodada da auditoria de seguranca:
    /// `parece_numero_com_milhar` checado a cada grupo novo (em vez de so no
    /// final) apagava todo celular NANP com DDI separado, porque o prefixo
    /// `[1,3,3]` e igual ao de um numero de verdade com separador de milhar
    /// — so o ultimo grupo (`4567`, 4 digitos, nao 3) desfaz a ambiguidade.
    #[test]
    fn celular_nanp_com_ddi_separado_nao_e_confundido_com_numero_de_milhar() {
        let casos = [
            "ligue +1 555 123 4567 agora",
            "ligue +1 (555) 123-4567 agora",
            "ligue +1-555-123-4567 agora",
            "ligue 1 555 123 4567 agora",
        ];
        for entrada in casos {
            let saida = mascarar_numeros_longos(entrada);
            assert!(
                saida.ends_with("\u{2026}4567 agora"),
                "{entrada} -> {saida}"
            );
        }
    }

    /// Guarda contra falso positivo: um id curto colado a letra na frente de
    /// um telefone de verdade (`"req5-<telefone>"`) nao pode arrastar o
    /// telefone para a rejeicao do id. So o id fica de fora da mascara.
    #[test]
    fn id_colado_a_letra_na_frente_nao_esconde_o_telefone_depois() {
        assert_eq!(
            mascarar_numeros_longos("req5-5511987654321 chegou"),
            "req5-\u{2026}4321 chegou"
        );
    }

    /// Guarda simetrica: um digito solto colado a letra **depois** de um
    /// telefone de verdade (`"...-9abc"`) nao pode arrastar o telefone para
    /// a rejeicao por `colado_depois`.
    #[test]
    fn digito_colado_a_letra_atras_nao_esconde_o_telefone_antes() {
        assert_eq!(
            mascarar_numeros_longos("liga pra 5511987654321-9abc"),
            "liga pra \u{2026}4321-9abc"
        );
    }

    /// Guarda contra falso positivo: uma data narrativa `AAAA-MM-DD` (a forma
    /// que o `CLAUDE.md` manda usar) seguida de outro numero curto separado
    /// por espaco nao pode juntar digitos ate estourar o limiar. Sem o guarda
    /// de data, `2026-09-22 05` soma exatamente 10 digitos.
    #[test]
    fn data_narrativa_seguida_de_numero_curto_nao_e_mascarada() {
        assert_eq!(
            mascarar_numeros_longos("log em 2026-09-22 05 registros processados"),
            "log em 2026-09-22 05 registros processados"
        );
    }

    /// Guarda contra falso positivo: uma lista de numeros curtos separados
    /// por espaco (comum em texto solto) nao pode acumular digitos por conta
    /// do teto de grupos adicionais da ponte.
    #[test]
    fn lista_de_numeros_curtos_separados_por_espaco_nao_e_mascarada() {
        assert_eq!(
            mascarar_numeros_longos("portas 1 2 3 4 5 6 7 8 9 10 liberadas"),
            "portas 1 2 3 4 5 6 7 8 9 10 liberadas"
        );
    }

    /// Timestamp ISO 8601 com `T` (o formato que o `CLAUDE.md` manda para log
    /// e audit) nao pode virar telefone: o `T` colado a letra ja bloqueia a
    /// ponte antes de ela alcancar o limiar, sem precisar do guarda de data.
    #[test]
    fn timestamp_iso_com_t_nao_e_mascarado() {
        assert_eq!(
            mascarar_numeros_longos("em 2026-09-22T05:44:36.123456789Z ocorreu"),
            "em 2026-09-22T05:44:36.123456789Z ocorreu"
        );
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
