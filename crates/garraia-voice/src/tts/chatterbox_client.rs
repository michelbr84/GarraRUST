use crate::pipeline::VoiceError;

/// Async HTTP client for the Chatterbox Multilingual TTS (Gradio) service.
///
/// Calls the Gradio `/gradio_api/call/generate_tts_audio` endpoint to synthesize
/// speech from text. The Chatterbox server must be running (e.g. via
/// `run_multilingual.bat`) before using this client.
#[derive(Clone)]
pub struct ChatterboxClient {
    endpoint: String,
    client: reqwest::Client,
    language: String,
}

impl ChatterboxClient {
    /// Create a new Chatterbox client.
    ///
    /// # Arguments
    /// * `endpoint` – base URL of the Gradio app, e.g. `http://127.0.0.1:7860`
    /// * `language` – default language code (e.g. `"pt"` for Portuguese)
    pub fn new(endpoint: &str, language: &str) -> Self {
        Self {
            endpoint: endpoint.trim_end_matches('/').to_string(),
            client: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(120))
                .build()
                .unwrap_or_else(|_| reqwest::Client::new()),
            language: language.to_string(),
        }
    }

    /// Rotas sondadas pelo `health_check`, na ordem. A primeira que responder
    /// 2xx decide que o servidor esta de pe.
    ///
    /// `/gradio_api/config` NAO esta aqui: era a rota da antiga sonda de
    /// fallback e nao existe em nenhuma versao do Gradio (#1538) — o app do
    /// chatterbox serve o config em `/config` (4.x ate 6.x). Com o servidor
    /// saudavel e a raiz por algum motivo indisponivel, a sonda antiga dava
    /// 404 e o gateway reportava `❌ tts-chatterbox` deixando o modo voz
    /// desligado. `/gradio_api/info` fecha a lista porque e a rota do schema
    /// da API, a mesma familia do caminho de sintese que usamos de verdade.
    pub const ROTAS_DE_SAUDE: [&'static str; 3] = ["/", "/config", "/gradio_api/info"];

    /// Check if the Chatterbox server is reachable.
    ///
    /// Sonda [`Self::ROTAS_DE_SAUDE`] em ordem e para na primeira 2xx. Só
    /// reporta `false` depois de todas falharem.
    pub async fn health_check(&self) -> Result<bool, VoiceError> {
        // Por que registrar o motivo de cada rota: antes, "nada escutando"
        // (erro de transporte) e "servidor de pe na rota errada" (404)
        // saiam com a MESMA linha de WARN sem status nenhum, e era
        // impossivel distinguir os dois em producao (#1538).
        let mut motivos: Vec<String> = Vec::with_capacity(Self::ROTAS_DE_SAUDE.len());

        for rota in Self::ROTAS_DE_SAUDE {
            let url = format!("{}{}", self.endpoint, rota);
            match self.client.get(&url).send().await {
                Ok(resp) if resp.status().is_success() => return Ok(true),
                Ok(resp) => motivos.push(format!("{rota} -> HTTP {}", resp.status().as_u16())),
                Err(e) => {
                    let motivo = if e.is_connect() {
                        "connect failed".to_string()
                    } else if e.is_timeout() {
                        "timeout".to_string()
                    } else {
                        "request error".to_string()
                    };
                    motivos.push(format!("{rota} -> {motivo}"));
                }
            }
        }

        tracing::warn!(
            endpoint = %self.endpoint,
            probes = %motivos.join("; "),
            "Chatterbox health check failed on every probe"
        );
        Ok(false)
    }

    /// Synthesize speech from text using the Chatterbox Multilingual model.
    ///
    /// Returns the raw audio bytes (WAV format).
    ///
    /// # Arguments
    /// * `text` – the text to speak
    /// * `language` – optional language override (uses default if `None`)
    pub async fn synthesize(
        &self,
        text: &str,
        language: Option<&str>,
    ) -> Result<Vec<u8>, VoiceError> {
        let lang = language.unwrap_or(&self.language);

        // Step 1: Submit the generation job via Gradio's call API
        let submit_url = format!("{}/gradio_api/call/generate_tts_audio", self.endpoint);
        let body = serde_json::json!({
            "data": [text, lang, null, 1.0]
        });

        let resp = self
            .client
            .post(&submit_url)
            .json(&body)
            .send()
            .await
            .map_err(|e| VoiceError::Tts(format!("Chatterbox submit failed: {e}")))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(VoiceError::Tts(format!(
                "Chatterbox submit returned {status}: {body}"
            )));
        }

        #[derive(serde::Deserialize)]
        struct SubmitResponse {
            event_id: String,
        }

        let submit_result: SubmitResponse = resp.json().await.map_err(|e| {
            VoiceError::Tts(format!("Failed to parse Chatterbox submit response: {e}"))
        })?;

        // Step 2: Poll for the result using the event_id
        let result_url = format!(
            "{}/gradio_api/call/generate_tts_audio/{}",
            self.endpoint, submit_result.event_id
        );

        let event_resp = self
            .client
            .get(&result_url)
            .send()
            .await
            .map_err(|e| VoiceError::Tts(format!("Chatterbox result poll failed: {e}")))?;

        if !event_resp.status().is_success() {
            let status = event_resp.status();
            let body = event_resp.text().await.unwrap_or_default();
            return Err(VoiceError::Tts(format!(
                "Chatterbox result returned {status}: {body}"
            )));
        }

        // The event stream returns SSE lines. Parse for the "complete" event
        // containing the audio file path.
        let event_text = event_resp
            .text()
            .await
            .map_err(|e| VoiceError::Tts(format!("Failed to read Chatterbox event stream: {e}")))?;

        // Parse SSE: look for "data:" line after "event: complete"
        let audio_url = Self::parse_audio_url_from_sse(&event_text, &self.endpoint)?;

        // Step 3: Download the audio file
        let audio_resp = self
            .client
            .get(&audio_url)
            .send()
            .await
            .map_err(|e| VoiceError::Tts(format!("Failed to download audio: {e}")))?;

        if !audio_resp.status().is_success() {
            let status = audio_resp.status();
            return Err(VoiceError::Tts(format!("Audio download returned {status}")));
        }

        let audio_bytes = audio_resp
            .bytes()
            .await
            .map_err(|e| VoiceError::Tts(format!("Failed to read audio bytes: {e}")))?;

        tracing::info!(
            bytes = audio_bytes.len(),
            lang,
            text_len = text.len(),
            "Chatterbox TTS synthesis complete"
        );

        Ok(audio_bytes.to_vec())
    }

    /// Parse the audio file URL from the Gradio SSE event stream.
    fn parse_audio_url_from_sse(sse_text: &str, endpoint: &str) -> Result<String, VoiceError> {
        // SSE format:
        //   event: complete
        //   data: [{"path": "/tmp/gradio/.../audio.wav", "url": "...", ...}]
        let mut found_complete = false;
        for line in sse_text.lines() {
            if line.starts_with("event: complete") {
                found_complete = true;
                continue;
            }
            if found_complete && line.starts_with("data: ") {
                let data = &line["data: ".len()..];
                // Try parsing as JSON array
                if let Ok(arr) = serde_json::from_str::<Vec<serde_json::Value>>(data) {
                    // The audio output is typically the first element
                    if let Some(obj) = arr.first() {
                        // Check for "url" field first
                        if let Some(url) = obj.get("url").and_then(|v| v.as_str()) {
                            return Ok(url.to_string());
                        }
                        // Fall back to constructing from "path"
                        if let Some(path) = obj.get("path").and_then(|v| v.as_str()) {
                            return Ok(format!("{}/file={}", endpoint, path));
                        }
                    }
                }
                // Maybe data is a single object instead of array
                if let Ok(obj) = serde_json::from_str::<serde_json::Value>(data) {
                    if let Some(url) = obj.get("url").and_then(|v| v.as_str()) {
                        return Ok(url.to_string());
                    }
                    if let Some(path) = obj.get("path").and_then(|v| v.as_str()) {
                        return Ok(format!("{}/file={}", endpoint, path));
                    }
                }
            }
        }

        Err(VoiceError::Tts(format!(
            "Could not parse audio URL from Chatterbox SSE response: {}",
            &sse_text[..sse_text.len().min(500)]
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;
    use std::sync::{Arc, Mutex};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    /// Um servidor HTTP minimo que responde 200 apenas nas rotas de `ok` e
    /// 404 em todas as outras, registrando o que foi pedido.
    ///
    /// Escrito a mao sobre `tokio::net` de proposito: `garraia-voice` nao tem
    /// dev-dependency de servidor, e a sonda so precisa de status line.
    struct Gradio {
        endpoint: String,
        pedidos: Arc<Mutex<Vec<String>>>,
    }

    impl Gradio {
        async fn com_rotas(ok: &[&'static str]) -> Self {
            let ok: HashSet<&'static str> = ok.iter().copied().collect();
            let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
            let porta = listener.local_addr().expect("addr").port();
            let pedidos = Arc::new(Mutex::new(Vec::new()));
            let registro = Arc::clone(&pedidos);

            tokio::spawn(async move {
                loop {
                    let Ok((mut socket, _)) = listener.accept().await else {
                        return;
                    };
                    let ok = ok.clone();
                    let registro = Arc::clone(&registro);
                    tokio::spawn(async move {
                        let mut buf = [0u8; 2048];
                        let lidos = socket.read(&mut buf).await.unwrap_or(0);
                        let req = String::from_utf8_lossy(&buf[..lidos]).to_string();
                        let rota = req
                            .split_whitespace()
                            .nth(1)
                            .unwrap_or("/")
                            .to_string();
                        registro.lock().expect("registro").push(rota.clone());
                        let resposta = if ok.contains(rota.as_str()) {
                            "HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok"
                        } else {
                            "HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                        };
                        let _ = socket.write_all(resposta.as_bytes()).await;
                        let _ = socket.flush().await;
                    });
                }
            });

            Self {
                endpoint: format!("http://127.0.0.1:{porta}"),
                pedidos,
            }
        }

        fn rotas_pedidas(&self) -> Vec<String> {
            self.pedidos.lock().expect("registro").clone()
        }
    }

    /// O caso do #1538: o app do chatterbox de pe, servindo o config em
    /// `/config`, e a raiz indisponivel. A sonda antiga caia em
    /// `/gradio_api/config` (404 em todo Gradio) e reportava o servidor como
    /// fora do ar, deixando o modo voz desligado.
    #[tokio::test]
    async fn config_de_pe_sem_raiz_e_saudavel() {
        let gradio = Gradio::com_rotas(&["/config"]).await;
        let client = ChatterboxClient::new(&gradio.endpoint, "pt");

        assert!(
            client.health_check().await.expect("health"),
            "servidor servindo /config tem de ser reportado saudavel"
        );
        assert_eq!(
            gradio.rotas_pedidas(),
            vec!["/".to_string(), "/config".to_string()],
            "a cadeia para na primeira 2xx e nunca pede /gradio_api/config"
        );
    }

    /// A raiz 200 (o caso comum) decide sozinha: nenhuma rota extra vai para a
    /// rede.
    #[tokio::test]
    async fn raiz_de_pe_encerra_a_cadeia() {
        let gradio = Gradio::com_rotas(&["/"]).await;
        let client = ChatterboxClient::new(&gradio.endpoint, "pt");

        assert!(client.health_check().await.expect("health"));
        assert_eq!(gradio.rotas_pedidas(), vec!["/".to_string()]);
    }

    /// Sem falso positivo: servidor de pe que responde 404 em TODA a cadeia
    /// continua sendo reportado como fora do ar.
    #[tokio::test]
    async fn tudo_404_nao_e_saudavel() {
        let gradio = Gradio::com_rotas(&[]).await;
        let client = ChatterboxClient::new(&gradio.endpoint, "pt");

        assert!(!client.health_check().await.expect("health"));
        assert_eq!(
            gradio.rotas_pedidas().len(),
            ChatterboxClient::ROTAS_DE_SAUDE.len(),
            "sem 2xx, a cadeia inteira e sondada antes de desistir"
        );
    }

    /// Nada escutando tambem e `false` — e sem erro, porque o chamador do boot
    /// trata `Ok(false)` como "nao alcancavel" e segue.
    #[tokio::test]
    async fn nada_escutando_nao_e_saudavel() {
        // Uma porta que foi aberta e fechada: ninguem escuta nela agora.
        let porta = {
            let l = TcpListener::bind("127.0.0.1:0").await.expect("bind");
            l.local_addr().expect("addr").port()
        };
        let client = ChatterboxClient::new(&format!("http://127.0.0.1:{porta}"), "pt");

        assert!(!client.health_check().await.expect("health"));
    }

    /// A rota que o #1538 acusou nao pode voltar para a cadeia: ela nao existe
    /// em nenhuma versao do Gradio (4.x a 6.x), e enquanto estava aqui um
    /// servidor saudavel aparecia como `❌ tts-chatterbox`.
    #[test]
    fn a_cadeia_nao_contem_gradio_api_config() {
        assert!(
            !ChatterboxClient::ROTAS_DE_SAUDE.contains(&"/gradio_api/config"),
            "/gradio_api/config nao e rota de nenhum Gradio — ver #1538"
        );
        assert_eq!(
            ChatterboxClient::ROTAS_DE_SAUDE[1], "/config",
            "o config do Gradio fica em /config"
        );
    }
}
