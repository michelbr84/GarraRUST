//! Leitor dos eventos SSE da Responses API, traduzidos para os
//! [`StreamEvent`] que o runtime consome.
//!
//! Regras que vem do runtime (`runtime.rs`, laco de streaming):
//! - cada function call precisa de `ToolUseStart` → `InputJsonDelta`* →
//!   `ContentBlockStop`, nessa ordem, **antes** da proxima: o runtime guarda
//!   uma tool por vez e uma nova sobrescreveria a pendente;
//! - `stop_reason` no estilo Anthropic: `tool_use` quando houve chamada,
//!   `end_turn` quando nao.
//!
//! Regras que vem da documentacao (`models-and-inference` §3): so
//! `response.completed` e sucesso; `response.failed`, `response.incomplete`,
//! eventos `error` e stream interrompido sao tratados a parte. Por isso
//! [`LeitorDeEventos::finalizar`] devolve erro se o stream acabou sem
//! evento terminal.
//!
//! O buffer guarda **bytes** e so decodifica blocos completos: um caractere
//! multibyte partido entre dois chunks nunca e corrompido.

use std::collections::{HashMap, VecDeque};

use garraia_common::Error;

use super::PAGINA_DE_USO;
use crate::providers::{StreamEvent, Usage};

/// Namespace das ferramentas locais do GarraIA no pedido (`preview-limitations`:
/// ferramentas de funcao agrupadas em namespaces).
pub(super) const NAMESPACE: &str = "garraia";

#[derive(Debug)]
struct Chamada {
    indice: usize,
    argumentos_enviados: bool,
}

#[derive(Debug, Default)]
pub(super) struct LeitorDeEventos {
    buffer: Vec<u8>,
    pendentes: VecDeque<StreamEvent>,
    chamadas: HashMap<String, Chamada>,
    houve_chamada: bool,
    terminou: bool,
}

impl LeitorDeEventos {
    /// Acrescenta bytes recebidos e processa todos os blocos completos.
    pub(super) fn alimentar(&mut self, bytes: &[u8]) -> Result<(), Error> {
        // SSE termina linha com `\n` ou `\r\n`; o JSON dos eventos escapa
        // controles, entao descartar `\r` nao altera nenhum dado.
        self.buffer
            .extend(bytes.iter().copied().filter(|b| *b != b'\r'));
        while let Some(pos) = self.buffer.windows(2).position(|w| w == b"\n\n") {
            let bloco: Vec<u8> = self.buffer.drain(..pos + 2).collect();
            self.processar_bloco(&String::from_utf8_lossy(&bloco))?;
        }
        Ok(())
    }

    /// Fim do corpo: processa o que sobrou e exige um evento terminal.
    pub(super) fn finalizar(&mut self) -> Result<(), Error> {
        if !self.buffer.is_empty() {
            let resto = std::mem::take(&mut self.buffer);
            self.processar_bloco(&String::from_utf8_lossy(&resto))?;
        }
        if self.terminou {
            Ok(())
        } else {
            Err(Error::Agent(
                "openai-codex: o stream terminou sem response.completed (conexao interrompida); a resposta foi descartada".into(),
            ))
        }
    }

    pub(super) fn proximo(&mut self) -> Option<StreamEvent> {
        self.pendentes.pop_front()
    }

    pub(super) fn terminou(&self) -> bool {
        self.terminou
    }

    fn processar_bloco(&mut self, bloco: &str) -> Result<(), Error> {
        let dados: Vec<&str> = bloco
            .lines()
            .filter_map(|l| l.strip_prefix("data:"))
            .map(|d| d.strip_prefix(' ').unwrap_or(d))
            .collect();
        if dados.is_empty() {
            return Ok(());
        }
        let dado = dados.join("\n");
        if dado.trim() == "[DONE]" {
            return Ok(());
        }
        let Ok(v) = serde_json::from_str::<serde_json::Value>(&dado) else {
            return Ok(());
        };
        self.processar_evento(&v)
    }

    fn processar_evento(&mut self, v: &serde_json::Value) -> Result<(), Error> {
        let tipo = v.get("type").and_then(|t| t.as_str()).unwrap_or_default();
        match tipo {
            "response.output_text.delta" => {
                if let Some(d) = v.get("delta").and_then(|d| d.as_str())
                    && !d.is_empty()
                {
                    self.pendentes
                        .push_back(StreamEvent::TextDelta(d.to_string()));
                }
            }
            "response.output_item.added" => {
                let item = &v["item"];
                if item.get("type").and_then(|t| t.as_str()) == Some("function_call") {
                    let indice =
                        v.get("output_index").and_then(|i| i.as_u64()).unwrap_or(0) as usize;
                    self.anunciar_chamada(item, indice);
                    if let Some(args) = item.get("arguments").and_then(|a| a.as_str())
                        && !args.is_empty()
                    {
                        self.enviar_argumentos(&id_do_item(item), args);
                    }
                }
            }
            "response.function_call_arguments.delta" => {
                let id = v
                    .get("item_id")
                    .and_then(|i| i.as_str())
                    .unwrap_or_default()
                    .to_string();
                if let Some(d) = v.get("delta").and_then(|d| d.as_str()) {
                    self.enviar_argumentos(&id, d);
                }
            }
            "response.function_call_arguments.done" => {
                let id = v
                    .get("item_id")
                    .and_then(|i| i.as_str())
                    .unwrap_or_default()
                    .to_string();
                if let Some(args) = v.get("arguments").and_then(|a| a.as_str())
                    && self
                        .chamadas
                        .get(&id)
                        .is_some_and(|c| !c.argumentos_enviados)
                {
                    self.enviar_argumentos(&id, args);
                }
            }
            "response.output_item.done" => {
                let item = &v["item"];
                if item.get("type").and_then(|t| t.as_str()) == Some("function_call") {
                    let id = id_do_item(item);
                    if !self.chamadas.contains_key(&id) {
                        let indice =
                            v.get("output_index").and_then(|i| i.as_u64()).unwrap_or(0) as usize;
                        self.anunciar_chamada(item, indice);
                    }
                    if self
                        .chamadas
                        .get(&id)
                        .is_some_and(|c| !c.argumentos_enviados)
                    {
                        let args = item
                            .get("arguments")
                            .and_then(|a| a.as_str())
                            .filter(|a| !a.is_empty())
                            .unwrap_or("{}");
                        self.enviar_argumentos(&id, args);
                    }
                    if let Some(c) = self.chamadas.remove(&id) {
                        self.pendentes
                            .push_back(StreamEvent::ContentBlockStop { index: c.indice });
                    }
                }
            }
            "response.completed" => {
                let usage = uso(&v["response"]);
                let stop = if self.houve_chamada {
                    "tool_use"
                } else {
                    "end_turn"
                };
                self.pendentes.push_back(StreamEvent::MessageDelta {
                    stop_reason: Some(stop.into()),
                    usage,
                });
                self.pendentes.push_back(StreamEvent::MessageStop);
                self.terminou = true;
            }
            "response.incomplete" => {
                let usage = uso(&v["response"]);
                self.pendentes.push_back(StreamEvent::MessageDelta {
                    stop_reason: Some("max_tokens".into()),
                    usage,
                });
                self.pendentes.push_back(StreamEvent::MessageStop);
                self.terminou = true;
            }
            "response.failed" => {
                let erro = &v["response"]["error"];
                return Err(erro_estruturado(
                    None,
                    texto(erro, "code"),
                    texto(erro, "message"),
                    texto(erro, "param"),
                ));
            }
            "error" => {
                let erro = if v.get("error").is_some_and(|e| e.is_object()) {
                    &v["error"]
                } else {
                    v
                };
                return Err(erro_estruturado(
                    None,
                    texto(erro, "code"),
                    texto(erro, "message"),
                    texto(erro, "param"),
                ));
            }
            _ => {}
        }
        Ok(())
    }

    fn anunciar_chamada(&mut self, item: &serde_json::Value, indice: usize) {
        let id_item = id_do_item(item);
        let call_id = item
            .get("call_id")
            .and_then(|c| c.as_str())
            .map(str::to_string)
            .unwrap_or_else(|| id_item.clone());
        let nome = nome_local(
            item.get("name")
                .and_then(|n| n.as_str())
                .unwrap_or_default(),
        );
        self.pendentes.push_back(StreamEvent::ToolUseStart {
            index: indice,
            id: call_id,
            name: nome,
        });
        self.chamadas.insert(
            id_item,
            Chamada {
                indice,
                argumentos_enviados: false,
            },
        );
        self.houve_chamada = true;
    }

    fn enviar_argumentos(&mut self, id_item: &str, args: &str) {
        if let Some(c) = self.chamadas.get_mut(id_item) {
            c.argumentos_enviados = true;
            self.pendentes
                .push_back(StreamEvent::InputJsonDelta(args.to_string()));
        }
    }
}

fn id_do_item(item: &serde_json::Value) -> String {
    item.get("id")
        .and_then(|i| i.as_str())
        .or_else(|| item.get("call_id").and_then(|c| c.as_str()))
        .unwrap_or_default()
        .to_string()
}

/// O nome da ferramenta como o GarraIA a registrou: sem o prefixo do
/// namespace, se a API o devolver qualificado (`garraia.file_read`).
pub(super) fn nome_local(nome: &str) -> String {
    nome.strip_prefix(&format!("{NAMESPACE}."))
        .unwrap_or(nome)
        .to_string()
}

fn texto<'a>(v: &'a serde_json::Value, campo: &str) -> &'a str {
    v.get(campo).and_then(|c| c.as_str()).unwrap_or_default()
}

fn uso(resposta: &serde_json::Value) -> Option<Usage> {
    let u = resposta.get("usage")?;
    Some(Usage {
        input_tokens: u.get("input_tokens").and_then(|n| n.as_u64()).unwrap_or(0) as u32,
        output_tokens: u.get("output_tokens").and_then(|n| n.as_u64()).unwrap_or(0) as u32,
    })
}

/// Um erro da Responses API como o runtime precisa ler (`errors-and-recovery`).
///
/// O runtime classifica pelo texto: `status=429`/`status=503` repetem com
/// espera, `status=401/403` vao direto ao fallback como recusa de
/// credencial. Por isso:
/// - limite de uso do plano **nao** leva `status=429` — a documentacao manda
///   pausar, nao repetir — e aponta a pagina de uso;
/// - indisponibilidade temporaria (`*_unavailable`) leva `status=503`, para a
///   repeticao com espera limitada que a documentacao pede;
/// - os demais carregam o status HTTP quando ha um.
pub(super) fn erro_estruturado(
    status: Option<u16>,
    codigo: &str,
    mensagem: &str,
    param: &str,
) -> Error {
    let msg = crate::providers::truncar(mensagem, 200);
    let param = if param.is_empty() {
        String::new()
    } else {
        format!(" (param: {param})")
    };
    match codigo {
        "subscription_sharing_usage_limit_exceeded" => Error::Agent(format!(
            "openai-codex: limite de uso do plano do ChatGPT atingido ({codigo}); novas requisicoes pelo plano ficam pausadas. Veja o uso em {PAGINA_DE_USO}"
        )),
        "subscription_sharing_usage_unavailable" | "subscription_sharing_user_unavailable" => {
            Error::Agent(format!(
                "openai-codex API error: status=503, uso do plano temporariamente indisponivel ({codigo}); tente de novo em instantes"
            ))
        }
        "subscription_sharing_user_not_eligible" => Error::Agent(format!(
            "openai-codex: o uso do plano do ChatGPT nao esta disponivel para esta conta, workspace ou politica ({codigo}): {msg}"
        )),
        "subscription_sharing_unsupported_capability" => Error::Agent(format!(
            "openai-codex: recurso nao suportado pelo uso do plano ({codigo}){param}: {msg}"
        )),
        "subscription_sharing_route_not_supported"
        | "chatpass_v2_scope_not_authorized"
        | "chatpass_v2_invalid_authorization_context" => Error::Agent(format!(
            "openai-codex: a autorizacao do plano nao cobre esta operacao ({codigo}): {msg}"
        )),
        _ => {
            let status = status.map(|s| format!("status={s}, ")).unwrap_or_default();
            let codigo = if codigo.is_empty() {
                String::new()
            } else {
                format!(" (code={codigo})")
            };
            Error::Agent(format!(
                "openai-codex API error: {status}body={msg}{codigo}{param}"
            ))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn eventos(entrada: &[&str]) -> (Vec<StreamEvent>, Result<(), Error>) {
        let mut l = LeitorDeEventos::default();
        let mut fim = Ok(());
        for pedaco in entrada {
            if let Err(e) = l.alimentar(pedaco.as_bytes()) {
                fim = Err(e);
                break;
            }
        }
        if fim.is_ok() {
            fim = l.finalizar();
        }
        let mut v = Vec::new();
        while let Some(e) = l.proximo() {
            v.push(e);
        }
        (v, fim)
    }

    fn sse(tipo: &str, corpo: serde_json::Value) -> String {
        let mut c = corpo;
        c["type"] = tipo.into();
        format!("event: {tipo}\ndata: {c}\n\n")
    }

    #[test]
    fn texto_em_pedacos_ate_o_completed() {
        let a = sse(
            "response.output_text.delta",
            serde_json::json!({"delta": "Olá, "}),
        );
        let b = sse(
            "response.output_text.delta",
            serde_json::json!({"delta": "mundo!"}),
        );
        let c = sse(
            "response.completed",
            serde_json::json!({"response": {"usage": {"input_tokens": 7, "output_tokens": 3}}}),
        );
        let tudo = format!("{a}{b}{c}");
        // Partido no meio do "á" (2 bytes em UTF-8) e com \r\n.
        let tudo = tudo.replace('\n', "\r\n");
        let bytes = tudo.as_bytes();
        let corte = tudo.find('á').expect("tem á") + 1;
        let mut l = LeitorDeEventos::default();
        l.alimentar(&bytes[..corte]).expect("parte 1");
        l.alimentar(&bytes[corte..]).expect("parte 2");
        l.finalizar().expect("terminal");
        let mut v = Vec::new();
        while let Some(e) = l.proximo() {
            v.push(e);
        }
        assert!(matches!(&v[0], StreamEvent::TextDelta(t) if t == "Olá, "));
        assert!(matches!(&v[1], StreamEvent::TextDelta(t) if t == "mundo!"));
        assert!(
            matches!(&v[2], StreamEvent::MessageDelta { stop_reason: Some(s), usage: Some(u) } if s == "end_turn" && u.input_tokens == 7 && u.output_tokens == 3)
        );
        assert!(matches!(v[3], StreamEvent::MessageStop));
    }

    #[test]
    fn duas_chamadas_paralelas_fecham_cada_uma_antes_da_proxima() {
        let item = |id: &str, call: &str, nome: &str| serde_json::json!({"type": "function_call", "id": id, "call_id": call, "name": nome, "arguments": ""});
        let entrada = [
            sse(
                "response.output_item.added",
                serde_json::json!({"output_index": 0, "item": item("fc_1", "call_1", "file_read")}),
            ),
            sse(
                "response.function_call_arguments.delta",
                serde_json::json!({"item_id": "fc_1", "delta": "{\"path\":"}),
            ),
            sse(
                "response.function_call_arguments.delta",
                serde_json::json!({"item_id": "fc_1", "delta": "\"a.txt\"}"}),
            ),
            sse(
                "response.output_item.done",
                serde_json::json!({"output_index": 0, "item": {"type": "function_call", "id": "fc_1", "call_id": "call_1", "name": "file_read", "arguments": "{\"path\":\"a.txt\"}"}}),
            ),
            // A segunda chega com o nome qualificado e sem deltas: os
            // argumentos vem so no `.done`.
            sse(
                "response.output_item.added",
                serde_json::json!({"output_index": 1, "item": item("fc_2", "call_2", "garraia.bash")}),
            ),
            sse(
                "response.function_call_arguments.done",
                serde_json::json!({"item_id": "fc_2", "arguments": "{\"command\":\"ls\"}"}),
            ),
            sse(
                "response.output_item.done",
                serde_json::json!({"output_index": 1, "item": {"type": "function_call", "id": "fc_2", "call_id": "call_2", "name": "garraia.bash", "arguments": "{\"command\":\"ls\"}"}}),
            ),
            sse("response.completed", serde_json::json!({"response": {}})),
        ];
        let entrada: Vec<&str> = entrada.iter().map(String::as_str).collect();
        let (v, fim) = eventos(&entrada);
        fim.expect("terminal");
        let resumo: Vec<String> = v
            .iter()
            .map(|e| match e {
                StreamEvent::ToolUseStart { id, name, index } => {
                    format!("start {index} {id} {name}")
                }
                StreamEvent::InputJsonDelta(d) => format!("args {d}"),
                StreamEvent::ContentBlockStop { index } => format!("stop {index}"),
                StreamEvent::MessageDelta { stop_reason, .. } => {
                    format!("delta {}", stop_reason.clone().unwrap_or_default())
                }
                StreamEvent::MessageStop => "fim".into(),
                StreamEvent::TextDelta(t) => format!("texto {t}"),
            })
            .collect();
        assert_eq!(
            resumo,
            vec![
                "start 0 call_1 file_read",
                "args {\"path\":",
                "args \"a.txt\"}",
                "stop 0",
                "start 1 call_2 bash",
                "args {\"command\":\"ls\"}",
                "stop 1",
                "delta tool_use",
                "fim",
            ]
        );
    }

    #[test]
    fn stream_sem_terminal_e_erro() {
        let a = sse(
            "response.output_text.delta",
            serde_json::json!({"delta": "meio"}),
        );
        let (v, fim) = eventos(&[&a]);
        assert_eq!(v.len(), 1);
        assert!(fim.is_err());
    }

    #[test]
    fn limite_de_uso_no_meio_do_stream_nao_vira_429_repetivel() {
        let f = sse(
            "response.failed",
            serde_json::json!({"response": {"error": {"code": "subscription_sharing_usage_limit_exceeded", "message": "limit"}}}),
        );
        let (_, fim) = eventos(&[&f]);
        let msg = fim.expect_err("falha").to_string();
        assert!(msg.contains("subscription_sharing_usage_limit_exceeded"));
        assert!(msg.contains(PAGINA_DE_USO));
        assert!(!msg.contains("429") && !msg.to_lowercase().contains("rate limit"));
    }

    #[test]
    fn indisponivel_vira_503_para_repetir_com_espera() {
        let e = erro_estruturado(Some(503), "subscription_sharing_usage_unavailable", "x", "")
            .to_string();
        assert!(e.contains("status=503"));
        let generico = erro_estruturado(Some(401), "", "{\"detail\":\"no\"}", "").to_string();
        assert!(generico.contains("status=401"));
        let capacidade = erro_estruturado(
            Some(400),
            "subscription_sharing_unsupported_capability",
            "no",
            "tools[0]",
        )
        .to_string();
        assert!(capacidade.contains("tools[0]"));
    }

    #[test]
    fn incomplete_termina_com_max_tokens() {
        let i = sse(
            "response.incomplete",
            serde_json::json!({"response": {"incomplete_details": {"reason": "max_output_tokens"}}}),
        );
        let (v, fim) = eventos(&[&i]);
        fim.expect("terminal");
        assert!(
            matches!(&v[0], StreamEvent::MessageDelta { stop_reason: Some(s), .. } if s == "max_tokens")
        );
    }
}
