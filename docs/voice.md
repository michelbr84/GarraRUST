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

### Wizard integration (plan 0126)

When `garraia init` runs on a machine with an NVIDIA GPU (`nvidia-smi`
detected) and the user opts into voice mode, the wizard pre-fills the
configuration block below and prints the install instructions for both
servers — but it **does not** auto-install the Python TTS/STT stacks.
Run those commands yourself once, then start the gateway with
`garraia start` (no `--with-voice` flag needed when `voice.enabled` is
already `true` in `config.yml`):

```bash
# TTS — Chatterbox Multilingual on :7860
pip install chatterbox-tts
chatterbox-tts serve --host 127.0.0.1 --port 7860

# STT — faster-whisper-server on :9090
pip install faster-whisper-server
fwsh serve --host 127.0.0.1 --port 9090
```

The wizard writes `voice.tts_endpoint=http://127.0.0.1:7860`,
`voice.stt_endpoint=http://127.0.0.1:9090`,
`voice.tts_provider=chatterbox`, and `voice.language=pt` into the
emitted `config.yml`. CPU-only machines skip the voice prompt entirely.

### Configuration

```yaml
voice:
  enabled: true
  tts_endpoint: "http://127.0.0.1:7860"  # Chatterbox/Hibiki
  stt_provider: whisper  # whisper or openai
  language: "pt"  # pt, en, es, fr, de, it, hi
```

## Voice is a local service

Both servers run **on the machine that runs the gateway** — there is no
hosted GarraIA voice endpoint, and no `chatterbox.garraia.org` or
`whisper.garraia.org` to point at (#1099). Every default in
`garraia-config` is a loopback URL (`http://127.0.0.1:7860` / `:9090`)
precisely for that reason. If a config of yours names a public hostname,
it came from somewhere other than this repository.

Because the servers are separate processes, "voice mode is on" and "the
voice servers are up" are different facts — see
[Diagnostics](#diagnostics) for how to tell them apart.

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

## Diagnostics

`GET /api/diagnostics` (and the Diagnostics page of the Web Console)
reports one row per voice server — `voice.tts` and `voice.stt` — each
probed with a 1.5 s budget:

| Row | Status | Meaning |
| --- | --- | --- |
| `voice.tts` / `voice.stt` | `skipped` | Voice mode is off in this process. Nothing is wrong; start with `--with-voice`. |
| | `ok` | The configured endpoint answered. |
| | `error` | Configured but unreachable, or not a URL this gateway may call. The row carries the exact start command as `next_step`. |

This is the answer to "voice fails silently" (#1098): an unreachable
server used to be a log line nobody read, and `POST /api/tts` answered
200 with a text fallback. It is now an `error` row in the console. To get
the failure as an HTTP error instead of the fallback, ask for it
explicitly:

```bash
curl -X POST 'http://127.0.0.1:3888/api/tts?fallback=false' \
  -H 'Content-Type: application/json' \
  -d '{"text": "Hello"}'
```

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
