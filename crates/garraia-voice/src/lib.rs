//! # garraia-voice
//!
//! Sistema de voz do GarraIA.
//!
//! Arquitetura:
//!   Whisper (STT) → LLM (cérebro) → TTS (Chatterbox/Hibiki/LMStudio)
//!
//! Interface principal:
//! ```ignore
//! let pipeline = VoicePipeline::new(whisper, hibiki, work_dir);
//! let (output_ogg, metrics) = pipeline.process_voice(&input_ogg, |text| async {
//!     // envia texto ao LLM e retorna resposta
//!     Ok(llm_response)
//! }).await?;
//! ```

pub mod audio;
pub mod http;
pub mod pipeline;
pub mod stt;
pub mod tts;

// Re-exports para conveniência
pub use http::{MAX_AUDIO_BYTES, MAX_TEXT_BYTES, VOICE_REQUEST_TIMEOUT};
pub use pipeline::{VoiceError, VoiceMetrics, VoicePipeline};
pub use stt::whisper_client::WhisperClient;
pub use tts::chatterbox_client::ChatterboxClient;
pub use tts::hibiki_client::HibikiClient;
pub use tts::lmstudio_client::LmStudioTtsClient;

/// Idioma pedido por um chamador do [`TtsSynthesizer`]. Vazio ou `"default"`
/// (o Telegram passa `"default"`) significa "use o idioma configurado".
fn language_override(language: &str) -> Option<&str> {
    let language = language.trim();
    (!language.is_empty() && !language.eq_ignore_ascii_case("default")).then_some(language)
}

/// Trait unificado para provedores TTS.
///
/// Retorna bytes de áudio WAV cru.
#[async_trait::async_trait]
pub trait TtsSynthesizer: Send + Sync {
    async fn synthesize_bytes(&self, text: &str, language: &str) -> Result<Vec<u8>, VoiceError>;
}

#[async_trait::async_trait]
impl TtsSynthesizer for ChatterboxClient {
    async fn synthesize_bytes(&self, text: &str, language: &str) -> Result<Vec<u8>, VoiceError> {
        self.synthesize(text, language_override(language)).await
    }
}

#[async_trait::async_trait]
impl TtsSynthesizer for HibikiClient {
    async fn synthesize_bytes(&self, text: &str, language: &str) -> Result<Vec<u8>, VoiceError> {
        // #1754: os bytes vem direto da resposta HTTP. O caminho antigo
        // passava por `garraia_tts_<pid>.wav`, o MESMO arquivo para toda
        // chamada do processo: duas sinteses concorrentes trocavam o audio
        // entre si. Sem arquivo, nao ha o que compartilhar nem o que limpar.
        HibikiClient::synthesize_bytes(self, text, language_override(language)).await
    }
}

#[async_trait::async_trait]
impl TtsSynthesizer for LmStudioTtsClient {
    async fn synthesize_bytes(&self, text: &str, language: &str) -> Result<Vec<u8>, VoiceError> {
        // #1754: sem o temporario compartilhado por PID — ver o Hibiki acima.
        self.synthesize_bytes_with_options(text, language_override(language), None)
            .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    /// Monta uma resposta HTTP/1.1 completa.
    fn http(status: &str, content_type: &str, corpo: &str) -> String {
        format!(
            "HTTP/1.1 {status}\r\nContent-Type: {content_type}\r\n\
             Content-Length: {}\r\nConnection: close\r\n\r\n{corpo}",
            corpo.len()
        )
    }

    /// Texto do pedido de sintese: `text` no Hibiki, `input` no LM Studio.
    fn texto_do_pedido(json: &serde_json::Value) -> String {
        json.get("text")
            .or_else(|| json.get("input"))
            .and_then(|v| v.as_str())
            .expect("texto no pedido")
            .to_string()
    }

    /// Servidor TTS falso em loopback: le o pedido inteiro (cabecalho e
    /// corpo JSON) e devolve o que `responder(rota, json)` montar.
    ///
    /// Escrito a mao sobre `tokio::net`, como o do `chatterbox_client`:
    /// `garraia-voice` nao tem dev-dependency de servidor HTTP.
    async fn servidor_tts(responder: fn(&str, &serde_json::Value) -> String) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let porta = listener.local_addr().expect("addr").port();
        tokio::spawn(async move {
            loop {
                let Ok((mut socket, _)) = listener.accept().await else {
                    return;
                };
                tokio::spawn(async move {
                    let mut req = Vec::new();
                    let mut buf = [0u8; 4096];
                    // Le ate ter `Content-Length` bytes depois da linha em
                    // branco.
                    let (rota, corpo) = loop {
                        let lidos = socket.read(&mut buf).await.unwrap_or(0);
                        if lidos == 0 {
                            return;
                        }
                        req.extend_from_slice(&buf[..lidos]);
                        let texto = String::from_utf8_lossy(&req).to_string();
                        let Some(fim) = texto.find("\r\n\r\n") else {
                            continue;
                        };
                        let tamanho = texto[..fim]
                            .lines()
                            .find_map(|l| {
                                let (k, v) = l.split_once(':')?;
                                k.eq_ignore_ascii_case("content-length")
                                    .then(|| v.trim().parse::<usize>().ok())
                                    .flatten()
                            })
                            .unwrap_or(0);
                        if req.len() >= fim + 4 + tamanho {
                            let rota = texto.split_whitespace().nth(1).unwrap_or("/").to_string();
                            break (rota, req[fim + 4..fim + 4 + tamanho].to_vec());
                        }
                    };
                    let json: serde_json::Value =
                        serde_json::from_slice(&corpo).expect("json do pedido");
                    let resposta = responder(&rota, &json);
                    let _ = socket.write_all(resposta.as_bytes()).await;
                    let _ = socket.flush().await;
                });
            }
        });
        format!("http://127.0.0.1:{porta}")
    }

    /// Devolve um "WAV" cujo corpo e o proprio texto pedido: cada chamada
    /// recebe bytes que so ela reconhece como seus.
    fn eco_do_texto(_rota: &str, json: &serde_json::Value) -> String {
        http(
            "200 OK",
            "audio/wav",
            &format!("RIFF-falso:{}", texto_do_pedido(json)),
        )
    }

    /// Dispara `n` sinteses AO MESMO TEMPO pelo mesmo provedor e exige que
    /// cada uma receba o audio do proprio texto.
    async fn cada_chamada_recebe_o_proprio_audio(tts: Arc<dyn TtsSynthesizer>, n: usize) {
        let mut tarefas = Vec::with_capacity(n);
        for i in 0..n {
            let tts = Arc::clone(&tts);
            tarefas.push(tokio::spawn(async move {
                let texto = format!("usuario-{i}");
                let audio = tts.synthesize_bytes(&texto, "pt").await;
                (texto, audio)
            }));
        }
        for tarefa in tarefas {
            let (texto, audio) = tarefa.await.expect("tarefa");
            let audio = audio.unwrap_or_else(|e| panic!("{texto}: sintese falhou: {e}"));
            assert_eq!(
                String::from_utf8_lossy(&audio),
                format!("RIFF-falso:{texto}"),
                "{texto} recebeu o audio de outra chamada"
            );
        }
    }

    /// #1754 (P1): o temporario era `garraia_tts_<pid>.wav`, o MESMO para
    /// toda chamada do processo. Duas sinteses concorrentes escreviam e
    /// liam o mesmo arquivo, e um usuario podia ouvir o audio de outro (ou
    /// receber erro porque a outra chamada ja tinha apagado o arquivo).
    /// Contra o codigo antigo este teste falhava nas duas formas.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn hibiki_chamadas_concorrentes_nao_trocam_audio() {
        let endpoint = servidor_tts(eco_do_texto).await;
        let tts: Arc<dyn TtsSynthesizer> = Arc::new(HibikiClient::new(&endpoint, "pt"));
        cada_chamada_recebe_o_proprio_audio(tts, 32).await;
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn lmstudio_chamadas_concorrentes_nao_trocam_audio() {
        let endpoint = servidor_tts(eco_do_texto).await;
        let tts: Arc<dyn TtsSynthesizer> =
            Arc::new(LmStudioTtsClient::new(&endpoint, "tts-modelo", "pt"));
        cada_chamada_recebe_o_proprio_audio(tts, 32).await;
    }

    /// O fallback `/api/v1/chat` do LM Studio tambem devolve bytes direto,
    /// inclusive quando o audio chega em base64 dentro de JSON.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn lmstudio_fallback_chat_base64_concorrente() {
        fn chat_base64(rota: &str, json: &serde_json::Value) -> String {
            use base64::Engine;
            if rota != "/api/v1/chat" {
                return http("404 Not Found", "text/plain", "");
            }
            let audio = format!("RIFF-falso:{}", texto_do_pedido(json));
            let b64 = base64::engine::general_purpose::STANDARD.encode(audio);
            http(
                "200 OK",
                "application/json",
                &serde_json::json!({ "audio": b64 }).to_string(),
            )
        }
        let endpoint = servidor_tts(chat_base64).await;
        let tts: Arc<dyn TtsSynthesizer> =
            Arc::new(LmStudioTtsClient::new(&endpoint, "tts-modelo", "pt"));
        cada_chamada_recebe_o_proprio_audio(tts, 8).await;
    }

    /// Corpo JSON sem audio cujo byte 200 cai no meio de um "ã" (2 bytes).
    fn corpo_sem_audio() -> String {
        serde_json::json!({ "mensagem": "ã".repeat(150) }).to_string()
    }

    /// Resposta sem audio vira erro, e o resumo do corpo no erro e cortado
    /// por CARACTERE: o corte antigo por byte (`&body[..200]`) dava panic
    /// quando o byte 200 caia no meio de um caractere multibyte.
    #[tokio::test]
    async fn lmstudio_resposta_sem_audio_e_erro_sem_panic() {
        fn sem_audio(rota: &str, _json: &serde_json::Value) -> String {
            if rota != "/api/v1/chat" {
                return http("404 Not Found", "text/plain", "");
            }
            http("200 OK", "application/json", &corpo_sem_audio())
        }
        assert!(
            !corpo_sem_audio().is_char_boundary(200),
            "o teste so prova algo se o corte por byte cair no meio de um caractere"
        );
        let endpoint = servidor_tts(sem_audio).await;
        let tts = LmStudioTtsClient::new(&endpoint, "tts-modelo", "pt");
        let err = tts
            .synthesize_to_bytes("oi")
            .await
            .expect_err("resposta sem audio tem de ser erro");
        assert!(
            err.to_string().contains("non-audio response"),
            "erro inesperado: {err}"
        );
    }
}
