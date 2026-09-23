//! Quem pode falar com o GarraIA pelo WhatsApp pessoal (#1345).
//!
//! # O defeito que este modulo fecha
//!
//! O `link` gravava a sessao, ligava o canal e dizia "pronto para receber
//! mensagens" — com `channels.whatsapp_linked.allow` e `owners` vazios. O
//! portao do gateway e fail-closed (lista vazia = ninguem), e esta certo; o
//! que estava errado era a CLI: ninguem recebia resposta, toda mensagem era
//! descartada em silencio, e o unico jeito de autorizar alguem era editar o
//! `config.yml` a mao.
//!
//! # O que continua como estava, de proposito
//!
//! - **Recusa silenciosa** de quem nao esta autorizado: responder confirmaria
//!   ao estranho que o numero roda um bot.
//! - **Nada de auto-claim.** O primeiro remetente nunca vira dono, e o numero
//!   vinculado nao e autorizado sozinho.
//! - **Dono so em `isolated-pod`**, e so com confirmacao explicita (default
//!   nao): um dono em `standard` seria um privilegio latente, que acordaria
//!   em silencio no dia em que a env do perfil mudasse.
//!
//! # Numero com `+` e codigo do pais — ou um LID
//!
//! Codigo do pais **obrigatorio** e nunca adivinhado, e por isso o `+`
//! inicial tambem: sem ele `11 99999-8888` (DDD + numero, como se escreve no
//! Brasil) passava por numero de 11 digitos, era gravado e nunca casava com o
//! remetente, que chega sempre com o codigo do pais. Espacos, hifens, pontos
//! e parenteses sao descartados; o resto tem de ser digito. Zero inicial e
//! recusado (e o prefixo de discagem local) e o total tem de caber no E.164
//! como a ponte o aceita (6 a 15 digitos — Niue e Tokelau tem 7). A forma
//! gravada e a que o portao do gateway compara
//! (`whatsapp_linked_normalizar_identidade`), e na tela so aparecem os quatro
//! ultimos digitos.
//!
//! O WhatsApp as vezes identifica o contato so por um LID (`<id>@lid`), sem
//! numero; a ponte entrega o numero quando o servidor o manda junto, e quando
//! nao manda o portao compara o LID. Por isso `<digitos>@lid` tambem e aceito,
//! gravado como veio.

use anyhow::{Result, bail};
use garraia_config::{AppConfig, ChannelConfig, ConfigLoader};

use super::{CONFIG_KEY, Context, EX_CANCELLED, EX_SOFTWARE, Lang, t, tb};
use crate::wizard::prompts::Prompter;

/// `EX_USAGE`: a combinacao de flags nao vale aqui (`--owner` em `standard`,
/// `--owner` num pipe sem `--yes`).
pub const EX_USAGE: i32 = 64;
/// `EX_DATAERR`: o numero nao e um numero com codigo do pais.
pub const EX_DATAERR: i32 = 65;

/// Quantas vezes o `link` pergunta o numero antes de desistir.
const TENTATIVAS: usize = 3;

// ---------------------------------------------------------------------------
// Numero
// ---------------------------------------------------------------------------

/// Por que um texto nao e um numero autorizavel.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NumeroInvalido {
    Vazio,
    /// Veio na forma `…@s.whatsapp.net` (ou qualquer JID que nao `<id>@lid`).
    Jid,
    /// Numero sem o `+` do codigo do pais.
    SemMais,
    /// Letra ou outro caractere fora de `+ -().` e espaco.
    Caractere,
    /// `*` (ou `+*`, `**`…): a tentativa de abrir o canal para todo mundo
    /// (#1389). Nao existe hoje semantica de curinga — e o erro generico de
    /// caractere fazia parecer erro de digitacao, e nao recurso inexistente.
    Curinga,
    /// Comeca com `0`: prefixo de discagem local, nao codigo de pais.
    ZeroInicial,
    /// Fora de 6 a 15 digitos.
    Tamanho(usize),
}

impl NumeroInvalido {
    /// A frase que o usuario le. Nunca repete o que ele digitou.
    pub fn mensagem(&self, lang: Lang) -> String {
        match self {
            Self::Vazio => t(lang, "O número está vazio.", "The number is empty.").to_string(),
            Self::Jid => t(
                lang,
                "Use o número com + e código do país, sem `@s.whatsapp.net` (só um LID, `<id>@lid`, vai com o @).",
                "Use the number with + and the country code, without `@s.whatsapp.net` (only a LID, `<id>@lid`, keeps the @).",
            )
            .to_string(),
            Self::SemMais => t(
                lang,
                "Comece com + e o código do país (ex.: +55 11 99999-8888): sem ele o número nunca casa com quem manda a mensagem.",
                "Start with + and the country code (e.g. +1 555 123 4567): without it the number never matches the sender.",
            )
            .to_string(),
            Self::Caractere => t(
                lang,
                "O número só pode ter dígitos (e, opcionalmente, +, espaços, hífens, pontos e parênteses).",
                "The number may only contain digits (and, optionally, +, spaces, dashes, dots and parentheses).",
            )
            .to_string(),
            Self::Curinga => t(
                lang,
                "`*` não é um número: este canal não tem \"autorizar todo mundo\". O portão é fail-closed e cada identidade entra uma a uma, com + e código do país (ex.: +55 11 99999-8888) — ou como LID (`<id>@lid`).",
                "`*` is not a number: this channel has no \"authorize everyone\". The gate is fail-closed and each identity is listed one by one, with + and the country code (e.g. +1 555 123 4567) — or as a LID (`<id>@lid`).",
            )
            .to_string(),
            Self::ZeroInicial => t(
                lang,
                "O número começa com 0: use o código do país no lugar do prefixo local (ex.: +55 11 99999-8888).",
                "The number starts with 0: use the country code instead of the local prefix (e.g. +55 11 99999-8888).",
            )
            .to_string(),
            Self::Tamanho(n) => match lang {
                Lang::Pt => format!(
                    "O número tem {n} dígitos; com o código do país ele precisa ter de 6 a 15 (ex.: +55 11 99999-8888)."
                ),
                Lang::En => format!(
                    "The number has {n} digits; with the country code it must have 6 to 15 (e.g. +55 11 99999-8888)."
                ),
            },
        }
    }
}

/// Separadores que o numero pode trazer e que sao descartados.
fn separador(c: char) -> bool {
    matches!(c, ' ' | '-' | '.' | '(' | ')')
}

/// `*`, `**`, `+*`: a tentativa de dizer "todo mundo" (#1389).
///
/// So existe para o erro poder ser especifico. **Nao** e um passo em direcao
/// ao curinga: a semantica de acesso aberto nao existe (depende da #1388), e
/// enquanto nao existir a resposta certa e recusar dizendo o porque — e nao
/// o `Caractere` generico, que faz um recurso inexistente parecer erro de
/// digitacao.
fn e_curinga(s: &str) -> bool {
    let corpo = s.strip_prefix('+').unwrap_or(s);
    !corpo.is_empty() && corpo.chars().all(|c| c == '*')
}

/// `<digitos>@lid`, exatamente: o LID que a ponte entrega quando o servidor
/// nao manda o numero junto.
fn e_lid_valido(s: &str) -> bool {
    s.strip_suffix("@lid")
        .is_some_and(|id| (6..=20).contains(&id.len()) && id.bytes().all(|b| b.is_ascii_digit()))
}

/// Normaliza um numero digitado para a forma que o portao do canal compara.
///
/// Pura. `Ok` e so digitos, com codigo do pais, 6 a 15 de comprimento — ou
/// um `<digitos>@lid` como veio — e e byte a byte o que
/// `whatsapp_linked_normalizar_identidade` do gateway devolve para a mesma
/// entrada (um teste prende isso).
pub fn normalizar_numero(raw: &str) -> Result<String, NumeroInvalido> {
    let s = raw.trim();
    if s.is_empty() {
        return Err(NumeroInvalido::Vazio);
    }
    if e_curinga(s) {
        return Err(NumeroInvalido::Curinga);
    }
    if e_lid_valido(s) {
        return Ok(garraia_gateway::bootstrap::whatsapp_linked_normalizar_identidade(s));
    }
    if s.contains('@') {
        return Err(NumeroInvalido::Jid);
    }
    let Some(corpo) = s.strip_prefix('+') else {
        return Err(if s.chars().all(|c| c.is_ascii_digit() || separador(c)) {
            NumeroInvalido::SemMais
        } else {
            NumeroInvalido::Caractere
        });
    };
    let mut digitos = String::with_capacity(corpo.len());
    for c in corpo.chars() {
        if c.is_ascii_digit() {
            digitos.push(c);
        } else if !separador(c) {
            return Err(NumeroInvalido::Caractere);
        }
    }
    if digitos.is_empty() {
        return Err(NumeroInvalido::Vazio);
    }
    if digitos.starts_with('0') {
        return Err(NumeroInvalido::ZeroInicial);
    }
    if !(6..=15).contains(&digitos.len()) {
        return Err(NumeroInvalido::Tamanho(digitos.len()));
    }
    Ok(garraia_gateway::bootstrap::whatsapp_linked_normalizar_identidade(&digitos))
}

/// Os quatro ultimos digitos — a unica parte de um numero (ou de um LID,
/// antes do `@lid`) que a CLI imprime.
pub fn final4(numero: &str) -> &str {
    let numero = numero.split_once('@').map_or(numero, |(id, _)| id);
    let n = numero.len();
    // So digitos ASCII chegam aqui (`normalizar_numero`), entao cortar por
    // byte nao parte caractere; o `get` e o seguro contra o dia em que isso
    // mudar.
    numero.get(n.saturating_sub(4)..).unwrap_or("")
}

/// E um LID (`<id>@lid`), e nao um numero?
fn e_lid(identidade: &str) -> bool {
    identidade.ends_with("@lid")
}

// ---------------------------------------------------------------------------
// Estado de acesso, lido da config
// ---------------------------------------------------------------------------

/// O que a config diz sobre quem entra. Contagens, nunca identidades.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Acesso {
    pub enabled: bool,
    /// `allow` uniao `owners`, sem repeticao — o mesmo numero que o portao
    /// do gateway admite.
    pub autorizados: usize,
    pub donos: usize,
}

/// Le o acesso pelo MESMO leitor que o gateway usa no turno.
pub fn acesso_da_config(config: &AppConfig) -> Acesso {
    let s = garraia_gateway::bootstrap::whatsapp_linked_settings(config);
    Acesso {
        enabled: s.enabled,
        autorizados: s.autorizados(),
        donos: s.donos(),
    }
}

/// Onde o numero vai.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Papel {
    /// `channels.whatsapp_linked.allow`.
    Autorizado,
    /// `channels.whatsapp_linked.owners` — e so la (ADR 0024).
    Dono,
}

impl Papel {
    fn chave(self) -> &'static str {
        match self {
            Self::Autorizado => "allow",
            Self::Dono => "owners",
        }
    }

    /// Como o `--json` do `users` nomeia o papel. Contrato de script: o nome
    /// e o da chave da config (`allow`/`owners`), e nao a palavra traduzida
    /// que aparece na tela.
    fn json(self) -> &'static str {
        self.chave()
    }
}

/// A chave que o portao do gateway compara — a MESMA para autorizar, listar e
/// remover.
///
/// Um numero escrito `+55 31 99999-8888` e o mesmo `553199998888` (o nono
/// digito, ver `whatsapp_linked_chave_do_portao`). Tres copias desta regra
/// divergiriam no dia em que o gateway mudasse a sua, e ai `remove` nao
/// acharia o que `allow` gravou.
fn chave_do_portao(identidade: &str) -> String {
    garraia_gateway::bootstrap::whatsapp_linked_chave_do_portao(
        &garraia_gateway::bootstrap::whatsapp_linked_normalizar_identidade(identidade),
    )
}

// ---------------------------------------------------------------------------
// Quem esta na lista (#1393)
// ---------------------------------------------------------------------------

/// Uma identidade autorizada, **ja mascarada**.
///
/// Nao carrega o numero: o unico pedaco que sai deste modulo para qualquer
/// tela — humana ou JSON — sao os quatro ultimos digitos, como no resto do
/// comando. Quem quiser o valor inteiro le o `config.yml`, que e 0600.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Autorizado {
    /// Os quatro ultimos digitos do numero (ou do id, antes do `@lid`).
    pub final4: String,
    pub papel: Papel,
    /// A identidade e um LID (`<id>@lid`), e nao um numero?
    pub lid: bool,
}

/// Quem a config autoriza, pelo MESMO leitor que o gateway usa no turno.
///
/// Donos primeiro, e a uniao e feita pela chave do portao: o mesmo celular em
/// `allow` e em `owners` (ou com e sem o nono digito) e **uma** linha, com o
/// papel `owners` — que e o que o gateway honra. Contar de outro jeito daria
/// ao operador uma lista que nao bate com as contagens do `status`.
pub fn listar(config: &AppConfig) -> Vec<Autorizado> {
    let s = garraia_gateway::bootstrap::whatsapp_linked_settings(config);
    let mut vistos = std::collections::HashSet::new();
    let mut out = Vec::new();
    for (identidades, papel) in [(&s.owners, Papel::Dono), (&s.allow, Papel::Autorizado)] {
        for identidade in identidades {
            if !vistos.insert(chave_do_portao(identidade)) {
                continue;
            }
            out.push(Autorizado {
                final4: final4(identidade).to_string(),
                papel,
                lid: e_lid(identidade),
            });
        }
    }
    out
}

/// O desfecho de [`autorizar`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Gravado {
    Novo,
    JaEstava,
}

// ---------------------------------------------------------------------------
// A secao do canal, para escrita
// ---------------------------------------------------------------------------

/// A recusa que `autorizar`, `remover`, `promover` e `rebaixar` fazem IGUAL:
/// uma secao `channels.whatsapp_linked` com `type:` de outro canal.
///
/// O gateway ignora a secao inteira nesse caso, entao escrever nela seria
/// afirmar uma autorizacao (ou uma revogacao) que nao vale. Trocar o `type` de
/// uma secao do operador tambem nao e decisao de nenhum destes comandos.
fn checar_tipo(secao: &ChannelConfig) -> Result<()> {
    if secao.channel_type != CONFIG_KEY {
        bail!(
            "`channels.{CONFIG_KEY}` existe com `type: {}` — corrija para `type: {CONFIG_KEY}` no config.yml",
            secao.channel_type
        );
    }
    Ok(())
}

/// A secao do canal para escrita, nascendo com `type: whatsapp_linked` e sem
/// `enabled` (desligado, ver `settings_from_config`) quando nao existe.
fn secao_criando(config: &mut AppConfig) -> Result<&mut ChannelConfig> {
    let secao = config
        .channels
        .entry(CONFIG_KEY.to_string())
        .or_insert_with(|| ChannelConfig {
            channel_type: CONFIG_KEY.to_string(),
            enabled: None,
            settings: Default::default(),
        });
    checar_tipo(secao)?;
    Ok(secao)
}

/// A secao do canal para escrita, **sem** cria-la: `None` numa config que nao
/// tem o canal. Quem so tira nome de lista nao inventa secao.
fn secao_existente(config: &mut AppConfig) -> Result<Option<&mut ChannelConfig>> {
    let Some(secao) = config.channels.get_mut(CONFIG_KEY) else {
        return Ok(None);
    };
    checar_tipo(secao)?;
    Ok(Some(secao))
}

/// A lista `allow` ou `owners` da secao, nascendo vazia quando ausente.
fn lista_mut<'a>(
    secao: &'a mut ChannelConfig,
    chave: &str,
) -> Result<&'a mut Vec<serde_json::Value>> {
    let valor = secao
        .settings
        .entry(chave.to_string())
        .or_insert_with(|| serde_json::Value::Array(Vec::new()));
    let Some(itens) = valor.as_array_mut() else {
        bail!("`channels.{CONFIG_KEY}.{chave}` nao e uma lista — corrija o config.yml");
    };
    Ok(itens)
}

/// A lista tem `alvo` (ja na chave do portao)?
fn contem(itens: &[serde_json::Value], alvo: &str) -> bool {
    itens
        .iter()
        .filter_map(|v| v.as_str())
        .any(|v| chave_do_portao(v) == alvo)
}

/// A lista `chave` da secao tem `alvo`? Uma lista ausente (ou que nao e lista)
/// nao tem ninguem — quem precisa recusar o que nao e lista e [`lista_mut`],
/// na hora de escrever.
fn contem_em(secao: &ChannelConfig, chave: &str, alvo: &str) -> bool {
    secao
        .settings
        .get(chave)
        .and_then(|v| v.as_array())
        .is_some_and(|itens| contem(itens, alvo))
}

/// Acrescenta `numero` (ja normalizado) a `allow` ou `owners`.
///
/// Carrega, mexe so na lista pedida, e grava pela escrita atomica `0600` do
/// [`ConfigLoader::save`]. Os **valores** ficam como estavam: as outras
/// chaves da secao (`default_mode`, `reply_in_groups`, a outra lista), as
/// outras secoes e **`enabled`** — autorizar nao liga canal; quem liga e o
/// `link`, depois de a sessao existir. Secao ausente nasce com `type:
/// whatsapp_linked` e sem `enabled` (desligado, ver `settings_from_config`).
///
/// O arquivo, porem, e **reescrito** a partir da config lida (como todo
/// `save`): comentarios, chaves que o `AppConfig` nao modela e a formatacao
/// original nao sobrevivem, e secoes com default passam a aparecer. A
/// documentacao diz isso; quem cura o `config.yml` a mao edita a lista a mao.
///
/// Nunca remove: quem revoga e [`remover`] (#1394), e o gateway rele a lista
/// a quente.
pub fn autorizar(loader: &ConfigLoader, numero: &str, papel: Papel) -> Result<Gravado> {
    loader.ensure_dirs()?;
    // Sem a env do perfil: ela nao vai ao disco (`#[serde(skip)]`) e nao
    // decide nada aqui — so o `--owner` depende dela, e ja foi validado.
    let mut config = loader.load_sem_env()?;
    let secao = secao_criando(&mut config)?;
    let itens = lista_mut(secao, papel.chave())?;
    // A mesma chave que o portao compara: `+55 31 99999-8888` ja cobre
    // `553199998888` (o nono digito, ver [`chave_do_portao`]).
    let alvo = chave_do_portao(numero);
    if contem(itens, &alvo) {
        return Ok(Gravado::JaEstava);
    }
    itens.push(serde_json::Value::String(numero.to_string()));
    loader.save(&config)?;
    Ok(Gravado::Novo)
}

/// Quantas entradas sairam de cada lista (#1394).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Remocao {
    pub de_allow: usize,
    pub de_owners: usize,
}

impl Remocao {
    pub fn total(self) -> usize {
        self.de_allow + self.de_owners
    }

    /// O numero removido era dono?
    pub fn era_dono(self) -> bool {
        self.de_owners > 0
    }
}

/// Tira `numero` (ja normalizado) de `allow` **e** de `owners`.
///
/// O espelho de [`autorizar`], com as mesmas tres promessas: carrega pelo
/// `load_sem_env`, mexe so nas duas listas e grava pela escrita atomica
/// `0600` do [`ConfigLoader::save`]. `enabled` nao e tocado — remover o
/// ultimo autorizado **nao** desliga o canal, porque desligar e decisao do
/// `logout`, e um canal ligado com portao vazio e um estado que o `status` ja
/// sabe explicar.
///
/// Sai das DUAS listas de proposito: "remover" para quem digita e revogar o
/// acesso, e um numero que estivesse em `allow` e em `owners` continuaria
/// entrando pela outra porta — o portao do gateway admite a uniao.
///
/// Compara pela chave do portao, entao remove o que `allow` gravaria: o mesmo
/// celular com e sem o nono digito, com ou sem separadores. Nada e criado:
/// numa config sem a secao o resultado e [`Remocao::total`] zero e o arquivo
/// nao e reescrito. Como todo `save`, comentarios do `config.yml` nao
/// sobrevivem a uma remocao que de fato grava.
///
/// Devolve tambem o [`Acesso`] **depois** da remocao, calculado da config que
/// acabou de ir ao disco. Reler o arquivo para descobrir se o portao ficou
/// vazio custava uma leitura que pode falhar bem no momento em que o operador
/// mais precisa do aviso — e engolir esse `Err` esconderia, de uma so vez,
/// "ninguem mais esta autorizado" e "a config ficou ilegivel logo depois de
/// eu grava-la".
pub fn remover(loader: &ConfigLoader, numero: &str) -> Result<(Remocao, Acesso)> {
    loader.ensure_dirs()?;
    let mut config = loader.load_sem_env()?;
    let alvo = chave_do_portao(numero);
    let mut fora = Remocao::default();
    if let Some(secao) = secao_existente(&mut config)? {
        for (chave, conta) in [
            ("allow", &mut fora.de_allow),
            ("owners", &mut fora.de_owners),
        ] {
            let Some(lista) = secao.settings.get_mut(chave) else {
                continue;
            };
            let Some(itens) = lista.as_array_mut() else {
                bail!("`channels.{CONFIG_KEY}.{chave}` nao e uma lista — corrija o config.yml");
            };
            let antes = itens.len();
            // Entrada que nao e string fica: ela nao e este numero, e
            // descartar o que nao se entende seria apagar escolha do operador.
            itens.retain(|v| v.as_str().is_none_or(|s| chave_do_portao(s) != alvo));
            *conta = antes - itens.len();
        }
    }
    if fora.total() > 0 {
        loader.save(&config)?;
    }
    // Do MESMO `config` que foi gravado: e o estado que o gateway vai ler.
    Ok((fora, acesso_da_config(&config)))
}

// ---------------------------------------------------------------------------
// Promover e rebaixar (#1395)
// ---------------------------------------------------------------------------

/// O desfecho de [`promover`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Promovido {
    /// Entrou em `owners` agora.
    Novo {
        /// A identidade NAO estava em `allow`: a promocao tambem abriu o
        /// portao para ela (o gateway admite a uniao das duas listas), e a
        /// tela tem de dizer isso — promover nao pode dar acesso em silencio.
        ganhou_acesso: bool,
    },
    /// Ja era dono — promover e idempotente.
    JaEra,
}

/// Poe `numero` (ja normalizado) em `owners`.
///
/// E a MESMA escrita que `allow --owner` faz — `owners`, e so `owners` (ADR
/// 0024) —, pela mesma chave do portao e pela mesma escrita atomica `0600` do
/// [`ConfigLoader::save`]. Nao duplicar a convencao e o ponto: o `owner` e o
/// caminho dedicado para promover quem ja esta autorizado, nao uma segunda
/// forma de gravar dono.
///
/// `allow` nao e tocado. Quem ja estava la continua la (e o [`listar`] mostra
/// uma linha so, com o papel `owners`, que e o que o gateway honra); quem nao
/// estava ganha acesso pela uniao, e o [`Promovido::Novo`] diz isso ao
/// chamador para a tela avisar.
///
/// Devolve o [`Acesso`] **depois**, calculado da config que acabou de ir ao
/// disco — sem reler o arquivo, pela mesma razao do [`remover`].
pub fn promover(loader: &ConfigLoader, numero: &str) -> Result<(Promovido, Acesso)> {
    loader.ensure_dirs()?;
    let mut config = loader.load_sem_env()?;
    let alvo = chave_do_portao(numero);
    let promovido = {
        let secao = secao_criando(&mut config)?;
        let ja_autorizado = contem_em(secao, "allow", &alvo);
        let owners = lista_mut(secao, "owners")?;
        if contem(owners, &alvo) {
            Promovido::JaEra
        } else {
            owners.push(serde_json::Value::String(numero.to_string()));
            Promovido::Novo {
                ganhou_acesso: !ja_autorizado,
            }
        }
    };
    if matches!(promovido, Promovido::Novo { .. }) {
        loader.save(&config)?;
    }
    Ok((promovido, acesso_da_config(&config)))
}

/// O desfecho de [`rebaixar`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Rebaixado {
    /// Saiu de `owners`.
    Feito {
        /// A entrada foi COPIADA para `allow` porque so existia em `owners`.
        /// Sem isso, rebaixar teria tirado o acesso junto com o papel — que e
        /// exatamente o que o `unowner` nao pode fazer.
        movido_para_allow: bool,
    },
    /// Nao era dono — rebaixar e idempotente.
    NaoEra,
}

/// Tira `numero` (ja normalizado) de `owners` **preservando o acesso**.
///
/// A diferenca inteira entre este comando e o [`remover`]: quem e rebaixado
/// continua podendo falar com o GarraIA, so perde o piso de dono. Como o
/// `allow --owner` grava **so** em `owners`, o caso comum e a identidade
/// existir apenas la — tirar dali e pronto seria revogar o acesso em silencio.
/// Entao, quando `allow` ainda nao a tem, a entrada e copiada para `allow`
/// **na mesma escrita** (um `save` so: nao existe instante no disco em que a
/// pessoa nao esteja em nenhuma das duas listas).
///
/// A entrada copiada e a string que estava em `owners`, byte a byte, e nao o
/// `numero` normalizado: o que o operador gravou (um LID, o celular sem o nono
/// digito) continua sendo o que o `config.yml` mostra.
///
/// Nao cria secao e nao escreve quando nao havia o que rebaixar. `enabled` nao
/// e tocado, como em [`autorizar`] e [`remover`].
pub fn rebaixar(loader: &ConfigLoader, numero: &str) -> Result<(Rebaixado, Acesso)> {
    loader.ensure_dirs()?;
    let mut config = loader.load_sem_env()?;
    let alvo = chave_do_portao(numero);
    let mut rebaixado = Rebaixado::NaoEra;
    if let Some(secao) = secao_existente(&mut config)? {
        let owners = lista_mut(secao, "owners")?;
        // A forma exata que estava gravada, para reescreve-la em `allow`.
        let gravada = owners
            .iter()
            .filter_map(|v| v.as_str())
            .find(|s| chave_do_portao(s) == alvo)
            .map(str::to_string);
        let antes = owners.len();
        // Entrada que nao e string fica, como no `remover`.
        owners.retain(|v| v.as_str().is_none_or(|s| chave_do_portao(s) != alvo));
        if antes != owners.len() {
            let allow = lista_mut(secao, "allow")?;
            let movido_para_allow = !contem(allow, &alvo);
            if movido_para_allow {
                allow.push(serde_json::Value::String(
                    gravada.unwrap_or_else(|| numero.to_string()),
                ));
            }
            rebaixado = Rebaixado::Feito { movido_para_allow };
        }
    }
    if matches!(rebaixado, Rebaixado::Feito { .. }) {
        loader.save(&config)?;
    }
    Ok((rebaixado, acesso_da_config(&config)))
}

// ---------------------------------------------------------------------------
// O gateway que esta rodando
// ---------------------------------------------------------------------------

/// O que dizer sobre o gateway depois de mexer no acesso.
///
/// A CLI **nao reinicia** nada: ela nao sabe o host, a porta, nem se o
/// gateway roda em primeiro plano, sob systemd ou num container, e um restart
/// mataria turnos em andamento nos outros canais. Ela diz qual e o caso.
///
/// "Sem reiniciar" nunca e afirmado: a CLI le o `enabled` do **arquivo**, e
/// o gateway so supervisiona o canal se ele estava ligado quando subiu (o
/// `link` pode te-lo ligado depois), e so rele a lista a quente se havia um
/// `config.yml` para vigiar no boot. Nenhum dos dois e visivel daqui, entao o
/// texto diz a condicao e o comando do outro caso.
pub fn dica_do_gateway(lang: Lang, canal_ja_supervisionado: bool, pid: Option<u32>) -> String {
    match (pid, canal_ja_supervisionado) {
        (Some(_), true) => tb(
            lang,
            "Se o gateway subiu com este canal ligado, ele aplica isto na próxima mensagem, sem reiniciar; se não, rode `{bin} restart`.",
            "If the gateway started with this channel on, it applies this on the next message, no restart needed; otherwise run `{bin} restart`.",
        ),
        (Some(_), false) => tb(
            lang,
            "O gateway está rodando sem este canal: rode `{bin} restart` para ele subir o WhatsApp.",
            "The gateway is running without this channel: run `{bin} restart` so it starts WhatsApp.",
        ),
        (None, _) => tb(
            lang,
            "Inicie o gateway: `{bin} start`.",
            "Start the gateway: `{bin} start`.",
        ),
    }
}

/// O aviso de "ninguem vai receber resposta". Sem numero nenhum.
pub fn aviso_ninguem_autorizado(lang: Lang) -> String {
    tb(
        lang,
        "⚠ Ninguém está autorizado a falar com o GarraIA por este WhatsApp — toda mensagem será ignorada em silêncio. Autorize um número: `{bin} whatsapp allow <número>` (com o código do país).",
        "⚠ Nobody is authorized to talk to GarraIA through this WhatsApp — every message will be silently ignored. Authorize a number: `{bin} whatsapp allow <number>` (with the country code).",
    )
}

/// O aviso do `from_me`: o celular vinculado nao fala com o GarraIA.
fn aviso_do_proprio_numero(lang: Lang) -> &'static str {
    t(
        lang,
        "⚠ Este parece ser o número do próprio celular vinculado. Mensagens enviadas por ele são ignoradas (elas saem da própria conta), então ele não conversa com o GarraIA — use outro número.",
        "⚠ This looks like the number of the linked phone itself. Messages it sends are ignored (they come from the account itself), so it cannot chat with GarraIA — use another number.",
    )
}

/// O texto da confirmacao de dono. Diz o que o dono ganha.
fn pergunta_de_dono(lang: Lang) -> &'static str {
    t(
        lang,
        "Tornar este número DONO? Em isolated-pod o dono recebe o piso `code` em conversa 1:1: arquivos, bash, ferramentas MCP e subagentes (em grupo, nunca)",
        "Make this number an OWNER? In isolated-pod the owner gets the `code` floor in 1:1 chats: files, bash, MCP tools and subagents (never in groups)",
    )
}

// ---------------------------------------------------------------------------
// `garraia whatsapp allow <numero> [--owner] [--yes]`
// ---------------------------------------------------------------------------

/// O pedido do `allow` (e as pre-respostas do `link --allow/--owner`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Pedido {
    pub numero: Option<String>,
    pub owner: bool,
    pub yes: bool,
}

/// Valida as flags que nao dependem de terminal: o numero (65) e o `--owner`
/// fora de `isolated-pod` (64). Devolve o numero normalizado, quando veio.
fn validar(ctx: &Context, pedido: &Pedido, config: &AppConfig) -> Result<Option<String>, i32> {
    let numero = match pedido.numero.as_deref() {
        Some(raw) => match normalizar_numero(raw) {
            Ok(n) => Some(n),
            Err(e) => {
                eprintln!("{}", e.mensagem(ctx.lang));
                return Err(EX_DATAERR);
            }
        },
        None => None,
    };
    if pedido.owner && !config.execution.perfil().is_isolated_pod() {
        eprintln!(
            "{}",
            t(
                ctx.lang,
                "`--owner` só vale com `execution.profile = isolated-pod`. Em `standard` um dono não tem poder nenhum — e ganharia em silêncio no dia em que o perfil mudasse. Autorize sem `--owner`. (O perfil lido aqui vem do config.yml ou de `GARRAIA_EXECUTION_PROFILE` NESTE shell; se o gateway roda com essa env, rode este comando com a mesma env.)",
                "`--owner` only applies with `execution.profile = isolated-pod`. In `standard` an owner has no power — and would silently gain it the day the profile changed. Authorize without `--owner`. (The profile read here comes from config.yml or from `GARRAIA_EXECUTION_PROFILE` in THIS shell; if the gateway runs with that env, run this command with the same env.)",
            )
        );
        return Err(EX_USAGE);
    }
    Ok(numero)
}

/// A config como o gateway a veria: o arquivo, e por cima a env do perfil
/// **capturada no [`Context`]** (e nao relida do processo) — assim o teste
/// fixa o perfil sem depender do `GARRAIA_EXECUTION_PROFILE` da maquina.
fn carregar_com_env(ctx: &Context, loader: &ConfigLoader) -> garraia_common::Result<AppConfig> {
    let mut config = loader.load_sem_env()?;
    config
        .execution
        .aplicar_valor_da_env(ctx.perfil_da_env.as_deref())
        .map_err(|e| garraia_common::Error::Config(e.to_string()))?;
    Ok(config)
}

fn carregar(ctx: &Context) -> Result<(&ConfigLoader, AppConfig), i32> {
    let Some(loader) = ctx.loader.as_ref() else {
        eprintln!(
            "{}",
            t(
                ctx.lang,
                "A config do GarraIA não está acessível neste ambiente.",
                "The GarraIA config is not accessible in this environment.",
            )
        );
        return Err(EX_SOFTWARE);
    };
    match carregar_com_env(ctx, loader) {
        Ok(c) => Ok((loader, c)),
        Err(e) => {
            eprintln!("{e}");
            Err(EX_SOFTWARE)
        }
    }
}

/// `garraia whatsapp allow`. Funciona sem terminal.
pub fn allow(ctx: &Context, prompter: &dyn Prompter, pedido: &Pedido) -> i32 {
    let (loader, config) = match carregar(ctx) {
        Ok(v) => v,
        Err(code) => return code,
    };
    let numero = match validar(ctx, pedido, &config) {
        Ok(Some(n)) => n,
        // O clap exige o argumento; `None` so viria de um chamador interno.
        Ok(None) => {
            eprintln!("{}", NumeroInvalido::Vazio.mensagem(ctx.lang));
            return EX_DATAERR;
        }
        Err(code) => return code,
    };

    let papel = if pedido.owner {
        if !pedido.yes {
            if !ctx.interactive {
                eprintln!(
                    "{}",
                    t(
                        ctx.lang,
                        "`--owner` sem terminal precisa de `--yes`: tornar alguém dono é uma decisão explícita.",
                        "`--owner` without a terminal needs `--yes`: making someone an owner is an explicit decision.",
                    )
                );
                return EX_USAGE;
            }
            match prompter.confirm(pergunta_de_dono(ctx.lang), false) {
                Ok(true) => {}
                Ok(false) | Err(_) => {
                    println!("{}", t(ctx.lang, "Cancelado.", "Cancelled."));
                    return EX_CANCELLED;
                }
            }
        }
        Papel::Dono
    } else {
        Papel::Autorizado
    };

    let antes = acesso_da_config(&config);
    match autorizar(loader, &numero, papel) {
        Ok(gravado) => imprimir_gravado(ctx.lang, &numero, papel, gravado),
        Err(e) => {
            eprintln!("{e}");
            return EX_SOFTWARE;
        }
    }
    if antes.enabled {
        println!("{}", dica_do_gateway(ctx.lang, true, ctx.gateway_pid));
    } else {
        println!(
            "{}",
            tb(
                ctx.lang,
                "O canal ainda não está ligado: vincule o WhatsApp com `{bin} whatsapp link`.",
                "The channel is not on yet: link WhatsApp with `{bin} whatsapp link`.",
            )
        );
    }
    0
}

fn imprimir_gravado(lang: Lang, numero: &str, papel: Papel, gravado: Gravado) {
    let fim = final4(numero);
    if e_lid(numero) {
        let linha = match (lang, papel, gravado) {
            (Lang::Pt, Papel::Autorizado, Gravado::Novo) => {
                format!("✓ LID terminado em {fim} autorizado.")
            }
            (Lang::Pt, Papel::Dono, Gravado::Novo) => {
                format!("✓ LID terminado em {fim} registrado como dono.")
            }
            (Lang::Pt, _, Gravado::JaEstava) => {
                format!("✓ O LID terminado em {fim} já estava na lista.")
            }
            (Lang::En, Papel::Autorizado, Gravado::Novo) => {
                format!("✓ LID ending in {fim} authorized.")
            }
            (Lang::En, Papel::Dono, Gravado::Novo) => {
                format!("✓ LID ending in {fim} registered as owner.")
            }
            (Lang::En, _, Gravado::JaEstava) => {
                format!("✓ The LID ending in {fim} was already listed.")
            }
        };
        println!("{linha}");
        return;
    }
    let linha = match (lang, papel, gravado) {
        (Lang::Pt, Papel::Autorizado, Gravado::Novo) => {
            format!("✓ Número terminado em {fim} autorizado.")
        }
        (Lang::Pt, Papel::Dono, Gravado::Novo) => {
            format!("✓ Número terminado em {fim} registrado como dono.")
        }
        (Lang::Pt, _, Gravado::JaEstava) => {
            format!("✓ O número terminado em {fim} já estava na lista.")
        }
        (Lang::En, Papel::Autorizado, Gravado::Novo) => {
            format!("✓ Number ending in {fim} authorized.")
        }
        (Lang::En, Papel::Dono, Gravado::Novo) => {
            format!("✓ Number ending in {fim} registered as owner.")
        }
        (Lang::En, _, Gravado::JaEstava) => {
            format!("✓ The number ending in {fim} was already listed.")
        }
    };
    println!("{linha}");
}

// ---------------------------------------------------------------------------
// `garraia whatsapp users [--json]` (#1393)
// ---------------------------------------------------------------------------

/// `garraia whatsapp users`. So le; funciona sem terminal.
///
/// O `status` ja dizia QUANTOS podem falar com o GarraIA, e so isso: quem
/// tinha autorizado tres numeros meses atras nao tinha como saber quais eram
/// sem abrir o `config.yml` a mao. Este comando responde "quem", no mesmo
/// limite do resto do modulo — **quatro ultimos digitos**, nunca a identidade
/// inteira, nem na tela nem no `--json`.
pub fn users(ctx: &Context, json: bool) -> i32 {
    let (_, config) = match carregar(ctx) {
        Ok(v) => v,
        Err(code) => return code,
    };
    let acesso = acesso_da_config(&config);
    let usuarios = listar(&config);
    if json {
        // Nada de cabecalho nem de frase solta: num `--json` a saida inteira
        // e o documento, para `jq` poder le-la direto.
        match serde_json::to_string_pretty(&json_de_usuarios(acesso, &usuarios)) {
            Ok(texto) => println!("{texto}"),
            Err(e) => {
                eprintln!("{e}");
                return EX_SOFTWARE;
            }
        }
        return 0;
    }
    super::print_header(ctx);
    for linha in linhas_de_usuarios(ctx.lang, acesso, &usuarios) {
        println!("{linha}");
    }
    0
}

/// As tres linhas de acesso que o `status` e o `users` dizem IGUAL: canal
/// ligado ou nao, as contagens, e o aviso de portao vazio.
///
/// Uma copia so. A versao anterior deste comando repetia as quatro frases
/// (pt/en × ligado/desligado) e a linha de contagem dentro do `users`, e duas
/// telas que discordam sobre quantos donos ha sao piores do que uma so —
/// exatamente o defeito que o `execution_profile_line` ja tinha pago uma vez
/// (review C3/C8/C13 da #1329, quando a CLI reimplementava a contagem).
///
/// O aviso so sai com o canal **ligado**, como no `status`: com o canal
/// desligado ninguem recebe mensagem de todo jeito, e "autorize um numero"
/// nao e o passo que resolve — o passo e o `link`.
pub fn linhas_de_acesso(lang: Lang, acesso: Acesso) -> Vec<String> {
    let mut out = vec![
        match (lang, acesso.enabled) {
            (Lang::Pt, true) => "Canal:    ligado",
            (Lang::Pt, false) => "Canal:    desligado",
            (Lang::En, true) => "Channel:  on",
            (Lang::En, false) => "Channel:  off",
        }
        .to_string(),
        match lang {
            Lang::Pt => format!(
                "Autorizados: {} · Donos: {}",
                acesso.autorizados, acesso.donos
            ),
            Lang::En => format!(
                "Authorized: {} · Owners: {}",
                acesso.autorizados, acesso.donos
            ),
        },
    ];
    if acesso.enabled && acesso.autorizados == 0 {
        out.push(aviso_ninguem_autorizado(lang));
    }
    out
}

/// As linhas do `users`, puras para o teste.
pub fn linhas_de_usuarios(lang: Lang, acesso: Acesso, usuarios: &[Autorizado]) -> Vec<String> {
    let mut out = linhas_de_acesso(lang, acesso);
    if usuarios.is_empty() {
        return out;
    }
    out.push(String::new());
    for u in usuarios {
        out.push(linha_de_usuario(lang, u));
    }
    out
}

/// Uma linha da lista: papel, tipo e os quatro ultimos digitos.
fn linha_de_usuario(lang: Lang, u: &Autorizado) -> String {
    match lang {
        Lang::Pt => {
            let papel = match u.papel {
                Papel::Dono => "dono",
                Papel::Autorizado => "autorizado",
            };
            let tipo = if u.lid { "LID" } else { "número" };
            format!("  {papel:<10} · {tipo} terminado em {}", u.final4)
        }
        Lang::En => {
            let papel = match u.papel {
                Papel::Dono => "owner",
                Papel::Autorizado => "authorized",
            };
            let tipo = if u.lid { "LID" } else { "number" };
            format!("  {papel:<10} · {tipo} ending in {}", u.final4)
        }
    }
}

/// O documento do `--json`. Puro, para o teste afirmar o contrato de script.
///
/// As chaves sao as da config (`allow`/`owners`), e nao as palavras da tela:
/// quem consome isto quer casar com o `config.yml`, e um `dono`/`owner`
/// traduzido mudaria de valor com o locale da maquina.
pub fn json_de_usuarios(acesso: Acesso, usuarios: &[Autorizado]) -> serde_json::Value {
    serde_json::json!({
        "enabled": acesso.enabled,
        "authorized": acesso.autorizados,
        "owners": acesso.donos,
        "users": usuarios
            .iter()
            .map(|u| serde_json::json!({
                "role": u.papel.json(),
                "kind": if u.lid { "lid" } else { "number" },
                // `last4`, e nao `number`: o nome do campo tem de dizer que
                // ali nunca vai a identidade inteira.
                "last4": u.final4,
            }))
            .collect::<Vec<_>>(),
    })
}

// ---------------------------------------------------------------------------
// `garraia whatsapp remove <numero> [--yes]` (#1394)
// ---------------------------------------------------------------------------

/// O pedido do `remove`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PedidoRemocao {
    pub numero: String,
    pub yes: bool,
}

/// O texto da confirmacao de remocao de dono. Diz o que o dono perde.
fn pergunta_de_remocao_de_dono(lang: Lang) -> &'static str {
    t(
        lang,
        "Este número é DONO. Remover mesmo assim? Ele perde o acesso e o piso de dono em conversa 1:1",
        "This number is an OWNER. Remove it anyway? It loses access and the owner floor in 1:1 chats",
    )
}

/// O numero esta em `owners`, pela chave do portao?
fn e_dono_na_config(config: &AppConfig, numero: &str) -> bool {
    let alvo = chave_do_portao(numero);
    garraia_gateway::bootstrap::whatsapp_linked_settings(config)
        .owners
        .iter()
        .any(|v| chave_do_portao(v) == alvo)
}

/// `garraia whatsapp remove`. Funciona sem terminal — menos para dono.
///
/// O espelho do [`allow`], com a assimetria que importa: **dono exige
/// confirmacao explicita**. Tornar alguem dono ja pedia `--yes` fora do
/// terminal; tirar o dono e a operacao que pode deixar o operador de fora do
/// proprio GarraIA, entao ela pede o mesmo, e nunca acontece em silencio.
///
/// Quem nao estava na lista sai 0: remover e idempotente, como autorizar duas
/// vezes o mesmo numero — um script que roda de novo nao pode falhar por ter
/// dado certo antes.
pub fn remove(ctx: &Context, prompter: &dyn Prompter, pedido: &PedidoRemocao) -> i32 {
    let (loader, config) = match carregar(ctx) {
        Ok(v) => v,
        Err(code) => return code,
    };
    let numero = match normalizar_numero(&pedido.numero) {
        Ok(n) => n,
        Err(e) => {
            eprintln!("{}", e.mensagem(ctx.lang));
            return EX_DATAERR;
        }
    };

    // O papel sai da config que este comando leu; o `remover` rele o arquivo
    // antes de gravar (nunca escreve por cima de uma leitura velha). A janela
    // entre as duas leituras e teorica numa CLI de um usuario so, e fecha-la
    // exigiria segurar o arquivo aberto entre a pergunta ao operador e a
    // escrita — o que travaria o gateway enquanto o prompt espera resposta.
    if e_dono_na_config(&config, &numero) && !pedido.yes {
        if !ctx.interactive {
            eprintln!(
                "{}",
                t(
                    ctx.lang,
                    "Este número é DONO: removê-lo sem terminal precisa de `--yes`. Revogar um dono é uma decisão explícita.",
                    "This number is an OWNER: removing it without a terminal needs `--yes`. Revoking an owner is an explicit decision.",
                )
            );
            return EX_USAGE;
        }
        match prompter.confirm(pergunta_de_remocao_de_dono(ctx.lang), false) {
            Ok(true) => {}
            Ok(false) | Err(_) => {
                println!("{}", t(ctx.lang, "Cancelado.", "Cancelled."));
                return EX_CANCELLED;
            }
        }
    }

    let (fora, depois) = match remover(loader, &numero) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("{e}");
            return EX_SOFTWARE;
        }
    };
    if fora.total() == 0 {
        println!("{}", linha_de_nao_estava(ctx.lang, &numero));
        return 0;
    }
    println!("{}", linha_de_removido(ctx.lang, &numero, fora));

    // O estado DEPOIS vem da config que o `remover` gravou — sem reler o
    // arquivo, e portanto sem um `Err` para engolir justamente na hora em que
    // o operador acabou de esvaziar o portao.
    //
    // So o canal ligado precisa de dica: num canal desligado nao ha turno em
    // andamento para a remocao alcancar.
    if depois.enabled {
        if depois.autorizados == 0 {
            println!("{}", aviso_ninguem_autorizado(ctx.lang));
        }
        println!("{}", dica_do_gateway(ctx.lang, true, ctx.gateway_pid));
    }
    0
}

/// A linha do desfecho que gravou. Pura, e so com o final do numero.
pub fn linha_de_removido(lang: Lang, numero: &str, fora: Remocao) -> String {
    let fim = final4(numero);
    let tipo = tipo_da_identidade(lang, numero);
    match (lang, fora.era_dono()) {
        (Lang::Pt, false) => format!("✓ {tipo} terminado em {fim} removido dos autorizados."),
        (Lang::Pt, true) => {
            format!("✓ {tipo} terminado em {fim} removido — ele era DONO e não tem mais acesso.")
        }
        (Lang::En, false) => format!("✓ {tipo} ending in {fim} removed from the allow list."),
        (Lang::En, true) => {
            format!("✓ {tipo} ending in {fim} removed — it was an OWNER and has no access now.")
        }
    }
}

/// A linha de quem nao estava na lista. Pura.
pub fn linha_de_nao_estava(lang: Lang, numero: &str) -> String {
    let fim = final4(numero);
    match (lang, e_lid(numero)) {
        (Lang::Pt, true) => format!("O LID terminado em {fim} não estava na lista — nada mudou."),
        (Lang::Pt, false) => {
            format!("O número terminado em {fim} não estava na lista — nada mudou.")
        }
        (Lang::En, true) => format!("The LID ending in {fim} was not listed — nothing changed."),
        (Lang::En, false) => {
            format!("The number ending in {fim} was not listed — nothing changed.")
        }
    }
}

// ---------------------------------------------------------------------------
// `garraia whatsapp owner|unowner <numero> [--yes]` (#1395)
// ---------------------------------------------------------------------------

/// O pedido do `owner` e do `unowner`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PedidoDePapel {
    pub numero: String,
    pub yes: bool,
}

/// Como se diz "LID"/"número" na lingua, para as linhas de desfecho.
fn tipo_da_identidade(lang: Lang, numero: &str) -> &'static str {
    match (lang, e_lid(numero)) {
        (_, true) => "LID",
        (Lang::Pt, false) => "Número",
        (Lang::En, false) => "Number",
    }
}

/// A recusa do `owner` fora de `isolated-pod`.
///
/// Mesma regra do `allow --owner`, e pela mesma razao (ADR 0024): em
/// `standard` o dono nao tem poder nenhum e ganharia tudo em silencio no dia
/// em que o perfil mudasse. O texto e proprio porque o comando e outro — quem
/// rodou `whatsapp owner` nao passou flag nenhuma para lhe tirarem.
fn recusa_de_dono_fora_do_pod(lang: Lang) -> &'static str {
    t(
        lang,
        "`whatsapp owner` só vale com `execution.profile = isolated-pod`. Em `standard` um dono não tem poder nenhum — e ganharia em silêncio no dia em que o perfil mudasse. Para só autorizar o número, use `whatsapp allow`. (O perfil lido aqui vem do config.yml ou de `GARRAIA_EXECUTION_PROFILE` NESTE shell; se o gateway roda com essa env, rode este comando com a mesma env.)",
        "`whatsapp owner` only applies with `execution.profile = isolated-pod`. In `standard` an owner has no power — and would silently gain it the day the profile changed. To merely authorize the number, use `whatsapp allow`. (The profile read here comes from config.yml or from `GARRAIA_EXECUTION_PROFILE` in THIS shell; if the gateway runs with that env, run this command with the same env.)",
    )
}

/// O que o dono ganha, dito depois de uma promocao que de fato gravou.
///
/// O `--yes` pula a pergunta, e com ela a unica frase que explicava o poder;
/// dizer isto no desfecho mantem o aviso no caminho do script tambem.
fn nota_do_poder_de_dono(lang: Lang) -> &'static str {
    t(
        lang,
        "Em isolated-pod o dono recebe o piso `code` em conversa 1:1: arquivos, bash, ferramentas MCP e subagentes (em grupo, nunca).",
        "In isolated-pod the owner gets the `code` floor in 1:1 chats: files, bash, MCP tools and subagents (never in groups).",
    )
}

/// A pergunta do rebaixamento do ULTIMO dono. Diz o que fica e o que sai.
fn pergunta_de_ultimo_dono(lang: Lang) -> &'static str {
    t(
        lang,
        "Este é o ÚNICO dono: rebaixá-lo deixa a configuração sem dono nenhum, e ninguém terá o piso `code` em conversa 1:1. O acesso dele é preservado. Rebaixar mesmo assim?",
        "This is the ONLY owner: demoting it leaves the configuration with no owner at all, and nobody will have the `code` floor in 1:1 chats. Its access is preserved. Demote anyway?",
    )
}

/// `garraia whatsapp owner`. Promove quem ja esta autorizado a dono.
///
/// As mesmas duas portas do `allow --owner`, porque a escrita e a mesma:
/// `isolated-pod` obrigatorio (64 fora dele) e confirmacao explicita — `--yes`
/// num pipe, pergunta com default NAO no terminal. Promover duas vezes sai 0:
/// e idempotente, como autorizar.
///
/// Quem ainda nao estava em `allow` **e** promovido assim mesmo, e nao
/// recusado: `owners` ja e por si so uma porta do portao (o gateway admite a
/// uniao), entao e exatamente o que `allow --owner` faz hoje. Inventar aqui um
/// "primeiro autorize, depois promova" criaria uma segunda convencao para a
/// mesma escrita. A tela avisa que a promocao tambem deu acesso.
pub fn owner(ctx: &Context, prompter: &dyn Prompter, pedido: &PedidoDePapel) -> i32 {
    let (loader, config) = match carregar(ctx) {
        Ok(v) => v,
        Err(code) => return code,
    };
    let numero = match normalizar_numero(&pedido.numero) {
        Ok(n) => n,
        Err(e) => {
            eprintln!("{}", e.mensagem(ctx.lang));
            return EX_DATAERR;
        }
    };
    if !config.execution.perfil().is_isolated_pod() {
        eprintln!("{}", recusa_de_dono_fora_do_pod(ctx.lang));
        return EX_USAGE;
    }
    if !pedido.yes {
        if !ctx.interactive {
            eprintln!(
                "{}",
                t(
                    ctx.lang,
                    "`whatsapp owner` sem terminal precisa de `--yes`: tornar alguém dono é uma decisão explícita.",
                    "`whatsapp owner` without a terminal needs `--yes`: making someone an owner is an explicit decision.",
                )
            );
            return EX_USAGE;
        }
        match prompter.confirm(pergunta_de_dono(ctx.lang), false) {
            Ok(true) => {}
            Ok(false) | Err(_) => {
                println!("{}", t(ctx.lang, "Cancelado.", "Cancelled."));
                return EX_CANCELLED;
            }
        }
    }

    let (promovido, depois) = match promover(loader, &numero) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("{e}");
            return EX_SOFTWARE;
        }
    };
    println!("{}", linha_de_promovido(ctx.lang, &numero, promovido));
    if matches!(promovido, Promovido::Novo { .. }) {
        println!("{}", nota_do_poder_de_dono(ctx.lang));
    }
    if depois.enabled {
        println!("{}", dica_do_gateway(ctx.lang, true, ctx.gateway_pid));
    } else {
        println!(
            "{}",
            tb(
                ctx.lang,
                "O canal ainda não está ligado: vincule o WhatsApp com `{bin} whatsapp link`.",
                "The channel is not on yet: link WhatsApp with `{bin} whatsapp link`.",
            )
        );
    }
    0
}

/// `garraia whatsapp unowner`. Tira o papel de dono, **nunca** o acesso.
///
/// Sem porta de perfil, de proposito: rebaixar precisa funcionar em `standard`
/// tambem. E justamente la que um `owners` esquecido e um privilegio latente,
/// esperando o dia em que o perfil mude — recusar a limpeza fora do pod
/// deixaria o operador sem o comando exatamente onde ele mais importa.
///
/// A confirmacao explicita — `--yes` num pipe (64 sem ele), pergunta com
/// default NAO no terminal — vale para o **ultimo** dono, nao para todo
/// rebaixamento. O precedente do `remove` (#1394) e "nada que mexa com dono
/// acontece em silencio", mas la a operacao tira o acesso; aqui ela o
/// preserva, e com outro dono na lista nada fica invalido. O que merece uma
/// parada e o estado que a issue nomeia: a configuracao ficar sem dono nenhum.
pub fn unowner(ctx: &Context, prompter: &dyn Prompter, pedido: &PedidoDePapel) -> i32 {
    let (loader, config) = match carregar(ctx) {
        Ok(v) => v,
        Err(code) => return code,
    };
    let numero = match normalizar_numero(&pedido.numero) {
        Ok(n) => n,
        Err(e) => {
            eprintln!("{}", e.mensagem(ctx.lang));
            return EX_DATAERR;
        }
    };

    // Mesma janela documentada no `remove`: o papel sai desta leitura e o
    // `rebaixar` rele o arquivo antes de gravar. Numa CLI de um usuario so, a
    // alternativa seria segurar o arquivo aberto durante o prompt.
    let antes = acesso_da_config(&config);
    let ultimo_dono = antes.donos == 1 && e_dono_na_config(&config, &numero);
    if ultimo_dono && !pedido.yes {
        if !ctx.interactive {
            eprintln!(
                "{}",
                t(
                    ctx.lang,
                    "Este é o ÚNICO dono: rebaixá-lo sem terminal precisa de `--yes`. O acesso dele é preservado, mas a configuração fica sem dono nenhum.",
                    "This is the ONLY owner: demoting it without a terminal needs `--yes`. Its access is preserved, but the configuration is left with no owner at all.",
                )
            );
            return EX_USAGE;
        }
        match prompter.confirm(pergunta_de_ultimo_dono(ctx.lang), false) {
            Ok(true) => {}
            Ok(false) | Err(_) => {
                println!("{}", t(ctx.lang, "Cancelado.", "Cancelled."));
                return EX_CANCELLED;
            }
        }
    }

    let (rebaixado, depois) = match rebaixar(loader, &numero) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("{e}");
            return EX_SOFTWARE;
        }
    };
    println!("{}", linha_de_rebaixado(ctx.lang, &numero, rebaixado));
    if !matches!(rebaixado, Rebaixado::Feito { .. }) {
        // Nada mudou no disco: mandar reiniciar (ou explicar o hot reload)
        // logo depois de "nada mudou" so sugeriria que havia o que aplicar.
        // Mesma saida do `remove` quando o numero nao estava na lista.
        return 0;
    }
    if depois.donos == 0 {
        println!(
            "{}",
            aviso_sem_dono(ctx.lang, config.execution.perfil().is_isolated_pod())
        );
    }
    if depois.enabled {
        println!("{}", dica_do_gateway(ctx.lang, true, ctx.gateway_pid));
    }
    0
}

/// O aviso de `owners` vazio. Nao e um erro: e o estado default do canal.
///
/// O texto depende do perfil, e tem de depender. Em `standard` ninguem tinha
/// o piso `code` para perder — dizer "ninguem recebe mais" ali descreveria uma
/// consequencia que nao existe —, e pior: mandar promover alguem seria mandar
/// rodar um comando que naquele perfil sai 64. O aviso do pod e o unico que
/// aponta o `owner`.
pub fn aviso_sem_dono(lang: Lang, pod: bool) -> String {
    if pod {
        return tb(
            lang,
            "Não há mais nenhum dono: ninguém recebe o piso `code` em conversa 1:1. Para promover alguém: `{bin} whatsapp owner <número>`.",
            "There is no owner left: nobody gets the `code` floor in 1:1 chats. To promote someone: `{bin} whatsapp owner <number>`.",
        );
    }
    t(
        lang,
        "Não há mais nenhum dono. Neste perfil (`standard`) isso não muda nada: dono só tem efeito em `isolated-pod`.",
        "There is no owner left. In this profile (`standard`) that changes nothing: owners only have an effect in `isolated-pod`.",
    )
    .to_string()
}

/// A linha do desfecho do `owner`. Pura, e so com o final do numero.
pub fn linha_de_promovido(lang: Lang, numero: &str, promovido: Promovido) -> String {
    let fim = final4(numero);
    let tipo = tipo_da_identidade(lang, numero);
    match (lang, promovido) {
        (Lang::Pt, Promovido::JaEra) => format!("✓ O {tipo} terminado em {fim} já era DONO."),
        (Lang::Pt, Promovido::Novo { ganhou_acesso }) => {
            let linha = format!("✓ {tipo} terminado em {fim} promovido a DONO.");
            if ganhou_acesso {
                format!(
                    "{linha} Ele não estava autorizado antes — agora também pode falar com o GarraIA."
                )
            } else {
                linha
            }
        }
        (Lang::En, Promovido::JaEra) => {
            format!("✓ The {tipo} ending in {fim} was already an OWNER.")
        }
        (Lang::En, Promovido::Novo { ganhou_acesso }) => {
            let linha = format!("✓ {tipo} ending in {fim} promoted to OWNER.");
            if ganhou_acesso {
                format!("{linha} It was not authorized before — it can talk to GarraIA now too.")
            } else {
                linha
            }
        }
    }
}

/// A linha do desfecho do `unowner`. Pura, e so com o final do numero.
///
/// Diz **sempre** que o acesso continua: e a diferenca que o operador precisa
/// ver para nao rodar um `remove` achando que rebaixar nao bastou.
pub fn linha_de_rebaixado(lang: Lang, numero: &str, rebaixado: Rebaixado) -> String {
    let fim = final4(numero);
    let tipo = tipo_da_identidade(lang, numero);
    match (lang, rebaixado) {
        (Lang::Pt, Rebaixado::NaoEra) => {
            format!("O {tipo} terminado em {fim} não era DONO — nada mudou.")
        }
        (Lang::Pt, Rebaixado::Feito { movido_para_allow }) => {
            let linha = format!("✓ {tipo} terminado em {fim} não é mais DONO.");
            if movido_para_allow {
                format!("{linha} Ele continua autorizado: a entrada passou para `allow`.")
            } else {
                format!("{linha} Ele continua autorizado em `allow`.")
            }
        }
        (Lang::En, Rebaixado::NaoEra) => {
            format!("The {tipo} ending in {fim} was not an OWNER — nothing changed.")
        }
        (Lang::En, Rebaixado::Feito { movido_para_allow }) => {
            let linha = format!("✓ {tipo} ending in {fim} is no longer an OWNER.");
            if movido_para_allow {
                format!("{linha} It stays authorized: the entry moved to `allow`.")
            } else {
                format!("{linha} It stays authorized in `allow`.")
            }
        }
    }
}

// ---------------------------------------------------------------------------
// O passo depois do `link`
// ---------------------------------------------------------------------------

/// Valida as pre-respostas do `link` ANTES do QR: numero invalido e
/// `--owner` fora do pod falham cedo, sem gastar um pareamento.
pub fn validar_pre_link(ctx: &Context, pre: &Pedido) -> Result<(), i32> {
    if pre.numero.is_none() && !pre.owner {
        return Ok(());
    }
    let (_, config) = carregar(ctx)?;
    validar(ctx, pre, &config).map(|_| ())
}

/// O desfecho do passo pos-link, para o chamador decidir o que imprimir.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PosLink {
    /// Quantos autorizados ha depois do passo.
    pub autorizados: usize,
}

/// Depois de a sessao estar salva e o canal ligado: garante que alguem
/// pode falar com o GarraIA, ou diz claramente que ninguem pode.
///
/// So roda dentro do `link`, que ja e interativo. Nunca remove nada.
///
/// - Portao vazio: pergunta o numero (codigo do pais obrigatorio). Resposta
///   vazia = ninguem por enquanto.
/// - Portao com gente (upgrade, re-vinculo): mostra as contagens e pergunta
///   se quer adicionar outro, default **nao**.
/// - Dono so e oferecido em `isolated-pod`, com confirmacao default nao.
/// - Numero cujo final bate com o do celular vinculado: avisa do `from_me` e
///   pede confirmacao, default nao.
pub fn pos_link(
    ctx: &Context,
    prompter: &dyn Prompter,
    phone_last4: Option<&str>,
    pre: &Pedido,
) -> Result<PosLink, i32> {
    let (loader, config) = carregar(ctx)?;
    let pod = config.execution.perfil().is_isolated_pod();
    let antes = acesso_da_config(&config);

    let quer_adicionar = if pre.numero.is_some() || antes.autorizados == 0 {
        true
    } else {
        println!(
            "{}",
            match ctx.lang {
                Lang::Pt => format!(
                    "Autorizados: {} · Donos: {}",
                    antes.autorizados, antes.donos
                ),
                Lang::En => format!(
                    "Authorized: {} · Owners: {}",
                    antes.autorizados, antes.donos
                ),
            }
        );
        prompter
            .confirm(
                t(
                    ctx.lang,
                    "Adicionar outro número autorizado?",
                    "Add another authorized number?",
                ),
                false,
            )
            .unwrap_or(false)
    };

    if quer_adicionar
        && let Some(numero) = obter_numero(ctx, prompter, phone_last4, pre, antes.autorizados == 0)
    {
        let papel = if pod
            && (pre.owner
                || prompter
                    .confirm(pergunta_de_dono(ctx.lang), false)
                    .unwrap_or(false))
        {
            Papel::Dono
        } else {
            Papel::Autorizado
        };
        match autorizar(loader, &numero, papel) {
            Ok(gravado) => imprimir_gravado(ctx.lang, &numero, papel, gravado),
            Err(e) => {
                eprintln!("{e}");
                return Err(EX_SOFTWARE);
            }
        }
    }

    let depois = match loader.load_sem_env() {
        Ok(c) => acesso_da_config(&c),
        Err(e) => {
            eprintln!("{e}");
            return Err(EX_SOFTWARE);
        }
    };
    Ok(PosLink {
        autorizados: depois.autorizados,
    })
}

/// O numero a autorizar: o pre-respondido, ou perguntado ate [`TENTATIVAS`]
/// vezes. `None` = ninguem por enquanto.
fn obter_numero(
    ctx: &Context,
    prompter: &dyn Prompter,
    phone_last4: Option<&str>,
    pre: &Pedido,
    portao_vazio: bool,
) -> Option<String> {
    if let Some(raw) = pre.numero.as_deref() {
        // Ja validado por `validar_pre_link`; normaliza de novo so para nao
        // carregar o `Result` pela pilha.
        let numero = normalizar_numero(raw).ok()?;
        return confirmar_se_proprio(ctx, prompter, phone_last4, numero);
    }
    if portao_vazio {
        println!();
        println!(
            "{}",
            t(
                ctx.lang,
                "Quem pode falar com o GarraIA por este WhatsApp? Ninguém, até você autorizar.",
                "Who may talk to GarraIA through this WhatsApp? Nobody, until you authorize someone.",
            )
        );
    }
    let prompt = t(
        ctx.lang,
        "Número autorizado, com código do país (ex.: +55 11 99999-8888; vazio = ninguém por enquanto)",
        "Authorized number, with the country code (e.g. +1 555 123 4567; empty = nobody for now)",
    );
    for _ in 0..TENTATIVAS {
        let resposta = match prompter.input(prompt, "") {
            Ok(r) => r,
            Err(_) => return None,
        };
        if resposta.trim().is_empty() {
            return None;
        }
        match normalizar_numero(&resposta) {
            Ok(numero) => {
                if let Some(n) = confirmar_se_proprio(ctx, prompter, phone_last4, numero) {
                    return Some(n);
                }
            }
            Err(e) => println!("{}", e.mensagem(ctx.lang)),
        }
    }
    None
}

/// Avisa quando o numero parece ser o do celular vinculado (`from_me`).
fn confirmar_se_proprio(
    ctx: &Context,
    prompter: &dyn Prompter,
    phone_last4: Option<&str>,
    numero: String,
) -> Option<String> {
    // Um LID nao e o numero do celular: o final dele nao diz nada sobre o
    // `from_me`.
    if !e_lid(&numero) && phone_last4.is_some_and(|p| p == final4(&numero)) {
        println!("{}", aviso_do_proprio_numero(ctx.lang));
        let usar = prompter
            .confirm(
                t(
                    ctx.lang,
                    "Autorizar este número mesmo assim?",
                    "Authorize this number anyway?",
                ),
                false,
            )
            .unwrap_or(false);
        if !usar {
            return None;
        }
    }
    Some(numero)
}
