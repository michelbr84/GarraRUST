- **TTS da Web volta a tocar o audio, com interrupcao e limpeza de recursos (#1754).**
  No sucesso o `POST /api/tts` devolve os bytes WAV crus com `Content-Type: audio/wav`,
  mas o `playTTS` do webchat fazia `await r.json()` e lia `audio` em base64 — o parse
  quebrava sobre o binario e o erro morria num `catch {}` vazio, entao o TTS nunca
  tocava. Agora o cliente le o content-type: em `audio/*` toca um `<audio>` via
  `URL.createObjectURL`, interrompe e revoga a fala anterior ao comecar uma nova e no
  `ended`/`error`, e avisa no console em vez de silenciar (autoplay bloqueado, resposta
  sem audio). O toggle "Read AI responses aloud" ganhou `data-testid` estavel.
- **TTS nao entra mais em panico com acento ou emoji na fronteira de 80 bytes (#1754).**
  O log de inicio da sintese cortava o texto por byte (`&texto[..80]`), caindo no meio de
  um code point multibyte (acento pt-BR, emoji) e entrando em panico; como o gateway nao
  tem `CatchPanicLayer`, a conexao caia. O corte passa a ser por caractere.
