use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use serde_json::Value;

use crate::pipeline::VoiceError;

/// Async HTTP client for the Chatterbox Multilingual TTS (Gradio) service.
///
/// Calls the Gradio `/gradio_api/call/generate_tts_audio` endpoint to synthesize
/// speech from text. The Chatterbox server must be running (e.g. via
/// `run_multilingual.bat`) before using this client.
///
/// # Contrato do endpoint `generate_tts_audio`
///
/// O app oficial (`multilingual_app.py` de resemble-ai/chatterbox) liga o botao
/// de gerar a uma funcao com **7 entradas**, nesta ordem: `text`, `language_id`,
/// `ref_wav` (audio de referencia), `exaggeration` (0.5), `temperature` (0.8),
/// `seed_num` (0) e `cfg_weight` (0.5). O Gradio 6.x recusa a chamada com menos
/// valores (`event: error`), e ate entao o cliente mandava so 4 — a sintese
/// falhava em silencio e a rota caia no fallback de texto.
///
/// Para funcionar tanto com o app oficial quanto com servidores customizados
/// (ha um local com `[text, lang, ref, number]`, 4 parametros), o cliente
/// **descobre a assinatura** em `GET /gradio_api/info`, monta o `data` com
/// exatamente o numero de valores que o servidor declara e cacheia o resultado.
/// Sem `/gradio_api/info`, usa o layout oficial de 7 valores.
#[derive(Clone)]
pub struct ChatterboxClient {
    endpoint: String,
    client: reqwest::Client,
    language: String,
    /// Cauda do payload `data` apos `text` e `language`, descoberta via
    /// `/gradio_api/info` e cacheada. Compartilhada entre clones (`Arc`) para
    /// que o `Arc<dyn TtsSynthesizer>` nao reconsulte a cada sintese. `None`
    /// = ainda nao descoberta (ou invalidada apos falha).
    signature_tail: Arc<Mutex<Option<Vec<Value>>>>,
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
            signature_tail: Arc::new(Mutex::new(None)),
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

        // Monta o `data` com o numero de valores que o servidor realmente
        // espera (descoberto em /gradio_api/info e cacheado).
        let data = self.build_data(text, lang).await;

        match self.submit_and_collect(&data).await {
            Ok(audio_bytes) => {
                tracing::info!(
                    bytes = audio_bytes.len(),
                    lang,
                    text_len = text.len(),
                    arity = data.len(),
                    "Chatterbox TTS synthesis complete"
                );
                Ok(audio_bytes)
            }
            Err(e) => {
                // A contagem de argumentos do servidor pode ter mudado (ou a
                // assinatura cacheada ficou obsoleta): descarta o cache para
                // redescobrir na proxima chamada. Barato e seguro — no pior
                // caso custa um GET /gradio_api/info a mais.
                self.invalidate_signature();
                Err(e)
            }
        }
    }

    /// Faz o ciclo Gradio call API: submete o job, faz poll do `event_id` via
    /// SSE e baixa o audio. Nao conhece a assinatura — recebe o `data` pronto.
    async fn submit_and_collect(&self, data: &[Value]) -> Result<Vec<u8>, VoiceError> {
        // Step 1: Submit the generation job via Gradio's call API
        let submit_url = format!("{}/gradio_api/call/generate_tts_audio", self.endpoint);
        let body = serde_json::json!({ "data": data });

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
        // containing the audio file path (or an "error" event).
        let event_text = event_resp
            .text()
            .await
            .map_err(|e| VoiceError::Tts(format!("Failed to read Chatterbox event stream: {e}")))?;

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

        Ok(audio_bytes.to_vec())
    }

    // ─── Descoberta da assinatura ────────────────────────────────────────

    /// Monta o array `data` completo para `text`/`lang`, usando a cauda
    /// descoberta/cacheada: `[text, lang, <referencia null>, <extras...>]`.
    async fn build_data(&self, text: &str, lang: &str) -> Vec<Value> {
        let tail = self.signature_tail().await;
        let mut data = Vec::with_capacity(2 + tail.len());
        data.push(Value::String(text.to_string()));
        data.push(Value::String(lang.to_string()));
        data.extend(tail);
        data
    }

    /// Devolve a cauda do payload, do cache quando houver, senao descobrindo
    /// via `/gradio_api/info` e guardando no cache.
    async fn signature_tail(&self) -> Vec<Value> {
        if let Some(cached) = self.cached_tail() {
            return cached;
        }
        // Lock NAO fica preso durante o await (descoberta faz I/O de rede).
        let tail = self.discover_signature_tail().await;
        self.store_tail(tail.clone());
        tail
    }

    /// Consulta `GET /gradio_api/info`, localiza
    /// `named_endpoints["/generate_tts_audio"].parameters` e monta a cauda com
    /// exatamente esse numero de valores. Se a rota nao responder ou o schema
    /// nao casar, cai no layout oficial de 7 valores.
    async fn discover_signature_tail(&self) -> Vec<Value> {
        match self.fetch_endpoint_parameters().await {
            Some(params) if params.len() >= 2 => Self::tail_from_params(&params),
            _ => Self::official_tail(),
        }
    }

    /// Busca a lista de parametros declarada pelo servidor para o endpoint
    /// `/generate_tts_audio`. `None` se a rota nao existe ou o JSON nao casa.
    async fn fetch_endpoint_parameters(&self) -> Option<Vec<Value>> {
        let url = format!("{}/gradio_api/info", self.endpoint);
        let resp = self.client.get(&url).send().await.ok()?;
        if !resp.status().is_success() {
            return None;
        }
        let info: Value = resp.json().await.ok()?;
        let params = info
            .get("named_endpoints")?
            .get("/generate_tts_audio")?
            .get("parameters")?
            .as_array()?;
        Some(params.clone())
    }

    /// Monta a cauda (indices 2..N do `data`) a partir dos parametros do
    /// servidor. Index 2 e sempre a referencia de voz = `null`; os demais usam
    /// o `parameter_default` declarado quando houver, senao o default oficial
    /// por posicao.
    fn tail_from_params(params: &[Value]) -> Vec<Value> {
        let n = params.len();
        let mut tail = Vec::with_capacity(n.saturating_sub(2));
        for data_idx in 2..n {
            if data_idx == 2 {
                // Referencia de voz: sempre null (o app oficial resolve o
                // prompt default; passar URL quebraria com librosa.load).
                tail.push(Value::Null);
                continue;
            }
            let declared = params.get(data_idx).and_then(Self::declared_default);
            tail.push(declared.unwrap_or_else(|| Self::official_default(data_idx)));
        }
        tail
    }

    /// Extrai o `parameter_default` de um parametro do `/gradio_api/info`,
    /// mas so quando o servidor marcou `parameter_has_default` e o valor nao
    /// e nulo (um default nulo cai no default oficial por posicao).
    fn declared_default(param: &Value) -> Option<Value> {
        let has_default = param
            .get("parameter_has_default")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        if !has_default {
            return None;
        }
        param
            .get("parameter_default")
            .cloned()
            .filter(|d| !d.is_null())
    }

    /// Cauda do layout oficial de 7 valores (indices 2..7 do `data`):
    /// `[referencia=null, exaggeration=0.5, temperature=0.8, seed=0, cfg_weight=0.5]`.
    /// Usada quando `/gradio_api/info` nao responde.
    fn official_tail() -> Vec<Value> {
        vec![
            Value::Null,
            serde_json::json!(0.5),
            serde_json::json!(0.8),
            serde_json::json!(0),
            serde_json::json!(0.5),
        ]
    }

    /// Default oficial do app para cada posicao do `data` (ordem de
    /// `multilingual_app.py`). Posicoes desconhecidas caem em `null`.
    fn official_default(data_idx: usize) -> Value {
        match data_idx {
            3 => serde_json::json!(0.5), // exaggeration
            4 => serde_json::json!(0.8), // temperature
            5 => serde_json::json!(0),   // seed_num (0 = aleatorio)
            6 => serde_json::json!(0.5), // cfg_weight
            _ => Value::Null,
        }
    }

    fn lock_signature(&self) -> MutexGuard<'_, Option<Vec<Value>>> {
        // Sem unwrap() em lock: um lock envenenado recupera o valor interno em
        // vez de entrar em panico (o dado cacheado e inofensivo).
        self.signature_tail
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
    }

    fn cached_tail(&self) -> Option<Vec<Value>> {
        self.lock_signature().clone()
    }

    fn store_tail(&self, tail: Vec<Value>) {
        *self.lock_signature() = Some(tail);
    }

    fn invalidate_signature(&self) {
        *self.lock_signature() = None;
    }

    /// Parse the audio file URL from the Gradio SSE event stream.
    ///
    /// Detecta tambem `event: error`: o Gradio 6.x responde assim quando a
    /// chamada chega com o numero errado de argumentos (entre outras causas).
    /// Nesse caso devolve `Err` com a mensagem do servidor — nunca o texto do
    /// usuario — para que o chamador registre o motivo antes do fallback.
    fn parse_audio_url_from_sse(sse_text: &str, endpoint: &str) -> Result<String, VoiceError> {
        // SSE format:
        //   event: complete
        //   data: [{"path": "/tmp/gradio/.../audio.wav", "url": "...", ...}]
        // ou, em falha:
        //   event: error
        //   data: "mensagem do servidor"
        let mut current_event: Option<String> = None;
        for line in sse_text.lines() {
            if let Some(ev) = line.strip_prefix("event:") {
                current_event = Some(ev.trim().to_string());
                continue;
            }
            let Some(data) = line.strip_prefix("data:") else {
                continue;
            };
            let data = data.trim_start();
            match current_event.as_deref() {
                Some("error") => {
                    let msg = Self::extract_error_message(data);
                    return Err(VoiceError::Tts(format!(
                        "Chatterbox returned error event: {msg}"
                    )));
                }
                Some("complete") => {
                    if let Some(url) = Self::extract_audio_url(data, endpoint) {
                        return Ok(url);
                    }
                    // data da conclusao nao parseavel: segue procurando.
                }
                _ => {} // generating / heartbeat / etc — ignorado
            }
        }

        Err(VoiceError::Tts(format!(
            "Could not parse audio URL from Chatterbox SSE response: {}",
            &sse_text[..sse_text.len().min(500)]
        )))
    }

    /// Extrai `url` (ou constroi a partir de `path`) do payload `data` de um
    /// evento `complete`, aceitando tanto array quanto objeto solto.
    fn extract_audio_url(data: &str, endpoint: &str) -> Option<String> {
        let from_obj = |obj: &Value| -> Option<String> {
            if let Some(url) = obj.get("url").and_then(|v| v.as_str()) {
                return Some(url.to_string());
            }
            obj.get("path")
                .and_then(|v| v.as_str())
                .map(|path| format!("{endpoint}/file={path}"))
        };

        // Primeiro como array (`[{...}]`), depois como objeto solto (`{...}`).
        let from_array = serde_json::from_str::<Vec<Value>>(data)
            .ok()
            .and_then(|arr| arr.first().and_then(&from_obj));
        from_array.or_else(|| {
            serde_json::from_str::<Value>(data)
                .ok()
                .and_then(|obj| from_obj(&obj))
        })
    }

    /// Normaliza a mensagem de um `event: error`. O `data` pode ser uma string
    /// JSON, `null`, ou um objeto/array; devolve algo curto e sem o texto do
    /// usuario. Limitada em tamanho para nao poluir o log.
    fn extract_error_message(data: &str) -> String {
        let msg = match serde_json::from_str::<Value>(data) {
            Ok(Value::String(s)) => s,
            Ok(Value::Null) => "unknown error (null)".to_string(),
            Ok(other) => other.to_string(),
            Err(_) => data.to_string(),
        };
        let msg = msg.trim();
        if msg.is_empty() {
            return "unknown error".to_string();
        }
        if msg.chars().count() > 300 {
            let truncated: String = msg.chars().take(300).collect();
            format!("{truncated}...")
        } else {
            msg.to_string()
        }
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
                        let rota = req.split_whitespace().nth(1).unwrap_or("/").to_string();
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

    /// Servidor que responde `GET /gradio_api/info` com um corpo JSON fixo (ou
    /// 404 quando `info_body` e `None`), registrando quantas vezes a rota foi
    /// pedida. Serve a descoberta de assinatura de `build_data`.
    struct GradioInfo {
        endpoint: String,
        info_hits: Arc<Mutex<usize>>,
    }

    impl GradioInfo {
        async fn new(info_body: Option<&'static str>) -> Self {
            let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
            let porta = listener.local_addr().expect("addr").port();
            let info_hits = Arc::new(Mutex::new(0usize));
            let contador = Arc::clone(&info_hits);

            tokio::spawn(async move {
                loop {
                    let Ok((mut socket, _)) = listener.accept().await else {
                        return;
                    };
                    let contador = Arc::clone(&contador);
                    tokio::spawn(async move {
                        let mut buf = [0u8; 2048];
                        let lidos = socket.read(&mut buf).await.unwrap_or(0);
                        let req = String::from_utf8_lossy(&buf[..lidos]).to_string();
                        let rota = req.split_whitespace().nth(1).unwrap_or("/").to_string();
                        let resposta = if rota == "/gradio_api/info" {
                            *contador.lock().expect("contador") += 1;
                            match info_body {
                                Some(body) => format!(
                                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\n\
                                     Content-Length: {}\r\nConnection: close\r\n\r\n{}",
                                    body.len(),
                                    body
                                ),
                                None => "HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\n\
                                         Connection: close\r\n\r\n"
                                    .to_string(),
                            }
                        } else {
                            "HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                                .to_string()
                        };
                        let _ = socket.write_all(resposta.as_bytes()).await;
                        let _ = socket.flush().await;
                    });
                }
            });

            Self {
                endpoint: format!("http://127.0.0.1:{porta}"),
                info_hits,
            }
        }

        fn info_hits(&self) -> usize {
            *self.info_hits.lock().expect("contador")
        }
    }

    // Assinatura oficial (7 parametros), com defaults declarados DIFERENTES dos
    // oficiais em exaggeration/temperature para provar que lemos o
    // `parameter_default` do servidor, nao o fallback.
    const INFO_7_PARAMS: &str = r#"{
      "named_endpoints": {
        "/generate_tts_audio": {
          "parameters": [
            {"parameter_name": "text_input", "parameter_has_default": false},
            {"parameter_name": "language_id", "parameter_has_default": false},
            {"parameter_name": "audio_prompt_path_input", "parameter_has_default": true, "parameter_default": null},
            {"parameter_name": "exaggeration_input", "parameter_has_default": true, "parameter_default": 0.6},
            {"parameter_name": "temperature_input", "parameter_has_default": true, "parameter_default": 0.7},
            {"parameter_name": "seed_num_input", "parameter_has_default": true, "parameter_default": 0},
            {"parameter_name": "cfgw_input", "parameter_has_default": true, "parameter_default": 0.5}
          ]
        }
      }
    }"#;

    // Servidor customizado de 4 parametros: [text, lang, ref, number], com o
    // number declarando default 1.0.
    const INFO_4_PARAMS: &str = r#"{
      "named_endpoints": {
        "/generate_tts_audio": {
          "parameters": [
            {"parameter_name": "text", "parameter_has_default": false},
            {"parameter_name": "lang", "parameter_has_default": false},
            {"parameter_name": "ref", "parameter_has_default": true, "parameter_default": null},
            {"parameter_name": "number", "parameter_has_default": true, "parameter_default": 1.0}
          ]
        }
      }
    }"#;

    /// Servidor de 7 params: o `data` enviado tem exatamente 7 valores, com os
    /// defaults declarados pelo servidor nas posicoes de parametro.
    #[tokio::test]
    async fn sete_params_monta_data_com_defaults_declarados() {
        let gradio = GradioInfo::new(Some(INFO_7_PARAMS)).await;
        let client = ChatterboxClient::new(&gradio.endpoint, "pt");

        let data = client.build_data("ola garra", "pt").await;

        assert_eq!(
            data,
            vec![
                Value::String("ola garra".to_string()),
                Value::String("pt".to_string()),
                Value::Null,            // referencia
                serde_json::json!(0.6), // exaggeration declarado
                serde_json::json!(0.7), // temperature declarado
                serde_json::json!(0),   // seed declarado
                serde_json::json!(0.5), // cfg_weight declarado
            ],
            "7 valores, respeitando os defaults declarados pelo servidor"
        );
    }

    /// Servidor customizado de 4 params: o `data` enviado tem exatamente 4
    /// valores — nunca mais do que o servidor aceita.
    #[tokio::test]
    async fn quatro_params_monta_data_com_quatro_valores() {
        let gradio = GradioInfo::new(Some(INFO_4_PARAMS)).await;
        let client = ChatterboxClient::new(&gradio.endpoint, "pt");

        let data = client.build_data("ola", "en").await;

        assert_eq!(
            data,
            vec![
                Value::String("ola".to_string()),
                Value::String("en".to_string()),
                Value::Null,            // referencia
                serde_json::json!(1.0), // number, default declarado pelo servidor
            ],
            "servidor de 4 params recebe exatamente 4 valores"
        );
    }

    /// Sem `/gradio_api/info` (404), cai no layout oficial de 7 valores.
    #[tokio::test]
    async fn info_indisponivel_usa_layout_oficial_de_sete() {
        let gradio = GradioInfo::new(None).await;
        let client = ChatterboxClient::new(&gradio.endpoint, "pt");

        let data = client.build_data("ola", "pt").await;

        assert_eq!(
            data,
            vec![
                Value::String("ola".to_string()),
                Value::String("pt".to_string()),
                Value::Null,            // referencia
                serde_json::json!(0.5), // exaggeration oficial
                serde_json::json!(0.8), // temperature oficial
                serde_json::json!(0),   // seed oficial
                serde_json::json!(0.5), // cfg_weight oficial
            ],
            "sem /gradio_api/info o layout oficial de 7 valores e usado"
        );
    }

    /// A assinatura e cacheada: duas sinteses so consultam /gradio_api/info uma
    /// vez.
    #[tokio::test]
    async fn assinatura_e_cacheada_entre_chamadas() {
        let gradio = GradioInfo::new(Some(INFO_7_PARAMS)).await;
        let client = ChatterboxClient::new(&gradio.endpoint, "pt");

        let _ = client.build_data("um", "pt").await;
        let _ = client.build_data("dois", "pt").await;

        assert_eq!(
            gradio.info_hits(),
            1,
            "/gradio_api/info deve ser consultada uma unica vez (cache)"
        );
    }

    /// Invalidar a assinatura forca nova consulta na proxima montagem.
    #[tokio::test]
    async fn invalidar_assinatura_reforca_consulta() {
        let gradio = GradioInfo::new(Some(INFO_7_PARAMS)).await;
        let client = ChatterboxClient::new(&gradio.endpoint, "pt");

        let _ = client.build_data("um", "pt").await;
        client.invalidate_signature();
        let _ = client.build_data("dois", "pt").await;

        assert_eq!(
            gradio.info_hits(),
            2,
            "apos invalidar, a assinatura e redescoberta"
        );
    }

    /// `event: error` no SSE vira `Err` com a mensagem do servidor.
    #[test]
    fn evento_de_erro_no_sse_vira_err() {
        let sse = "event: error\ndata: \"server is busy\"\n\n";
        let err =
            ChatterboxClient::parse_audio_url_from_sse(sse, "http://x").expect_err("deve falhar");
        let VoiceError::Tts(msg) = err else {
            panic!("esperava VoiceError::Tts");
        };
        assert!(
            msg.contains("error event") && msg.contains("server is busy"),
            "mensagem do servidor deve aparecer: {msg}"
        );
    }

    /// `event: error` com `data: null` ainda vira `Err` (sem panico).
    #[test]
    fn evento_de_erro_nulo_vira_err() {
        let sse = "event: error\ndata: null\n\n";
        let err =
            ChatterboxClient::parse_audio_url_from_sse(sse, "http://x").expect_err("deve falhar");
        assert!(matches!(err, VoiceError::Tts(_)));
    }

    /// `event: complete` com url continua resolvendo o audio (nao regride).
    #[test]
    fn evento_completo_resolve_url_do_audio() {
        let sse = "event: complete\ndata: [{\"url\": \"http://x/file/audio.wav\", \"path\": \"/tmp/a.wav\"}]\n\n";
        let url =
            ChatterboxClient::parse_audio_url_from_sse(sse, "http://x").expect("deve resolver");
        assert_eq!(url, "http://x/file/audio.wav");
    }

    /// `event: complete` sem url mas com path constroi a URL `/file=`.
    #[test]
    fn evento_completo_constroi_url_a_partir_do_path() {
        let sse = "event: complete\ndata: [{\"path\": \"/tmp/gradio/a.wav\"}]\n\n";
        let url =
            ChatterboxClient::parse_audio_url_from_sse(sse, "http://x").expect("deve resolver");
        assert_eq!(url, "http://x/file=/tmp/gradio/a.wav");
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
            ChatterboxClient::ROTAS_DE_SAUDE[1],
            "/config",
            "o config do Gradio fica em /config"
        );
    }
}
