/**
 * #1754 — Voice Chat contínuo na Web, simulado no navegador.
 *
 * Nada de microfone, alto-falante ou provedor de verdade:
 *  - `getUserMedia` devolve uma track silenciosa de um `AudioContext` próprio
 *    (o AudioWorklet roda de verdade sobre ela);
 *  - a fala entra como PCM simulado pelo seam `window.__garraVoice.chat.onPcm`,
 *    o mesmo caminho dos frames do worklet;
 *  - `/api/voice/*` e `/api/providers` são respondidos por `page.route`, e o
 *    `/ws` inteiro por `page.routeWebSocket` — nenhum turno chega a um LLM.
 *
 * O que se prova: VAD (pre-roll, fim de fala, limite, ruído) e segmentador
 * com PCM simulado; a chamada fala → STT → deltas → TTS por frase; barge-in
 * que para o áudio, cancela o turno com `turn_id`/`heard_chars` e descarta
 * frames atrasados; troca de modelo sem trocar de sessão; silenciar/encerrar
 * com limpeza; erro de STT sem provedor alternativo.
 *
 * A validação física (microfone, eco, alto-falante, provedores reais) NÃO é
 * coberta aqui e fica no checklist manual do PR.
 */

import { test, expect, Page, WebSocketRoute } from '@playwright/test';

test.use({ launchOptions: { args: ['--autoplay-policy=no-user-gesture-required'] } });

/** WAV PCM16 mono 16 kHz com um tom: áudio que o `decodeAudioData` aceita. */
function wavTone(seconds: number): Buffer {
  const rate = 16_000;
  const n = Math.round(seconds * rate);
  const buf = Buffer.alloc(44 + n * 2);
  buf.write('RIFF', 0);
  buf.writeUInt32LE(36 + n * 2, 4);
  buf.write('WAVEfmt ', 8);
  buf.writeUInt32LE(16, 16);
  buf.writeUInt16LE(1, 20);
  buf.writeUInt16LE(1, 22);
  buf.writeUInt32LE(rate, 24);
  buf.writeUInt32LE(rate * 2, 28);
  buf.writeUInt16LE(2, 32);
  buf.writeUInt16LE(16, 34);
  buf.write('data', 36);
  buf.writeUInt32LE(n * 2, 40);
  for (let i = 0; i < n; i++) buf.writeInt16LE(Math.round(Math.sin(i * 0.1) * 6000), 44 + i * 2);
  return buf;
}

const CATALOG = {
  stt: [
    { id: 'configured', provider: 'local', name: 'Whisper local (configurado)', available: true, voices: [], prices: null },
    {
      id: 'mistralai/voxtral-mini-transcribe-3-0', provider: 'voice-router', name: 'Voxtral transcribe', available: true,
      voices: [], prices: [{ field: 'prompt', usd: '0.0000583333', unit: null }],
    },
  ],
  tts: [
    { id: 'configured', provider: 'local', name: 'chatterbox local (configurado)', available: true, voices: [], prices: null },
    {
      id: 'google/gemini-3.8-flash-lite-tts', provider: 'voice-router', name: 'Gemini TTS', available: true,
      voices: ['Kore', 'Puck'], prices: [{ field: 'prompt', usd: '0.0000005', unit: null }],
    },
  ],
  speech_to_speech: [{ id: 'openai/gpt-audio-mini', provider: 'voice-router', available: false, reason: 'No verified realtime speech-to-speech adapter' }],
  limitations: ['Transcrição por fala e síntese por frase.'],
  catalogs: [{ provider: 'voice-router', fetched_at: '2026-11-02T18:16:00Z', ttl_seconds: 300, voice_models: 4 }],
  catalog_ttl_seconds: 300,
  default_language: 'pt',
  llm_fallback_providers: [],
};

const PROVIDERS = {
  instances: [
    { id: 'primeiro', display_name: 'primeiro', active: true, is_default: true, model: 'modelo-a', models: ['modelo-a'] },
    { id: 'segundo', display_name: 'segundo', active: true, is_default: false, model: 'modelo-b', models: ['modelo-b'] },
  ],
};

/** Um `/ws` aberto pela página: o chat de texto abre o dele no boot, a
 * chamada abre outro ao iniciar. */
type Socket = { ws: WebSocketRoute; msgs: Record<string, any>[]; closed: boolean };

type Gravado = {
  transcribes: { bytes: number; riff: boolean; fields: string }[];
  synth: Record<string, unknown>[];
  sockets: Socket[];
  voz: Socket | null;
};

interface Opcoes {
  transcripts?: string[];
  transcribeStatus?: number;
  ttsSeconds?: number;
}

/** Abre `#/voice` com microfone falso, rotas de voz e `/ws` simulados. */
async function abrirVoz(page: Page, opcoes: Opcoes = {}): Promise<Gravado> {
  const gravado: Gravado = { transcribes: [], synth: [], sockets: [], voz: null };
  const transcripts = [...(opcoes.transcripts ?? ['olá garra'])];

  await page.addInitScript(() => {
    const w = window as unknown as Record<string, unknown>;
    const mic = { calls: [] as unknown[], tracks: [] as MediaStreamTrack[], contexts: [] as AudioContext[] };
    w.__fakeMic = mic;
    const Original = window.AudioContext;
    // Registra todo AudioContext criado, para provar que o encerramento fecha.
    class Gravado extends Original {
      constructor(opts?: AudioContextOptions) {
        super(opts);
        mic.contexts.push(this);
      }
    }
    (window as unknown as { AudioContext: typeof AudioContext }).AudioContext = Gravado;
    const fonte = new Original();
    navigator.mediaDevices.getUserMedia = async (constraints?: MediaStreamConstraints) => {
      mic.calls.push(constraints);
      // Track silenciosa: o worklet roda de verdade, a fala entra pelo seam.
      const destino = fonte.createMediaStreamDestination();
      const stream = destino.stream;
      mic.tracks.push(...stream.getTracks());
      return stream;
    };
  });

  await page.route(/\/api\/voice\/models(\?.*)?$/, (route) => route.fulfill({ json: CATALOG }));
  await page.route(/\/api\/providers(\?.*)?$/, (route) => route.fulfill({ json: PROVIDERS }));
  await page.route('**/api/voice/transcribe', async (route) => {
    const body = route.request().postDataBuffer() ?? Buffer.alloc(0);
    const texto = body.toString('latin1');
    gravado.transcribes.push({ bytes: body.length, riff: texto.includes('RIFF'), fields: texto });
    if (opcoes.transcribeStatus) {
      await route.fulfill({ status: opcoes.transcribeStatus, json: { error: 'Voice capacity reached' } });
      return;
    }
    await route.fulfill({ json: { text: transcripts.shift() ?? 'mais uma fala', usage: { cost: 0.0001, seconds: 0.4 } } });
  });
  await page.route('**/api/voice/synthesize', async (route) => {
    gravado.synth.push(JSON.parse(route.request().postData() ?? '{}'));
    await route.fulfill({
      status: 200,
      headers: { 'content-type': 'audio/wav', 'cache-control': 'no-store' },
      body: wavTone(opcoes.ttsSeconds ?? 0.3),
    });
  });
  // Todo `/ws` é simulado: nenhum turno chega a um LLM de verdade.
  await page.routeWebSocket(/\/ws(\?.*)?$/, (ws) => {
    const socket: Socket = { ws, msgs: [], closed: false };
    gravado.sockets.push(socket);
    ws.onMessage((raw) => {
      let msg: Record<string, any>;
      try {
        msg = JSON.parse(String(raw));
      } catch {
        return;
      }
      socket.msgs.push(msg);
      if (msg.type === 'init' || msg.type === 'resume') {
        ws.send(JSON.stringify({ type: 'connected', session_id: 'sessao-voz', session_token: 'tok-voz' }));
      } else if (msg.type === 'stop') {
        ws.send(JSON.stringify({ type: 'stopped', session_id: 'sessao-voz', ...(msg.turn_id ? { turn_id: msg.turn_id } : {}) }));
      }
    });
    ws.onClose(() => {
      socket.closed = true;
    });
  });

  await page.goto('/#/voice');
  await page.waitForFunction(() => Boolean((window as unknown as { __garraVoice?: { chat?: unknown } }).__garraVoice?.chat), null, {
    timeout: 10_000,
  });
  // Se o roteador da página ainda não ativou a view, carrega o catálogo direto.
  await page.evaluate(() => {
    const chat = (window as unknown as { __garraVoice: { chat: { modelsLoaded?: boolean; loadModels: () => Promise<void> } } }).__garraVoice.chat;
    if (!chat.modelsLoaded) return chat.loadModels();
  });
  await expect(page.locator('#voice-start')).toBeEnabled();
  return gravado;
}

/** Fala simulada: `falaMs` de tom acima do limiar e `silencioMs` de silêncio. */
async function falar(page: Page, falaMs: number, silencioMs: number) {
  await page.evaluate(
    ({ falaMs, silencioMs }) => {
      const chat = (window as unknown as { __garraVoice: { chat: { onPcm: (d: { samples: Float32Array; rms: number }) => void } } }).__garraVoice.chat;
      const frame = (amp: number) => {
        const samples = new Float32Array(320);
        for (let i = 0; i < 320; i++) samples[i] = amp * Math.sin(i * 0.3);
        return { samples, rms: amp / Math.SQRT2 };
      };
      for (let t = 0; t < falaMs; t += 20) chat.onPcm(frame(0.2));
      for (let t = 0; t < silencioMs; t += 20) chat.onPcm(frame(0));
    },
    { falaMs, silencioMs },
  );
}

/** Inicia a chamada; o socket aberto a partir daqui é o da voz. */
async function iniciar(page: Page, g: Gravado) {
  const antes = g.sockets.length;
  await page.locator('#voice-start').click();
  await expect(page.locator('#voice-stage')).toHaveAttribute('data-state', 'listening', { timeout: 10_000 });
  expect(g.sockets.length).toBe(antes + 1);
  g.voz = g.sockets[antes];
}

function doSocketDeVoz(g: Gravado, filtro: (m: Record<string, any>) => boolean) {
  return (g.voz?.msgs ?? []).filter(filtro);
}

function mensagensDeVoz(g: Gravado) {
  return doSocketDeVoz(g, (m) => m.voice === true);
}

function stops(g: Gravado) {
  return doSocketDeVoz(g, (m) => m.type === 'stop');
}

function aberturas(g: Gravado) {
  return doSocketDeVoz(g, (m) => m.type === 'init' || m.type === 'resume');
}

async function esperarTurno(g: Gravado, n: number) {
  await expect.poll(() => mensagensDeVoz(g).length, { timeout: 10_000 }).toBe(n);
  return mensagensDeVoz(g)[n - 1] as { turn_id: string; content: string; provider: string; model?: string };
}

function responder(g: Gravado, frame: Record<string, unknown>) {
  g.voz!.ws.send(JSON.stringify({ session_id: 'sessao-voz', ...frame }));
}

test.describe('#1754 — VAD e segmentador com PCM simulado', () => {
  test('silêncio, clique, fala curta, pausas, limite de 30 s e ruído', async ({ page }) => {
    await abrirVoz(page);
    const r = await page.evaluate(() => {
      const { VoiceActivityDetector } = (window as unknown as { __garraVoice: { VoiceActivityDetector: new (o?: object) => any } }).__garraVoice;
      const frame = (amp: number) => {
        const s = new Float32Array(320);
        for (let i = 0; i < 320; i++) s[i] = amp * Math.sin(i * 0.3);
        return { s, rms: amp / Math.SQRT2 };
      };
      const run = (vad: any, script: [number, number][]) => {
        const out: { started: number; utterances: { ms: number; limited: boolean; abreEmSilencio: boolean }[] } = { started: 0, utterances: [] };
        for (const [amp, ms] of script) {
          for (let t = 0; t < ms; t += 20) {
            const f = frame(amp);
            const ev = vad.push(f.s, f.rms);
            if (ev.started) out.started++;
            if (ev.utterance) {
              out.utterances.push({
                ms: ev.utterance.length * 20,
                limited: ev.limited,
                abreEmSilencio: ev.utterance[0].every((v: number) => v === 0),
              });
            }
          }
        }
        return out;
      };
      const silencio = run(new VoiceActivityDetector(), [[0, 3000]]);
      const clique = run(new VoiceActivityDetector(), [[0, 500], [0.3, 60], [0, 2000]]);
      const curta = run(new VoiceActivityDetector(), [[0, 500], [0.2, 300], [0, 1000]]);
      const pausas = run(new VoiceActivityDetector({ silenceMs: 800 }), [[0.2, 300], [0, 400], [0.2, 300], [0, 1000]]);
      const longa = run(new VoiceActivityDetector(), [[0.2, 31_000], [0, 1000]]);
      // Piso de ruído: 3 s de ruído logo abaixo do limiar inicial sobem o piso,
      // e uma "fala" que antes dispararia deixa de disparar.
      const adaptado = new VoiceActivityDetector();
      run(adaptado, [[0.016, 3000]]);
      const fracaDepoisDoRuido = run(adaptado, [[0.024, 600], [0, 1000]]);
      const forteDepoisDoRuido = run(adaptado, [[0.3, 600], [0, 1000]]);
      return { silencio, clique, curta, pausas, longa, fracaDepoisDoRuido, forteDepoisDoRuido, piso: adaptado.threshold() };
    });
    expect(r.silencio).toEqual({ started: 0, utterances: [] });
    expect(r.clique).toEqual({ started: 0, utterances: [] });
    expect(r.curta.started).toBe(1);
    expect(r.curta.utterances).toHaveLength(1);
    // Pre-roll de 12 frames (240 ms: 120 ms de silêncio + os 120 ms de fala
    // que disparam o início), o resto da fala (180 ms) e 800 ms até fechar.
    expect(r.curta.utterances[0]).toEqual({ ms: 1220, limited: false, abreEmSilencio: true });
    // Pausa menor que o fim de fala não corta a frase.
    expect(r.pausas.started).toBe(1);
    expect(r.pausas.utterances).toHaveLength(1);
    // 31 s de fala: um trecho de 30 s (limitado) e o resto em seguida.
    expect(r.longa.utterances[0]).toMatchObject({ ms: 30_000, limited: true });
    expect(r.longa.utterances).toHaveLength(2);
    expect(r.piso).toBeGreaterThan(0.012);
    expect(r.fracaDepoisDoRuido.started).toBe(0);
    expect(r.forteDepoisDoRuido.started).toBe(1);
  });

  test('segmentador por frase, texto falável, WAV e preço sem unidade inventada', async ({ page }) => {
    await abrirVoz(page);
    const r = await page.evaluate(async () => {
      const v = (window as unknown as { __garraVoice: any }).__garraVoice;
      const wav = new DataView(await v.encodeWav([new Float32Array(320), new Float32Array(320)]).arrayBuffer());
      return {
        frase: v.takePhrase('Olá. Tudo bem? Sim'),
        incompleta: v.takePhrase('sem ponto ainda'),
        final: v.takePhrase('sem ponto ainda', true),
        longa: v.takePhrase('palavra '.repeat(60)),
        codigo: v.speakable('Veja ```rm -rf /``` e [o link](https://x.example) **agora**'),
        emoji: v.codePointLength('🦀a'),
        wav: { riff: wav.getUint32(0, false), rate: wav.getUint32(24, true), canais: wav.getUint16(22, true), dados: wav.getUint32(40, true), tamanho: wav.byteLength },
        precoDesconhecido: v.describePrices({ provider: 'x', prices: [{ field: 'prompt', usd: '0.000013', unit: null }] }),
        precoZero: v.describePrices({ provider: 'x', prices: [{ field: 'prompt', usd: '0', unit: null }] }),
        semPreco: v.describePrices({ provider: 'x', prices: null }),
      };
    });
    expect(r.frase).toEqual(['Olá.', 'Tudo bem? Sim']);
    expect(r.incompleta).toEqual(['', 'sem ponto ainda']);
    expect(r.final).toEqual(['sem ponto ainda', '']);
    expect(r.longa[0].length).toBeLessThanOrEqual(240);
    expect(r.codigo).toBe('Veja  trecho de código na transcrição  e o link agora');
    expect(r.emoji).toBe(2);
    expect(r.wav).toEqual({ riff: 0x52494646, rate: 16_000, canais: 1, dados: 1280, tamanho: 1324 });
    expect(r.precoDesconhecido).toContain('unidade não informada');
    expect(r.precoDesconhecido).not.toMatch(/token/i);
    expect(r.precoZero).toContain('não garante gratuidade');
    expect(r.semPreco).toContain('não informado');
  });
});

test.describe('#1754 — chamada simulada', () => {
  test('fala → STT → deltas → TTS por frase, com quem respondeu e custo informado', async ({ page }) => {
    const g = await abrirVoz(page);
    await expect(page.locator('#voice-catalog-time')).toContainText('voice-router');
    await expect(page.locator('#voice-limitations')).toContainText('speech-to-speech');
    await iniciar(page, g);

    const mic = await page.evaluate(() => (window as unknown as { __fakeMic: { calls: any[] } }).__fakeMic.calls);
    expect(mic).toHaveLength(1);
    expect(mic[0].audio.echoCancellation).toBe(true);
    expect(aberturas(g)).toEqual([{ type: 'init' }]);

    await falar(page, 400, 1000);
    const turno = await esperarTurno(g, 1);
    expect(g.transcribes).toHaveLength(1);
    expect(g.transcribes[0].riff).toBe(true);
    expect(g.transcribes[0].fields).toContain('name="provider"\r\n\r\nlocal');
    expect(g.transcribes[0].fields).toContain('name="language"\r\n\r\npt');
    expect(turno).toMatchObject({ content: 'olá garra', provider: 'primeiro', model: 'modelo-a', stream: true });
    expect(turno.turn_id).toMatch(/^[A-Za-z0-9_-]{1,64}$/);

    responder(g, { type: 'delta', turn_id: turno.turn_id, content: 'Primeira frase. ' });
    responder(g, { type: 'delta', turn_id: turno.turn_id, content: 'Segunda frase.' });
    responder(g, {
      type: 'message', turn_id: turno.turn_id, content: 'Primeira frase. Segunda frase.',
      provider: 'primeiro', model: 'modelo-a', fallback: false,
    });

    await expect.poll(() => g.synth.map((s) => s.text), { timeout: 10_000 }).toEqual(['Primeira frase.', 'Segunda frase.']);
    expect(g.synth[0]).toMatchObject({ provider: 'local', model: 'configured', language: 'pt' });
    await expect(page.locator('#voice-stage')).toHaveAttribute('data-state', 'listening', { timeout: 10_000 });
    await expect(page.getByTestId('voice-served-by')).toContainText('primeiro · modelo-a');
    await expect(page.locator('#voice-cost')).toContainText('US$ 0.000100');
    await expect(page.locator('#voice-first-audio')).toHaveText(/\d+ ms/);
    const transcricao = page.locator('#voice-transcript');
    await expect(transcricao).toContainText('olá garra');
    await expect(transcricao).toContainText('Primeira frase. Segunda frase.');
  });

  test('barge-in para o áudio, cancela o turno com turn_id e descarta frames atrasados', async ({ page }) => {
    const g = await abrirVoz(page, { transcripts: ['conte uma história', 'mais curto'], ttsSeconds: 3 });
    await iniciar(page, g);
    await falar(page, 400, 1000);
    const primeiro = await esperarTurno(g, 1);
    responder(g, { type: 'delta', turn_id: primeiro.turn_id, content: 'Era uma vez um gateway. ' });
    await expect(page.locator('#voice-stage')).toHaveAttribute('data-state', 'speaking', { timeout: 10_000 });

    // O usuário fala por cima: só o início da fala (o enunciado ainda não fechou).
    await falar(page, 200, 0);
    await expect.poll(() => stops(g).length, { timeout: 5_000 }).toBe(1);
    const stop = stops(g)[0] as { turn_id: string; heard_chars: number };
    expect(stop.turn_id).toBe(primeiro.turn_id);
    expect(typeof stop.heard_chars).toBe('number');
    const estado = await page.evaluate(() => {
      const c = (window as unknown as { __garraVoice: { chat: any } }).__garraVoice.chat;
      return { tocando: Boolean(c.source), fila: c.audioQueue.length + c.textQueue.length, status: c.status };
    });
    expect(estado).toEqual({ tocando: false, fila: 0, status: 'user-speaking' });
    await expect(page.locator('#voice-transcript .voice-turn[data-interrupted="true"]')).toHaveCount(1);

    // Frames atrasados do turno velho não viram áudio nem texto.
    const sinteses = g.synth.length;
    responder(g, { type: 'delta', turn_id: primeiro.turn_id, content: 'Atrasado demais. ' });
    responder(g, { type: 'message', turn_id: primeiro.turn_id, content: 'Era uma vez um gateway. Atrasado demais.' });
    await page.waitForTimeout(300);
    expect(g.synth.length).toBe(sinteses);
    await expect(page.locator('#voice-transcript')).not.toContainText('Atrasado demais');

    // Fecha a nova fala: novo turno, novo turn_id, mesma sessão.
    await falar(page, 0, 1000);
    const segundo = await esperarTurno(g, 2);
    expect(segundo.content).toBe('mais curto');
    expect(segundo.turn_id).not.toBe(primeiro.turn_id);
    expect(aberturas(g)).toHaveLength(1);
  });

  test('trocar LLM, TTS e voz no meio da chamada mantém a sessão', async ({ page }) => {
    const g = await abrirVoz(page, { transcripts: ['primeira', 'segunda'] });
    await iniciar(page, g);
    await falar(page, 400, 1000);
    const primeiro = await esperarTurno(g, 1);
    responder(g, { type: 'message', turn_id: primeiro.turn_id, content: 'Ok.', provider: 'primeiro' });
    await expect.poll(() => g.synth.length, { timeout: 10_000 }).toBe(1);
    await expect(page.locator('#voice-stage')).toHaveAttribute('data-state', 'listening', { timeout: 10_000 });

    await page.locator('#voice-llm').selectOption('segundo');
    await page.locator('#voice-tts').selectOption(JSON.stringify(['voice-router', 'google/gemini-3.8-flash-lite-tts']));
    await expect(page.locator('#voice-voice')).toHaveValue('Kore');
    await page.locator('#voice-voice').selectOption('Puck');
    await expect(page.locator('#voice-config-status')).toContainText('contexto da chamada será mantido');
    await expect(page.locator('#voice-pricing')).toContainText('unidade não informada');

    await falar(page, 400, 1000);
    const segundo = await esperarTurno(g, 2);
    expect(segundo).toMatchObject({ provider: 'segundo', model: 'modelo-b', content: 'segunda' });
    responder(g, { type: 'message', turn_id: segundo.turn_id, content: 'Certo.', provider: 'segundo', fallback: true });
    await expect.poll(() => g.synth.length, { timeout: 10_000 }).toBe(2);
    expect(g.synth[1]).toMatchObject({ provider: 'voice-router', model: 'google/gemini-3.8-flash-lite-tts', voice: 'Puck' });
    // Um único socket, um único init: a troca não recriou a sessão.
    expect(aberturas(g)).toHaveLength(1);
    await expect(page.getByTestId('voice-served-by')).toContainText('fallback configurado pelo operador');
  });

  test('silenciar ignora o microfone e encerrar libera track, AudioContext e socket', async ({ page }) => {
    const g = await abrirVoz(page);
    await iniciar(page, g);
    await page.locator('#voice-mute').click();
    await expect(page.locator('#voice-stage')).toHaveAttribute('data-state', 'muted');
    await falar(page, 400, 1000);
    await page.waitForTimeout(300);
    expect(g.transcribes).toHaveLength(0);
    expect(await page.evaluate(() => (window as unknown as { __fakeMic: { tracks: MediaStreamTrack[] } }).__fakeMic.tracks.every((t) => !t.enabled))).toBe(true);
    await page.locator('#voice-mute').click();
    await expect(page.locator('#voice-stage')).toHaveAttribute('data-state', 'listening');

    await page.locator('#voice-end').click();
    await expect(page.locator('#voice-stage')).toHaveAttribute('data-state', 'idle');
    const limpeza = () =>
      page.evaluate(() => {
        const mic = (window as unknown as { __fakeMic: { tracks: MediaStreamTrack[]; contexts: AudioContext[] } }).__fakeMic;
        const chat = (window as unknown as { __garraVoice: { chat: any } }).__garraVoice.chat;
        return {
          tracks: mic.tracks.map((t) => t.readyState),
          contextos: mic.contexts.map((c) => c.state),
          socket: chat.socket === null,
          ativa: document.body.classList.contains('voice-call-active'),
        };
      });
    // `AudioContext.close()` é assíncrono.
    await expect.poll(limpeza, { timeout: 5_000 }).toEqual({
      tracks: ['ended'],
      contextos: ['closed'],
      socket: true,
      ativa: false,
    });
    await expect.poll(() => g.voz?.closed, { timeout: 5_000 }).toBe(true);
    await expect(page.getByTestId('voice-new-session')).toBeEnabled();
  });

  test('falha de STT aparece e nenhum provedor alternativo é acionado', async ({ page }) => {
    const g = await abrirVoz(page, { transcribeStatus: 429 });
    await iniciar(page, g);
    await falar(page, 400, 1000);
    await expect(page.locator('#voice-error')).toContainText('capacidade de voz', { timeout: 10_000 });
    expect(g.transcribes).toHaveLength(1);
    expect(mensagensDeVoz(g)).toHaveLength(0);
    expect(g.synth).toHaveLength(0);
    await expect(page.locator('#voice-stage')).toHaveAttribute('data-state', 'listening');
  });
});
