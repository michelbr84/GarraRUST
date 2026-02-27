# Voice Mode

GarraIA supports end-to-end voice conversation with speech-to-text and text-to-speech.

## Overview

```
┌─────────────────────────────────────────────────────────────┐
│                    VOICE PIPELINE                             │
├─────────────────────────────────────────────────────────────┤
│                                                              │
│  User Audio → STT → LLM → TTS → Audio Response             │
│                                                              │
│  STT: Whisper (local or API)                               │
│  TTS: Chatterbox, Hibiki, OpenAI TTS                       │
│                                                              │
└─────────────────────────────────────────────────────────────┘
```

## Setup

### Prerequisites

- FFmpeg installed
- TTS server (Chatterbox or Hibiki) for TTS
- Optional: Whisper for local STT

### Configuration

```yaml
voice:
  enabled: true
  tts_endpoint: "http://127.0.0.1:7860"  # Chatterbox/Hibiki
  stt_provider: whisper  # whisper or openai
  language: "pt"  # pt, en, es, fr, de, it, hi
```

## TTS Providers

### Chatterbox (Recommended)

Docker-based GPU TTS:

```bash
docker run -d --gpus all -p 7860:7860 ghcr.io/garraia/chatterbox:latest
```

Features:
- Multilingual (pt, en, es, fr, de, it, hi)
- GPU accelerated
- Low latency

### Hibiki

Alternative GPU TTS:

```bash
docker run -d --gpus all -p 7861:7860 ghcr.io/garraia/hibiki:latest
```

### OpenAI TTS

Cloud-based TTS:

```yaml
voice:
  enabled: true
  tts_provider: openai
  tts_model: "tts-1-hd"
  tts_voice: "alloy"
```

## STT Providers

### Local Whisper

```yaml
voice:
  stt_provider: whisper
  whisper_model: "base"  # tiny, base, small, medium, large
```

### OpenAI Whisper API

```yaml
voice:
  stt_provider: openai
  openai_api_key: "sk-..."
```

## Usage

### Starting with Voice

```bash
garraia start --with-voice
```

### Voice Commands

- `/voz` or `/voice` - Toggle voice mode for current session
- Voice responses are automatic when enabled

### Telegram Voice

Send voice messages and receive voice responses automatically when voice mode is enabled.

## API Endpoints

### TTS Endpoint

```bash
curl -X POST http://127.0.0.1:3888/api/tts \
  -H "Content-Type: application/json" \
  -d '{"text": "Hello, how can I help you?", "language": "en"}'
```

Returns audio file (WAV/MP3).

### STT Endpoint

```bash
curl -X POST http://127.0.0.1:3888/api/stt \
  -H "Content-Type: audio/wav" \
  --data-binary @audio.wav
```

Returns transcribed text.

## Health Checks

Voice services are checked at startup:

```bash
garraia health
```

Output includes TTS and STT status.

## Troubleshooting

### TTS not responding

Check TTS server:
```bash
curl http://127.0.0.1:7860/health
```

### Audio quality issues

- Increase TTS quality setting
- Check network latency to TTS server
- Use local TTS (Chatterbox/Hibiki)

### STT errors

- Check FFmpeg installation
- Verify audio format (16kHz mono recommended)
- Try different Whisper model
