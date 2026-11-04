//! "Quem responde": o passo de provedor de LLM do Assistente de Primeiro Uso
//! (#1700) e a deteccao de um gateway que nao e deste app (#1816).
//!
//! Puro onde da (validacao, argumentos do sidecar, leitura do envelope do
//! teste) e com um unico pedaco de I/O — [`get_loopback`] — restrito a
//! `127.0.0.1`, para a casca Tauri nao precisar de cliente HTTP so para
//! perguntar ao Ollama local e ao proprio gateway se eles estao de pe.
//!
//! Segredo: a chave do provedor so passa por [`Escolha::chave`] para a casca a
//! escrever no **stdin** do `garraia config set-routing --api-key-stdin`. Ela
//! nunca entra em argumento de processo (`/proc/<pid>/cmdline`), em mensagem
//! de erro, em `Debug` nem em nada que volte para a webview.

use std::fmt;
use std::io::{Read, Write};
use std::net::{Ipv4Addr, SocketAddr, TcpStream};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use crate::config_alheia::{Liberado, Passo};
pub use zeroize::Zeroizing;

/// Porta padrao do Ollama local.
pub const OLLAMA_PORTA: u16 = 11434;
/// Porta padrao do gateway (`garraia start` sem `--port`/`PORT`).
pub const GATEWAY_PORTA: u16 = 3888;
/// Teto do corpo lido de um servico local: o suficiente para a lista de
/// modelos do Ollama e o `/api/health`, nunca uma resposta arbitraria.
const CORPO_MAX: usize = 1024 * 1024;
/// Teto da chave colada (OpenRouter usa ~73 caracteres).
const CHAVE_MAX: usize = 512;
/// Teto do nome de modelo.
const MODELO_MAX: usize = 200;

/// O pedido do teste de conectividade: curto, barato e com resposta previsivel.
pub const PERGUNTA_DE_TESTE: &str = "Responda apenas com a palavra: ok";
/// Prazo do teste, em segundos (o `ask` sai com 124 quando estoura).
pub const PRAZO_DO_TESTE_SECS: u64 = 45;

/// Os provedores que o assistente sabe configurar sem terminal.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Provedor {
    /// Nuvem, com chave (`llm.openrouter`).
    Openrouter,
    /// Modelo local em `127.0.0.1:11434`, sem chave (`llm.ollama`).
    Ollama,
}

impl Provedor {
    /// O tipo de provider que o `garraia config set-routing` entende — e a
    /// chave do `llm:` que ele grava.
    pub fn como_cli(self) -> &'static str {
        match self {
            Provedor::Openrouter => "openrouter",
            Provedor::Ollama => "ollama",
        }
    }

    pub fn precisa_de_chave(self) -> bool {
        matches!(self, Provedor::Openrouter)
    }
}

/// Uma escolha validada. `Debug` manual: a chave nunca aparece.
#[derive(Clone, PartialEq, Eq)]
pub struct Escolha {
    pub provedor: Provedor,
    pub modelo: String,
    /// Zerada na memoria quando a escolha sai de escopo.
    chave: Option<Zeroizing<String>>,
}

impl fmt::Debug for Escolha {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Escolha")
            .field("provedor", &self.provedor)
            .field("modelo", &self.modelo)
            .field("tem_chave", &self.chave.is_some())
            .finish()
    }
}

impl Escolha {
    /// A chave, para a casca escrever no stdin do sidecar — e so para isso.
    pub fn chave(&self) -> Option<&str> {
        self.chave.as_ref().map(|c| c.as_str())
    }

    /// Os argumentos do `garraia` que gravam esta escolha. A chave, quando ha,
    /// vai pelo stdin (`--api-key-stdin`), nunca aqui. Exige a prova da guarda
    /// da #1824 (com um gateway de fora, o config e o dele).
    pub fn args_set_routing(&self, liberado: &Liberado) -> Vec<String> {
        debug_assert_eq!(liberado.passo(), Passo::Provedor);
        let mut args = vec![
            "config".to_string(),
            "set-routing".to_string(),
            "--primary-provider".to_string(),
            self.provedor.como_cli().to_string(),
            "--primary-model".to_string(),
            self.modelo.clone(),
        ];
        if self.chave.is_some() {
            args.push("--api-key-stdin".to_string());
        }
        args
    }

    /// O que a casca escreve no stdin: a chave e uma quebra de linha (o
    /// `set-routing` le a primeira linha).
    pub fn stdin(&self) -> Option<Zeroizing<Vec<u8>>> {
        self.chave.as_ref().map(|c| {
            let mut bytes = Zeroizing::new(Vec::with_capacity(c.len() + 1));
            bytes.extend_from_slice(c.as_bytes());
            bytes.push(b'\n');
            bytes
        })
    }
}

/// Valida o que veio da tela. Mensagens de erro em pt-BR, prontas para a
/// webview — e **nunca** com a chave dentro.
pub fn validar_escolha(
    provedor: Provedor,
    modelo: &str,
    chave: Option<&str>,
) -> Result<Escolha, String> {
    let modelo = modelo.trim();
    if modelo.is_empty() {
        return Err("Escolha um modelo.".to_string());
    }
    // Vai como argumento do sidecar: nada de comecar com `-` (pareceria uma
    // flag) e so o alfabeto de nomes de modelo (ollama `a:b`, openrouter
    // `org/modelo`, sufixos `@`, `+`).
    if modelo.len() > MODELO_MAX
        || modelo.starts_with('-')
        || !modelo
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "._:/@+-".contains(c))
    {
        return Err(
            "Nome de modelo invalido: use so letras, numeros e . _ : / @ + - (ate 200 caracteres)."
                .to_string(),
        );
    }
    let chave = chave.map(str::trim).filter(|c| !c.is_empty());
    let chave = match (provedor.precisa_de_chave(), chave) {
        (true, None) => {
            return Err("Cole a chave do OpenRouter (comeca com sk-or-).".to_string());
        }
        (true, Some(c)) => {
            if c.len() > CHAVE_MAX || c.chars().any(|ch| ch.is_control() || ch.is_whitespace()) {
                return Err(
                    "A chave colada nao parece valida: ela nao tem espacos nem quebras de linha."
                        .to_string(),
                );
            }
            Some(Zeroizing::new(c.to_string()))
        }
        // Ollama local nao usa chave: uma colada por engano e descartada,
        // nunca gravada.
        (false, _) => None,
    };
    Ok(Escolha {
        provedor,
        modelo: modelo.to_string(),
        chave,
    })
}

/// Os argumentos do teste de conectividade: o `ask` de verdade, com o provider
/// que o `set-routing` acabou de tornar padrao, em JSON (`garra.ask.v1`).
pub fn args_teste() -> Vec<String> {
    vec![
        "ask".to_string(),
        "--json".to_string(),
        "--timeout-secs".to_string(),
        PRAZO_DO_TESTE_SECS.to_string(),
        PERGUNTA_DE_TESTE.to_string(),
    ]
}

/// O resultado do teste, como a webview o mostra. Nunca leva a resposta do
/// modelo nem a chave — so se conversou, quanto demorou e, se nao, por que.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TesteProvedor {
    pub ok: bool,
    pub latencia_ms: Option<u64>,
    pub provedor: Option<String>,
    pub modelo: Option<String>,
    /// O que deu errado, em uma frase (ja sem chave).
    pub motivo: Option<String>,
    /// O que a pessoa pode fazer a respeito.
    pub dica: Option<String>,
}

impl TesteProvedor {
    fn falha(motivo: impl Into<String>, dica: impl Into<String>) -> Self {
        Self {
            ok: false,
            latencia_ms: None,
            provedor: None,
            modelo: None,
            motivo: Some(redigir(&motivo.into())),
            dica: Some(dica.into()),
        }
    }
}

/// Mascara qualquer coisa com cara de chave (`sk-...`, `Bearer ...`) — defesa
/// em profundidade: o `ask` ja redige, mas o motivo vem de fora deste crate.
pub fn redigir(texto: &str) -> String {
    let mut saida = Vec::new();
    let mut proxima_e_segredo = false;
    for palavra in texto.split(' ') {
        let limpa = palavra.trim_matches(|c: char| !c.is_alphanumeric() && c != '-' && c != '_');
        if proxima_e_segredo
            || limpa.starts_with("sk-")
            || (limpa.len() >= 32 && !limpa.contains('/'))
        {
            saida.push("[redigido]".to_string());
            proxima_e_segredo = false;
            continue;
        }
        proxima_e_segredo = limpa.eq_ignore_ascii_case("bearer");
        saida.push(palavra.to_string());
    }
    saida.join(" ")
}

/// Le a saida do `garraia ask --json` e o codigo de saida.
pub fn interpretar_teste(stdout: &str, code: Option<i32>) -> TesteProvedor {
    let envelope = stdout
        .lines()
        .rev()
        .filter_map(|l| serde_json::from_str::<serde_json::Value>(l.trim()).ok())
        .find(|v| v.get("schema").and_then(|s| s.as_str()) == Some("garra.ask.v1"));
    let Some(env) = envelope else {
        return if code == Some(124) {
            TesteProvedor::falha(
                "o modelo nao respondeu dentro do prazo",
                "Confira a conexao; num modelo local, o primeiro uso pode demorar para carregar — tente de novo.",
            )
        } else {
            TesteProvedor::falha(
                format!("o teste nao rodou (codigo {code:?})"),
                "Feche e abra o Garra e tente de novo; se repetir, rode `garraia doctor`.",
            )
        };
    };
    if env.get("ok").and_then(|v| v.as_bool()) == Some(true) {
        return TesteProvedor {
            ok: true,
            latencia_ms: env.get("latency_ms").and_then(|v| v.as_u64()),
            provedor: env
                .get("provider")
                .and_then(|v| v.as_str())
                .map(str::to_string),
            modelo: env
                .get("model")
                .and_then(|v| v.as_str())
                .map(str::to_string),
            motivo: None,
            dica: None,
        };
    }
    let erro = env.get("error");
    let tipo = erro
        .and_then(|e| e.get("kind"))
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let mensagem = erro
        .and_then(|e| e.get("message"))
        .and_then(|v| v.as_str())
        .unwrap_or("erro sem descricao");
    let baixa = mensagem.to_lowercase();
    let dica = match tipo {
        "timeout" => {
            "O modelo demorou demais. Confira a internet (OpenRouter) ou se o Ollama terminou de carregar, e teste de novo."
        }
        "no_provider" => "Nenhum provedor ficou configurado. Escolha um acima e salve de novo.",
        _ if baixa.contains("401")
            || baixa.contains("403")
            || baixa.contains("unauthorized")
            || baixa.contains("invalid api key")
            || baixa.contains("no auth") =>
        {
            "O provedor recusou a chave. Copie de novo em openrouter.ai/keys e cole aqui."
        }
        _ if baixa.contains("402")
            || baixa.contains("credit")
            || baixa.contains("insufficient") =>
        {
            "A conta do provedor esta sem credito. Adicione credito ou escolha um modelo gratuito."
        }
        _ if baixa.contains("connection refused")
            || (baixa.contains("connect") && baixa.contains("11434")) =>
        {
            "O Ollama nao esta rodando. Abra o Ollama (ou rode `ollama serve`) e teste de novo."
        }
        _ if baixa.contains("not found")
            || baixa.contains("404")
            || baixa.contains("no endpoints") =>
        {
            "Esse modelo nao existe nesse provedor. Confira o nome (ex.: z-ai/glm-5.3-flash) ou escolha outro."
        }
        _ => {
            "Confira a chave, o modelo e a internet, e teste de novo. O `garraia doctor` mostra mais detalhes."
        }
    };
    TesteProvedor::falha(mensagem, dica)
}

/// `GET http://127.0.0.1:<porta><caminho>` com prazo total. Devolve o status e
/// o corpo, ou `None` quando nao ha ninguem ouvindo (ou ele nao fala HTTP).
/// So loopback, por construcao: nao ha parametro de host.
pub fn get_loopback(porta: u16, caminho: &str, prazo: Duration) -> Option<(u16, String)> {
    // Prazo TOTAL (conexao + escrita + leitura): um servico que pinga um byte
    // por vez nao segura quem pergunta alem dele.
    let fim = Instant::now() + prazo;
    let restante = || {
        fim.checked_duration_since(Instant::now())
            .filter(|d| !d.is_zero())
    };
    let endereco = SocketAddr::from((Ipv4Addr::LOCALHOST, porta));
    let mut conexao = TcpStream::connect_timeout(&endereco, restante()?).ok()?;
    conexao.set_write_timeout(Some(restante()?)).ok()?;
    let pedido = format!(
        "GET {caminho} HTTP/1.0\r\nHost: 127.0.0.1:{porta}\r\nAccept: application/json\r\nConnection: close\r\n\r\n"
    );
    conexao.write_all(pedido.as_bytes()).ok()?;
    let teto = CORPO_MAX + 16 * 1024;
    let mut bruto = Vec::new();
    let mut pedaco = [0u8; 8192];
    loop {
        conexao.set_read_timeout(Some(restante()?)).ok()?;
        match conexao.read(&mut pedaco) {
            Ok(0) => break,
            Ok(n) => {
                bruto.extend_from_slice(&pedaco[..n]);
                if bruto.len() > teto {
                    return None;
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(_) => return None,
        }
    }
    let texto = String::from_utf8_lossy(&bruto);
    let (cabecalho, corpo) = texto.split_once("\r\n\r\n")?;
    let status = cabecalho
        .lines()
        .next()?
        .split_whitespace()
        .nth(1)?
        .parse::<u16>()
        .ok()?;
    Some((status, corpo.to_string()))
}

/// Os modelos que o Ollama local tem instalados (`GET /api/tags`).
pub fn modelos_do_ollama(corpo: &str) -> Vec<String> {
    let Ok(v) = serde_json::from_str::<serde_json::Value>(corpo) else {
        return Vec::new();
    };
    v.get("models")
        .and_then(|m| m.as_array())
        .map(|lista| {
            lista
                .iter()
                .filter_map(|m| m.get("name").and_then(|n| n.as_str()))
                .filter(|n| !n.is_empty() && !n.contains("embed"))
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

/// Pergunta ao Ollama local que modelos ele tem. `None`: nao ha Ollama.
pub fn detectar_ollama(prazo: Duration) -> Option<Vec<String>> {
    let (status, corpo) = get_loopback(OLLAMA_PORTA, "/api/tags", prazo)?;
    (status == 200).then(|| modelos_do_ollama(&corpo))
}

/// A versao de um gateway GarraIA, lida do corpo do `/api/health`. `None`
/// quando o corpo nao e o health de um gateway GarraIA.
pub fn versao_do_gateway(corpo: &str) -> Option<String> {
    health_do_gateway(corpo).map(|h| h.versao)
}

/// O que interessa do `/api/health` de um gateway GarraIA.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HealthDoGateway {
    pub versao: String,
    /// O provedor de LLM que o gateway registrou; `None` = ele nao conversa.
    pub provedor: Option<String>,
}

/// Le o `/api/health`. A versao so vale no alfabeto de versao (ela vai para
/// a tela e para o terminal: nada de sequencia de escape vinda de um servico
/// qualquer na porta).
pub fn health_do_gateway(corpo: &str) -> Option<HealthDoGateway> {
    let v: serde_json::Value = serde_json::from_str(corpo).ok()?;
    v.get("gateway_url")?;
    let versao = v.get("version")?.as_str()?;
    let valida = !versao.is_empty()
        && versao.len() <= 32
        && versao
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || ".+-".contains(c));
    if !valida {
        return None;
    }
    let provedor = v
        .get("provider")
        .and_then(|p| p.as_str())
        .filter(|p| !p.is_empty() && p.len() <= 64)
        .map(str::to_string);
    Some(HealthDoGateway {
        versao: versao.to_string(),
        provedor,
    })
}

/// O health de um gateway GarraIA na porta, se houver um.
pub fn health_na_porta(porta: u16, prazo: Duration) -> Option<HealthDoGateway> {
    let (status, corpo) = get_loopback(porta, "/api/health", prazo)?;
    if status != 200 {
        return None;
    }
    health_do_gateway(&corpo)
}

/// Ha um gateway GarraIA ouvindo na porta? Devolve a versao dele (#1816).
pub fn gateway_na_porta(porta: u16, prazo: Duration) -> Option<String> {
    health_na_porta(porta, prazo).map(|h| h.versao)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;

    #[test]
    fn openrouter_exige_chave_e_ela_vai_pelo_stdin() {
        assert!(validar_escolha(Provedor::Openrouter, "z-ai/glm-5.3-flash", None).is_err());
        assert!(validar_escolha(Provedor::Openrouter, "z-ai/glm-5.3-flash", Some("   ")).is_err());
        let e = validar_escolha(
            Provedor::Openrouter,
            " z-ai/glm-5.3-flash ",
            Some(" sk-or-v1-abc "),
        )
        .unwrap();
        let args = e.args_set_routing(&Liberado::para_teste(Passo::Provedor));
        assert_eq!(
            args,
            [
                "config",
                "set-routing",
                "--primary-provider",
                "openrouter",
                "--primary-model",
                "z-ai/glm-5.3-flash",
                "--api-key-stdin"
            ]
        );
        assert!(
            !args.iter().any(|a| a.contains("sk-or")),
            "a chave nunca entra em argv"
        );
        assert_eq!(e.stdin().unwrap().as_slice(), b"sk-or-v1-abc\n");
        assert!(!format!("{e:?}").contains("sk-or"), "nem no Debug");
    }

    #[test]
    fn ollama_nao_grava_chave_nem_por_engano() {
        let e = validar_escolha(Provedor::Ollama, "qwen3.5:2b", Some("sk-colada-errado")).unwrap();
        assert_eq!(e.chave(), None);
        assert_eq!(e.stdin(), None);
        assert!(
            !e.args_set_routing(&Liberado::para_teste(Passo::Provedor))
                .contains(&"--api-key-stdin".to_string())
        );
    }

    #[test]
    fn modelo_e_chave_invalidos_sao_recusados_sem_ecoar_a_chave() {
        assert!(validar_escolha(Provedor::Ollama, "", None).is_err());
        assert!(validar_escolha(Provedor::Ollama, "nome com espaco", None).is_err());
        assert!(validar_escolha(Provedor::Ollama, &"x".repeat(201), None).is_err());
        let erro =
            validar_escolha(Provedor::Openrouter, "m", Some("sk-or-v1 com espaco")).unwrap_err();
        assert!(!erro.contains("sk-or"), "{erro}");
    }

    #[test]
    fn teste_ok_le_latencia_e_nao_devolve_a_resposta() {
        let out = r#"{"schema":"garra.ask.v1","ok":true,"answer":"ok","provider":"openrouter","model":"z-ai/glm-5.3-flash","latency_ms":812}"#;
        let t = interpretar_teste(&format!("aviso qualquer\n{out}\n"), Some(0));
        assert!(t.ok);
        assert_eq!(t.latencia_ms, Some(812));
        assert_eq!(t.provedor.as_deref(), Some("openrouter"));
        assert!(!serde_json::to_string(&t).unwrap().contains("\"ok\":\"ok\""));
    }

    #[test]
    fn teste_com_erro_vira_motivo_e_dica_acionavel() {
        let env = |kind: &str, msg: &str| {
            format!(
                r#"{{"schema":"garra.ask.v1","ok":false,"error":{{"kind":"{kind}","message":"{msg}"}}}}"#
            )
        };
        let t = interpretar_teste(&env("provider_error", "HTTP 401 Unauthorized"), Some(1));
        assert!(!t.ok);
        assert!(t.dica.unwrap().contains("recusou a chave"));
        let t = interpretar_teste(&env("timeout", "LLM call exceeded 45s timeout"), Some(124));
        assert!(t.dica.unwrap().contains("demorou"));
        let t = interpretar_teste(
            &env(
                "provider_error",
                "error sending request: Connection refused (127.0.0.1:11434)",
            ),
            Some(1),
        );
        assert!(t.dica.unwrap().contains("Ollama nao esta rodando"));
        let t = interpretar_teste(&env("provider_error", "model 'x' not found"), Some(1));
        assert!(t.dica.unwrap().contains("nao existe"));
        let t = interpretar_teste(&env("no_provider", "no provider"), Some(1));
        assert!(t.dica.unwrap().contains("Nenhum provedor"));
    }

    #[test]
    fn sem_envelope_o_codigo_decide() {
        assert!(
            interpretar_teste("", Some(124))
                .motivo
                .unwrap()
                .contains("prazo")
        );
        assert!(
            interpretar_teste("lixo", None)
                .motivo
                .unwrap()
                .contains("nao rodou")
        );
    }

    #[test]
    fn motivo_nunca_carrega_chave() {
        let env = r#"{"schema":"garra.ask.v1","ok":false,"error":{"kind":"provider_error","message":"bad key sk-or-v1-0123456789abcdef0123456789abcdef and Bearer abc123"}}"#;
        let t = interpretar_teste(env, Some(1));
        let m = t.motivo.unwrap();
        assert!(!m.contains("sk-or-v1-0123"), "{m}");
        assert!(!m.contains("abc123"), "{m}");
    }

    /// Revisao de seguranca (#1821): modelo que parece flag ou com caractere
    /// estranho e recusado antes de virar argumento do sidecar.
    #[test]
    fn modelo_com_cara_de_flag_ou_caractere_estranho_e_recusado() {
        for ruim in [
            "-h",
            "--dry-run",
            "--primary-base-url=x",
            "a;b",
            "a$(x)",
            "modelo\u{1b}[31m",
        ] {
            assert!(
                validar_escolha(Provedor::Ollama, ruim, None).is_err(),
                "{ruim}"
            );
        }
        for bom in [
            "qwen3.5:0.8b",
            "z-ai/glm-5.3-flash",
            "hf.co/unsloth/Qwen3.8-27B-GGUF:UD-Q4_K_XL",
            "a@b+c",
        ] {
            assert!(
                validar_escolha(Provedor::Ollama, bom, None).is_ok(),
                "{bom}"
            );
        }
    }

    #[test]
    fn health_le_provedor_e_recusa_versao_com_escape() {
        let ok = r#"{"status":"healthy","version":"0.5.2","gateway_url":"x","provider":"ollama"}"#;
        assert_eq!(
            health_do_gateway(ok),
            Some(HealthDoGateway {
                versao: "0.5.2".into(),
                provedor: Some("ollama".into())
            })
        );
        let sem = r#"{"status":"healthy","version":"0.5.2","gateway_url":"x","provider":null}"#;
        assert_eq!(health_do_gateway(sem).unwrap().provedor, None);
        let escape = r#"{"version":"0.5\u001b[2J","gateway_url":"x"}"#;
        assert_eq!(health_do_gateway(escape), None);
        assert_eq!(
            health_do_gateway(r#"{"version":"","gateway_url":"x"}"#),
            None
        );
    }

    /// Prazo total: um servico que manda um byte por vez nao segura a sonda.
    #[test]
    fn get_loopback_respeita_o_prazo_total() {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let porta = l.local_addr().unwrap().port();
        std::thread::spawn(move || {
            if let Ok((mut s, _)) = l.accept() {
                let mut buf = [0u8; 512];
                let _ = s.read(&mut buf);
                for _ in 0..100 {
                    if s.write_all(b"x").is_err() {
                        break;
                    }
                    std::thread::sleep(Duration::from_millis(100));
                }
            }
        });
        let inicio = Instant::now();
        assert_eq!(get_loopback(porta, "/", Duration::from_millis(600)), None);
        assert!(
            inicio.elapsed() < Duration::from_secs(3),
            "{:?}",
            inicio.elapsed()
        );
    }

    #[test]
    fn modelos_do_ollama_ignora_embedding_e_lixo() {
        let corpo = r#"{"models":[{"name":"qwen3.5:2b"},{"name":"qwen3-embedding:0.6b"},{"name":""},{"x":1}]}"#;
        assert_eq!(modelos_do_ollama(corpo), ["qwen3.5:2b"]);
        assert!(modelos_do_ollama("nao e json").is_empty());
    }

    #[test]
    fn versao_do_gateway_so_para_o_health_de_um_gateway() {
        let health = r#"{"status":"healthy","version":"0.5.1","gateway_url":"http://127.0.0.1:3888","uptime_secs":1}"#;
        assert_eq!(versao_do_gateway(health).as_deref(), Some("0.5.1"));
        assert_eq!(
            versao_do_gateway(r#"{"version":"1"}"#),
            None,
            "outro servico"
        );
        assert_eq!(versao_do_gateway("<html>"), None);
    }

    /// Um servidor HTTP de verdade, de uma resposta, em 127.0.0.1.
    fn servidor_de_uma_resposta(resposta: &'static str) -> u16 {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let porta = l.local_addr().unwrap().port();
        std::thread::spawn(move || {
            if let Ok((mut s, _)) = l.accept() {
                let mut buf = [0u8; 1024];
                let _ = s.read(&mut buf);
                let _ = s.write_all(resposta.as_bytes());
            }
        });
        porta
    }

    #[test]
    fn get_loopback_le_status_e_corpo_de_um_servidor_real() {
        let porta = servidor_de_uma_resposta(
            "HTTP/1.0 200 OK\r\nContent-Type: application/json\r\n\r\n{\"status\":\"healthy\",\"version\":\"0.5.2\",\"gateway_url\":\"x\"}",
        );
        assert_eq!(
            gateway_na_porta(porta, Duration::from_secs(2)).as_deref(),
            Some("0.5.2")
        );
    }

    #[test]
    fn porta_fechada_e_ninguem_ouvindo() {
        // Reserva uma porta e solta: ninguem ouve nela.
        let porta = TcpListener::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap()
            .port();
        assert_eq!(get_loopback(porta, "/", Duration::from_millis(300)), None);
        assert_eq!(gateway_na_porta(porta, Duration::from_millis(300)), None);
    }
}
