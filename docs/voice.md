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
# TTS — Chatterbox Multilingual (Gradio app) on :7860
# The `chatterbox-tts` wheel is a *library* and ships no CLI — the server
# is the Gradio app that lives in the upstream repository.
git clone https://github.com/resemble-ai/chatterbox
cd chatterbox
pip install -e .
GRADIO_SERVER_NAME=127.0.0.1 GRADIO_SERVER_PORT=7860 python multilingual_app.py

# STT — whisper.cpp server on :9090
git clone https://github.com/ggml-org/whisper.cpp
cd whisper.cpp
cmake -B build && cmake --build build -j --config Release
./models/download-ggml-model.sh base
./build/bin/whisper-server --host 127.0.0.1 --port 9090 -m models/ggml-base.bin
```

On older whisper.cpp checkouts the server binary is called `./server`
instead of `./build/bin/whisper-server`. Any other server that exposes
the OpenAI-compatible `POST /v1/audio/transcriptions` route works too:
the client tries whisper.cpp's `POST /inference` first and falls back to
the OpenAI shape.

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

Local GPU TTS served by the upstream Gradio app — GarraIA publishes no
container image for it, so run it from the source checkout:

```bash
git clone https://github.com/resemble-ai/chatterbox
cd chatterbox
pip install -e .
GRADIO_SERVER_NAME=127.0.0.1 GRADIO_SERVER_PORT=7860 python multilingual_app.py
```

GarraIA talks to it over the Gradio API
(`POST /gradio_api/call/generate_tts_audio`), which is what
`multilingual_app.py` exposes. The `chatterbox-tts` PyPI wheel is a
library only and has no `serve` subcommand.

Features:
- Multilingual (pt, en, es, fr, de, it, hi)
- GPU accelerated
- Low latency

### Hibiki

Alternative GPU TTS. There is no published GarraIA image for it either —
follow the upstream project's own instructions and point
`voice.tts_endpoint` at whatever host and port you start it on.

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
| | `error` (unreachable) | Configured but did not answer — connection refused or timeout. The row carries the exact start command as `next_step`. |
| | `error` (unhealthy) | Answered with HTTP 5xx: the server is up but the service is broken. `next_step` says the server's own logs are the next stop. |
| | `error` (invalid URL) | The configured endpoint is not a URL this gateway may call. `next_step` is a generic instruction to fix the endpoint in the config — there is no start command for a URL that is not valid. |

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
