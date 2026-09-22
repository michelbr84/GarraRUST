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
}

/// O desfecho de [`autorizar`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Gravado {
    Novo,
    JaEstava,
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
/// Nunca remove: revogar e editar o arquivo (a documentacao diz como, e o
/// gateway relê a quente).
pub fn autorizar(loader: &ConfigLoader, numero: &str, papel: Papel) -> Result<Gravado> {
    loader.ensure_dirs()?;
    // Sem a env do perfil: ela nao vai ao disco (`#[serde(skip)]`) e nao
    // decide nada aqui — so o `--owner` depende dela, e ja foi validado.
    let mut config = loader.load_sem_env()?;
    let secao = config
        .channels
        .entry(CONFIG_KEY.to_string())
        .or_insert_with(|| ChannelConfig {
            channel_type: CONFIG_KEY.to_string(),
            enabled: None,
            settings: Default::default(),
        });
    if secao.channel_type != CONFIG_KEY {
        // Nao e este canal: o gateway ignora a secao inteira, e escrever nela
        // seria dizer "autorizado" para algo que nao vale. Trocar o `type` de
        // uma secao do operador tambem nao e decisao deste comando.
        bail!(
            "`channels.{CONFIG_KEY}` existe com `type: {}` — corrija para `type: {CONFIG_KEY}` no config.yml",
            secao.channel_type
        );
    }
    let chave = papel.chave();
    let lista = secao
        .settings
        .entry(chave.to_string())
        .or_insert_with(|| serde_json::Value::Array(Vec::new()));
    let Some(itens) = lista.as_array_mut() else {
        bail!("`channels.{CONFIG_KEY}.{chave}` nao e uma lista — corrija o config.yml");
    };
    // A mesma chave que o portao compara: `+55 31 99999-8888` ja cobre
    // `553199998888` (o nono digito, ver `whatsapp_linked_chave_do_portao`).
    let chave = |v: &str| {
        garraia_gateway::bootstrap::whatsapp_linked_chave_do_portao(
            &garraia_gateway::bootstrap::whatsapp_linked_normalizar_identidade(v),
        )
    };
    let alvo = chave(numero);
    let ja_estava = itens
        .iter()
        .filter_map(|v| v.as_str())
        .any(|v| chave(v) == alvo);
    if ja_estava {
        return Ok(Gravado::JaEstava);
    }
    itens.push(serde_json::Value::String(numero.to_string()));
    loader.save(&config)?;
    Ok(Gravado::Novo)
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
