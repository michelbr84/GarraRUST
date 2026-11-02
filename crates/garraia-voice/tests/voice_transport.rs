//! Real HTTP adapters against local mock servers; no audio service or API key.
use std::sync::{Arc, Mutex};
use std::time::Duration;

use garraia_voice::{HibikiClient, LmStudioTtsClient, WhisperClient};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

async fn read_request(socket: &mut TcpStream) -> String {
    let mut bytes = Vec::new();
    loop {
        let mut chunk = [0u8; 4096];
        let n = socket.read(&mut chunk).await.expect("read request");
        assert_ne!(n, 0);
        bytes.extend_from_slice(&chunk[..n]);
        if let Some(offset) = bytes.windows(4).position(|s| s == b"\r\n\r\n") {
            let headers = String::from_utf8_lossy(&bytes[..offset]);
            let length = headers
                .lines()
                .find_map(|s| {
                    s.to_ascii_lowercase()
                        .strip_prefix("content-length:")
                        .and_then(|s| s.trim().parse::<usize>().ok())
                })
                .unwrap_or(0);
            if bytes.len() >= offset + 4 + length {
                return String::from_utf8_lossy(&bytes).into_owned();
            }
        }
    }
}

async fn respond(socket: &mut TcpStream, status: u16, body: &[u8]) {
    let header = format!(
        "HTTP/1.1 {status} Response\r\nContent-Type: audio/wav\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    );
    socket.write_all(header.as_bytes()).await.expect("header");
    socket.write_all(body).await.expect("body");
}

#[tokio::test]
async fn transcription_overrides_and_mime_are_per_request() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!("http://{}", listener.local_addr().unwrap());
    let requests = Arc::new(Mutex::new(Vec::new()));
    let captured = requests.clone();
    let server = tokio::spawn(async move {
        for _ in 0..2 {
            let (mut socket, _) = listener.accept().await.unwrap();
            let request = read_request(&mut socket).await;
            captured.lock().unwrap().push(request);
            respond(&mut socket, 200, br#"{"text":"ola"}"#).await;
        }
    });
    let client = WhisperClient::new(&endpoint, "pt");
    assert_eq!(
        client
            .transcribe_bytes(
                b"custom audio",
                "clip.webm",
                "audio/webm",
                Some("es"),
                Some("local-stt")
            )
            .await
            .unwrap(),
        "ola"
    );
    assert_eq!(
        client
            .transcribe_bytes(b"second audio", "clip.wav", "audio/wav", None, None)
            .await
            .unwrap(),
        "ola"
    );
    server.await.unwrap();
    let requests = requests.lock().unwrap();
    assert!(requests[0].starts_with("POST /v1/audio/transcriptions "));
    assert!(requests[0].contains("Content-Type: audio/webm"));
    assert!(requests[0].contains("\r\n\r\nes\r\n"));
    assert!(requests[0].contains("\r\n\r\nlocal-stt\r\n"));
    assert!(requests[1].starts_with("POST /inference "));
    assert!(requests[1].contains("\r\n\r\npt\r\n"));
    assert!(!requests[1].contains("local-stt"));
}

#[tokio::test]
async fn transcription_provider_error_is_not_retried_or_reflected() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!("http://{}", listener.local_addr().unwrap());
    let server = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        read_request(&mut socket).await;
        respond(&mut socket, 429, b"private upstream diagnostic").await;
        // A retry would establish another connection before returning.
        assert!(
            tokio::time::timeout(Duration::from_millis(100), listener.accept())
                .await
                .is_err()
        );
    });
    let error = WhisperClient::new(&endpoint, "pt")
        .transcribe_bytes(b"audio", "clip.wav", "audio/wav", None, None)
        .await
        .unwrap_err()
        .to_string();
    assert!(error.contains("429"));
    assert!(!error.contains("private"));
    server.await.unwrap();
}

#[tokio::test]
async fn concurrent_speech_calls_keep_their_own_audio_and_voice() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!("http://{}/v1", listener.local_addr().unwrap());
    let barrier = Arc::new(tokio::sync::Barrier::new(2));
    let server = tokio::spawn(async move {
        let mut tasks = Vec::new();
        for _ in 0..2 {
            let (mut socket, _) = listener.accept().await.unwrap();
            let barrier = barrier.clone();
            tasks.push(tokio::spawn(async move {
                let request = read_request(&mut socket).await;
                assert!(request.starts_with("POST /v1/audio/speech "));
                let json: serde_json::Value =
                    serde_json::from_str(request.split("\r\n\r\n").nth(1).unwrap()).unwrap();
                barrier.wait().await;
                let output = format!(
                    "{}:{}",
                    json["input"].as_str().unwrap(),
                    json["voice"].as_str().unwrap()
                );
                respond(&mut socket, 200, output.as_bytes()).await;
            }));
        }
        for task in tasks {
            task.await.unwrap();
        }
    });
    let client = LmStudioTtsClient::new(&endpoint, "local-voice", "pt");
    let (one, two) = tokio::join!(
        client.synthesize_bytes_with_options("primeiro", Some("pt"), Some("voice-one")),
        client.synthesize_bytes_with_options("second", Some("en"), Some("voice-two")),
    );
    assert_eq!(one.unwrap(), b"primeiro:voice-one");
    assert_eq!(two.unwrap(), b"second:voice-two");
    server.await.unwrap();
}

#[tokio::test]
async fn cancelling_transcription_drops_the_upstream_request() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!("http://{}", listener.local_addr().unwrap());
    let (seen_tx, seen_rx) = tokio::sync::oneshot::channel();
    let server = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        read_request(&mut socket).await;
        seen_tx.send(()).unwrap();
        let mut buffer = [0u8; 1];
        let n = tokio::time::timeout(Duration::from_secs(2), socket.read(&mut buffer))
            .await
            .expect("cancel closes HTTP request")
            .unwrap();
        assert_eq!(n, 0);
    });
    let client = WhisperClient::new(&endpoint, "pt");
    let call = tokio::spawn(async move {
        client
            .transcribe_bytes(b"audio", "clip.wav", "audio/wav", None, None)
            .await
    });
    seen_rx.await.unwrap();
    call.abort();
    assert!(call.await.unwrap_err().is_cancelled());
    server.await.unwrap();
}

#[tokio::test]
async fn cancelling_speech_drops_the_upstream_request() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!("http://{}", listener.local_addr().unwrap());
    let (seen_tx, seen_rx) = tokio::sync::oneshot::channel();
    let server = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        read_request(&mut socket).await;
        seen_tx.send(()).unwrap();
        let mut buffer = [0u8; 1];
        let n = tokio::time::timeout(Duration::from_secs(2), socket.read(&mut buffer))
            .await
            .expect("cancel closes HTTP request")
            .unwrap();
        assert_eq!(n, 0);
    });
    let client = LmStudioTtsClient::new(&endpoint, "local-voice", "pt");
    let call = tokio::spawn(async move {
        client
            .synthesize_bytes_with_options("ola", None, None)
            .await
    });
    seen_rx.await.unwrap();
    call.abort();
    assert!(call.await.unwrap_err().is_cancelled());
    server.await.unwrap();
}

#[tokio::test]
async fn hibiki_honors_the_request_language() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!("http://{}", listener.local_addr().unwrap());
    let server = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        let request = read_request(&mut socket).await;
        let json: serde_json::Value =
            serde_json::from_str(request.split("\r\n\r\n").nth(1).unwrap()).unwrap();
        assert_eq!(json["language"], "es");
        respond(&mut socket, 200, b"audio").await;
    });
    let client = HibikiClient::new(&endpoint, "pt");
    assert_eq!(
        client.synthesize_bytes("hola", Some("es")).await.unwrap(),
        b"audio"
    );
    server.await.unwrap();
}

#[tokio::test]
async fn chunked_response_without_content_length_is_bounded() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!("http://{}", listener.local_addr().unwrap());
    let server = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        read_request(&mut socket).await;
        socket.write_all(b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n8\r\n12345678\r\n8\r\nabcdefgh\r\n0\r\n\r\n").await.unwrap();
    });
    let response = reqwest::get(endpoint).await.unwrap();
    assert!(
        garraia_voice::http::bounded_body(response, 10)
            .await
            .unwrap_err()
            .contains("size limit")
    );
    server.await.unwrap();
}
