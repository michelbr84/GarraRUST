// Continuous voice in the existing gateway console (#1754, ADR 0027).
//
// Provider credentials and destinations stay on the gateway: this module only
// knows the gateway key the console already holds and public catalog IDs.
// Pipeline: AudioWorklet PCM -> energy VAD -> WAV per utterance -> STT ->
// /ws turn (voice, turn_id) -> deltas -> phrases -> TTS -> playback.
const SAMPLE_RATE = 16000;
const MAX_IDLE_MS = 5 * 60 * 1000;
const MAX_CALL_MS = 60 * 60 * 1000;
const MAX_AUDIO_QUEUE = 8;
const STOP_ACK_MS = 5000;
const STATES = {
  idle: 'Pronto para conversar', connecting: 'Conectando…', listening: 'Estou escutando',
  'user-speaking': 'Pode falar…', processing: 'Pensando…', speaking: 'GarraIA está falando',
  muted: 'Microfone silenciado', reconnecting: 'Reconectando…', error: 'Chamada encerrada',
};
const SESSION_KEY = 'garraia.voice.session_id';
const TOKEN_KEY = 'garraia.voice.session_token';

const store = {
  get(key) { try { return localStorage.getItem(key) || ''; } catch { return ''; } },
  set(key, value) { try { localStorage.setItem(key, value); } catch { /* private mode */ } },
  remove(key) { try { localStorage.removeItem(key); } catch { /* private mode */ } },
};

/**
 * Energy VAD driven by frame duration (no wall-clock timers that drift in
 * background tabs): adaptive noise floor, 120 ms to start, pre-roll so the
 * first syllable is not clipped, hysteresis on release, configurable end of
 * speech and a hard limit per utterance.
 */
export class VoiceActivityDetector {
  constructor({ silenceMs = 800, maxSpeechMs = 30000, minSpeechMs = 120, preRollFrames = 12 } = {}) {
    this.silenceMs = silenceMs;
    this.maxSpeechMs = maxSpeechMs;
    this.minSpeechMs = minSpeechMs;
    this.preRollFrames = preRollFrames;
    this.noise = .002;
    this.reset();
  }
  reset() {
    this.active = false;
    this.preRoll = [];
    this.frames = [];
    this.candidateMs = this.quietMs = this.durationMs = 0;
  }
  threshold() { return Math.max(.012, this.noise * 3.5); }
  push(samples, rms) {
    const ms = samples.length / SAMPLE_RATE * 1000;
    const threshold = this.threshold();
    if (!this.active) {
      this.preRoll.push(samples);
      while (this.preRoll.length > this.preRollFrames) this.preRoll.shift();
      if (rms < threshold) this.noise = this.noise * .98 + rms * .02;
      this.candidateMs = rms >= threshold ? this.candidateMs + ms : 0;
      if (this.candidateMs < this.minSpeechMs) return {};
      this.active = true;
      this.frames = this.preRoll.slice();
      this.preRoll = [];
      this.durationMs = this.frames.reduce((n, f) => n + f.length / SAMPLE_RATE * 1000, 0);
      return { started: true };
    }
    this.frames.push(samples);
    this.durationMs += ms;
    this.quietMs = rms >= threshold * .7 ? 0 : this.quietMs + ms;
    if (this.quietMs < this.silenceMs && this.durationMs < this.maxSpeechMs) return {};
    const utterance = this.frames;
    const limited = this.durationMs >= this.maxSpeechMs;
    this.reset();
    return { utterance, limited };
  }
}

/** PCM16 mono 16 kHz WAV, the only upload format the gateway accepts. */
export function encodeWav(frames) {
  const samples = frames.reduce((n, frame) => n + frame.length, 0);
  const buffer = new ArrayBuffer(44 + samples * 2);
  const view = new DataView(buffer);
  const ascii = (offset, value) => [...value].forEach((char, i) => view.setUint8(offset + i, char.charCodeAt(0)));
  ascii(0, 'RIFF'); view.setUint32(4, 36 + samples * 2, true); ascii(8, 'WAVE');
  ascii(12, 'fmt '); view.setUint32(16, 16, true); view.setUint16(20, 1, true);
  view.setUint16(22, 1, true); view.setUint32(24, SAMPLE_RATE, true);
  view.setUint32(28, SAMPLE_RATE * 2, true); view.setUint16(32, 2, true); view.setUint16(34, 16, true);
  ascii(36, 'data'); view.setUint32(40, samples * 2, true);
  let offset = 44;
  for (const frame of frames) for (const value of frame) {
    const sample = Math.max(-1, Math.min(1, value));
    view.setInt16(offset, sample * (sample < 0 ? 32768 : 32767), true); offset += 2;
  }
  return new Blob([buffer], { type: 'audio/wav' });
}

/** Split at complete phrases, retaining a trailing fragment until more text arrives. */
export function takePhrase(text, final = false) {
  const sentence = /[.!?。！？](?:["'”’)]*)\s+|\n/.exec(text);
  if (sentence) {
    const end = sentence.index + sentence[0].length;
    if (end <= 420) return [text.slice(0, end).trim(), text.slice(end)];
  }
  if (text.length > 240) {
    const boundary = text.lastIndexOf(' ', 240);
    const end = boundary > 80 ? boundary + 1 : 240;
    return [text.slice(0, end).trim(), text.slice(end)];
  }
  return final && text.trim() ? [text.trim(), ''] : ['', text];
}

/** What is read aloud: Markdown decoration and code stay in the transcript. */
export function speakable(phrase) {
  return phrase
    .replace(/```[\s\S]*?```/g, ' trecho de código na transcrição ')
    .replace(/\[([^\]]+)\]\([^)]+\)/g, '$1')
    .replace(/[*#`_]/g, '')
    .trim();
}

/** Unicode scalar count: the unit the gateway uses for `heard_chars`. */
export function codePointLength(text) { return [...text].length; }

/** Price as the catalog states it; the unit is never assumed (ADR 0027). */
export function describePrices(model) {
  if (!model) return '';
  if (model.provider === 'local') return 'servidor local configurado, sem tarifa informada';
  const prices = Array.isArray(model.prices) ? model.prices : [];
  if (!prices.length) return 'preço não informado pelo catálogo';
  if (prices.every(p => Number(p.usd) === 0)) return 'preço zero informado (não garante gratuidade de limites ou de outras etapas)';
  return prices.filter(p => Number(p.usd) > 0)
    .map(p => `${p.field} US$ ${p.usd} ${p.unit ? 'por ' + p.unit : '(unidade não informada)'}`).join('; ');
}

export function newTurnId(seq) { return `v${Date.now().toString(36)}-${seq}`; }

const modelKey = model => JSON.stringify([model.provider, model.id]);
const selectedModel = (list, select) => list.find(model => modelKey(model) === select.value);
const option = (value, label, disabled = false) => {
  const el = document.createElement('option'); el.value = value; el.textContent = label; el.disabled = disabled; return el;
};
const errorText = error => error?.name === 'NotAllowedError'
  ? 'Permita o acesso ao microfone nas configurações do navegador e tente novamente.'
  : error?.name === 'NotFoundError' ? 'Nenhum microfone foi encontrado. Conecte um dispositivo e tente novamente.'
    : error?.name === 'NotReadableError' ? 'O microfone está indisponível ou em uso por outro aplicativo.'
      : String(error?.message || error || 'Falha de áudio.');

export class VoiceChat {
  constructor(root) {
    this.root = root;
    this.el = id => document.getElementById('voice-' + id);
    this.vad = new VoiceActivityDetector();
    this.catalog = { stt: [], tts: [] };
    this.active = this.ready = this.muted = this.turnActive = false;
    this.generation = this.lifecycle = this.retries = this.turnSeq = this.pendingUtterances = 0;
    this.textQueue = []; this.audioQueue = [];
    this.ttsControllers = new Set(); this.sttControllers = new Set();
    this.sttChain = Promise.resolve(); this.carryText = '';
    this.turnId = ''; this.stopTurnId = '';
    this.status = 'idle'; this.startedAt = 0; this.lastActivity = 0; this.sttCost = 0;
    this.session = store.get(SESSION_KEY);
    this.sessionToken = store.get(TOKEN_KEY);
    this.el('start').addEventListener('click', () => this.start());
    this.el('end').addEventListener('click', () => this.end('Chamada encerrada.'));
    this.el('mute').addEventListener('click', () => this.toggleMute());
    this.el('interrupt').addEventListener('click', () => this.interrupt());
    this.el('new-session').addEventListener('click', () => this.newConversation());
    this.el('transcript-toggle').addEventListener('change', event => { this.el('transcript').hidden = !event.target.checked; });
    this.el('refresh-models').addEventListener('click', () => this.loadModels());
    for (const id of ['stt', 'tts', 'llm', 'llm-model', 'language', 'voice', 'silence']) {
      this.el(id).addEventListener('change', () => {
        if (id === 'tts') this.updateVoices();
        if (id === 'stt') this.updatePricing();
        if (id === 'llm') this.updateLlmModels();
        this.el('config-status').textContent = this.active
          ? 'Alterações serão aplicadas na próxima fala. O contexto da chamada será mantido.' : 'Configuração pronta para a próxima chamada.';
        this.renderState(this.status);
      });
    }
    window.addEventListener('pagehide', () => this.end());
    document.addEventListener('visibilitychange', () => {
      clearTimeout(this.hiddenTimer);
      if (document.hidden && this.active) this.hiddenTimer = setTimeout(() => this.end('Chamada encerrada após 30 segundos em segundo plano.'), 30000);
    });
    // The catalog is fetched only when the page is opened: no provider
    // discovery on every console load.
    document.addEventListener('garra:page-change', event => {
      if (event.detail === 'voice' && !this.modelsLoaded) this.loadModels();
    });
    this.renderState('idle');
    if (root.classList.contains('active')) this.loadModels();
  }

  async api(path, init = {}) {
    const headers = new Headers(init.headers);
    const key = store.get('garraia.gateway_key');
    if (key) headers.set('Authorization', 'Bearer ' + key);
    const response = await fetch(path, { ...init, headers });
    if (!response.ok) {
      const body = await response.json().catch(() => ({}));
      if (response.status === 401) throw new Error('Informe a chave do gateway no painel de conexão do Chat e atualize os modelos.');
      if (response.status === 429) throw new Error('A capacidade de voz do gateway está ocupada. Aguarde um instante e fale de novo.');
      throw new Error(body.error?.message || body.error || body.message || `Falha no gateway (${response.status}).`);
    }
    return response;
  }

  async loadModels() {
    if (this.loadingModels) return;
    this.loadingModels = true;
    this.el('refresh-models').disabled = true;
    try {
      const [catalog, providers] = await Promise.all([
        this.api('/api/voice/models').then(r => r.json()), this.api('/api/providers').then(r => r.json()),
      ]);
      this.catalog = { ...catalog, stt: catalog.stt || [], tts: catalog.tts || [] };
      for (const kind of ['stt', 'tts']) {
        const select = this.el(kind); const old = select.value;
        select.replaceChildren();
        for (const model of this.catalog[kind]) {
          const item = option(modelKey(model), `${model.name || model.id} · ${model.provider}${model.available ? '' : ' (indisponível)'}`, !model.available);
          item.title = model.reason || ''; select.append(item);
        }
        if (this.catalog[kind].some(m => m.available && modelKey(m) === old)) select.value = old;
        else select.value = this.catalog[kind].filter(m => m.available).map(modelKey)[0] || '';
        if (!select.value) select.replaceChildren(option('', `Nenhum modelo ${kind.toUpperCase()} disponível`));
      }
      this.providers = (providers.instances || providers.providers || []).filter(p => p.active);
      const oldProvider = this.el('llm').value || store.get('garraia.provider');
      this.el('llm').replaceChildren(...this.providers.map(p => option(p.id, p.display_name || p.id)));
      if (this.providers.some(p => p.id === oldProvider)) this.el('llm').value = oldProvider;
      else this.el('llm').value = this.providers.find(p => p.is_default)?.id || this.providers[0]?.id || '';
      this.updateLlmModels(); this.updateVoices();
      this.modelsLoaded = true;
      const languages = new Set(['pt', 'en', 'es', 'fr', 'de', 'it', 'ja']);
      if (catalog.default_language) languages.add(catalog.default_language);
      for (const model of [...this.catalog.stt, ...this.catalog.tts]) for (const language of model.languages || []) {
        const id = typeof language === 'string' ? language : language.id || language.code;
        if (id) languages.add(id);
      }
      const previousLanguage = this.el('language').value || catalog.default_language || 'pt';
      this.el('language').replaceChildren(...[...languages].map(id => option(id, id)));
      this.el('language').value = previousLanguage;
      const unavailable = !this.canStart();
      const reasons = [...this.catalog.stt, ...this.catalog.tts].filter(m => !m.available).map(m => m.reason).filter(Boolean);
      this.el('availability').textContent = unavailable
        ? 'Configure STT, TTS e um provedor de conversa para iniciar. ' + [...new Set(reasons)].join(' ')
        : 'Áudio enviado somente durante a chamada aos provedores selecionados. Nenhuma troca automática para serviços pagos.';
      const limitations = (catalog.limitations || []).map(item => typeof item === 'string' ? item : item.message || item.reason || '');
      const s2s = catalog.speech_to_speech || [];
      if (s2s.length) limitations.push(`${s2s.length} modelo(s) speech-to-speech no catálogo, indisponíveis: sem adaptador verificado.`);
      this.el('limitations').textContent = limitations.filter(Boolean).join(' ');
      const ttl = Math.round((catalog.catalog_ttl_seconds || 300) / 60);
      const fetched = (catalog.catalogs || []).filter(c => c.fetched_at)
        .map(c => `${c.provider} às ${new Date(c.fetched_at).toLocaleTimeString()}`);
      this.el('catalog-time').textContent = fetched.length
        ? `Catálogo consultado: ${fetched.join(', ')} (reconsultado após ${ttl} min).`
        : 'Somente modelos locais configurados no servidor.';
      this.el('error').textContent = '';
    } catch (error) { this.showError(error); }
    finally { this.loadingModels = false; this.el('refresh-models').disabled = false; this.renderState(this.status); }
  }

  updateLlmModels() {
    const provider = (this.providers || []).find(p => p.id === this.el('llm').value);
    this.el('llm-options').replaceChildren(...(provider?.models || []).map(model => option(model, model)));
    this.el('llm-model').value = provider?.model || provider?.models?.[0] || '';
  }
  updateVoices() {
    const model = selectedModel(this.catalog.tts, this.el('tts'));
    const previous = this.el('voice').value;
    const voices = (model?.voices || [])
      .map(v => typeof v === 'string' ? { id: v, name: v } : { id: v?.id, name: v?.name || v?.id })
      .filter(v => v.id);
    if (voices.length) {
      // A model that advertises voices requires one: no "default" to send.
      this.el('voice').replaceChildren(...voices.map(v => option(v.id, v.name)));
      this.el('voice').value = voices.some(v => v.id === previous) ? previous : voices[0].id;
      this.el('voice').disabled = false;
    } else {
      this.el('voice').replaceChildren(option('', 'Padrão do modelo'));
      this.el('voice').disabled = true;
    }
    this.updatePricing();
  }
  updatePricing() {
    const stt = selectedModel(this.catalog.stt, this.el('stt'));
    const tts = selectedModel(this.catalog.tts, this.el('tts'));
    this.el('pricing').textContent = (stt || tts)
      ? `Ouvir: ${describePrices(stt) || '—'}. Responder: ${describePrices(tts) || '—'}. Conversa: tarifa do provedor de texto. São metadados do catálogo, não o custo desta chamada.`
      : 'Custo estimado e efetivo: indisponíveis.';
  }
  canStart() {
    return Boolean(selectedModel(this.catalog.stt, this.el('stt'))?.available
      && selectedModel(this.catalog.tts, this.el('tts'))?.available && this.el('llm').value);
  }
  settings() {
    return {
      stt: selectedModel(this.catalog.stt, this.el('stt')), tts: selectedModel(this.catalog.tts, this.el('tts')),
      provider: this.el('llm').value, model: this.el('llm-model').value.trim(),
      language: this.el('language').value, voice: this.el('voice').value,
    };
  }
  isPlayingAudio() {
    return Boolean(this.source || this.audioQueue.length || this.textQueue.length || this.synthesizing);
  }
  renderState(state) {
    this.status = state;
    this.el('stage').dataset.state = state;
    this.el('state').textContent = STATES[state] || state;
    this.el('start').disabled = this.active || !this.canStart() || this.loadingModels;
    this.el('end').disabled = !this.active;
    this.el('new-session').disabled = this.active;
    this.el('mute').disabled = !this.active || !this.ready;
    this.el('interrupt').disabled = !this.active || !(this.turnActive || this.isPlayingAudio());
    this.el('mute').textContent = this.muted ? 'Ativar microfone' : 'Silenciar';
    this.el('mute').setAttribute('aria-pressed', String(this.muted));
    this.el('microphone').textContent = !this.active ? 'Microfone desligado' : this.muted ? 'Microfone silenciado' : this.ready ? 'Microfone ligado' : 'Preparando microfone';
    document.body.classList.toggle('voice-call-active', this.active);
  }
  restingState() { return this.muted ? 'muted' : this.vad.active ? 'user-speaking' : 'listening'; }
  showError(error) { this.el('error').textContent = errorText(error); }
  alive(life) { return this.active && life === this.lifecycle; }
  current(generation) { return this.active && this.ready && this.generation === generation; }

  newConversation() {
    if (this.active) return;
    this.session = ''; this.sessionToken = '';
    store.remove(SESSION_KEY); store.remove(TOKEN_KEY);
    this.el('transcript').replaceChildren();
    this.el('served-by').textContent = '';
    this.el('config-status').textContent = 'Nova conversa: a próxima chamada começa sem o contexto anterior.';
  }

  async start(reconnect = false) {
    if (this.active && !reconnect) return;
    if (!this.canStart()) { this.showError('Selecione modelos disponíveis para STT, conversa e TTS.'); return; }
    if (!window.isSecureContext || !navigator.mediaDevices?.getUserMedia || !window.AudioWorkletNode) {
      this.showError('Use um navegador com AudioWorklet em HTTPS ou localhost para acessar o microfone.'); return;
    }
    const life = ++this.lifecycle;
    this.active = true; this.ready = false; this.el('error').textContent = '';
    if (!reconnect) {
      this.startedAt = this.lastActivity = performance.now(); this.retries = 0; this.muted = false; this.sttCost = 0;
      this.el('first-audio').textContent = '—'; this.el('stt-latency').textContent = '—';
      this.el('cost').textContent = 'Indisponível';
    }
    this.renderState(reconnect ? 'reconnecting' : 'connecting');
    try {
      // The microphone is requested only here, after the user starts a call.
      this.context = new AudioContext();
      await this.context.resume();
      if (life !== this.lifecycle) return;
      await this.openMicrophone(life);
      if (life !== this.lifecycle) return;
      await this.connectSocket(life);
      if (life !== this.lifecycle) return;
      this.ready = true; this.retries = 0;
      this.vad.reset(); this.renderState(this.restingState());
      clearInterval(this.callTimer);
      this.callTimer = setInterval(() => this.tick(), 1000);
      this.drawLevel();
    } catch (error) {
      if (life !== this.lifecycle) return;
      this.end(); this.showError(error); this.renderState('error');
    }
  }

  async openMicrophone(life) {
    const stream = await navigator.mediaDevices.getUserMedia({ audio: { channelCount: 1, echoCancellation: true, noiseSuppression: true, autoGainControl: true }, video: false });
    if (life !== this.lifecycle) { stream.getTracks().forEach(t => t.stop()); return; }
    this.stream = stream;
    for (const track of stream.getTracks()) {
      track.enabled = !this.muted;
      track.onended = () => { if (this.active && life === this.lifecycle) this.end('O microfone foi desconectado. Selecione ou conecte um dispositivo e inicie novamente.'); };
    }
    await this.context.audioWorklet.addModule('/assets/voice-pcm-worklet.js');
    if (life !== this.lifecycle) return;
    this.input = this.context.createMediaStreamSource(stream);
    this.worklet = new AudioWorkletNode(this.context, 'garra-voice-pcm', { numberOfInputs: 1, numberOfOutputs: 1, outputChannelCount: [1] });
    this.worklet.port.onmessage = ({ data }) => this.onPcm(data);
    this.input.connect(this.worklet); this.worklet.connect(this.context.destination);
    this.analyser = this.context.createAnalyser(); this.analyser.fftSize = 256;
    this.analyser.connect(this.context.destination);
  }

  connectSocket(life) {
    return new Promise((resolve, reject) => {
      const url = new URL('/ws', location.href); url.protocol = location.protocol === 'https:' ? 'wss:' : 'ws:';
      // Same handshake as the text chat: a browser WebSocket cannot send an
      // Authorization header, so the gateway key goes in the query here only.
      const key = store.get('garraia.gateway_key'); if (key) url.searchParams.set('token', key);
      const socket = this.socket = new WebSocket(url);
      let settled = false;
      const finish = error => {
        if (settled) return; settled = true; clearTimeout(timer);
        if (error) reject(error); else resolve();
      };
      const timer = setTimeout(() => finish(new Error('A conexão de voz excedeu 10 segundos. Verifique o gateway.')), 10000);
      socket.onopen = () => {
        if (life !== this.lifecycle) { socket.close(); return; }
        const init = this.session ? { type: 'resume', session_id: this.session, session_token: this.sessionToken } : { type: 'init' };
        socket.send(JSON.stringify(init));
      };
      socket.onmessage = ({ data }) => {
        if (life !== this.lifecycle || typeof data !== 'string') return;
        let event; try { event = JSON.parse(data); } catch { return; }
        if (event.type === 'connected' || event.type === 'resumed') {
          if (event.session_id) {
            if (this.session && event.session_id !== this.session) {
              this.el('config-status').textContent = 'A conversa anterior não pôde ser retomada; uma nova sessão foi iniciada.';
            }
            this.session = event.session_id; store.set(SESSION_KEY, this.session);
          }
          if (event.session_token) { this.sessionToken = event.session_token; store.set(TOKEN_KEY, this.sessionToken); }
          finish(); return;
        }
        if (!settled && event.type === 'error') { finish(new Error(event.message || 'Não foi possível recuperar a sessão de voz.')); return; }
        this.onServerEvent(event);
      };
      socket.onerror = () => { if (!settled) finish(new Error('Falha na conexão. Verifique o gateway e a chave de acesso.')); };
      socket.onclose = () => {
        if (life !== this.lifecycle) return;
        if (!settled) finish(new Error('O gateway fechou a conexão de voz.'));
        else if (this.active) this.reconnect();
      };
    });
  }

  reconnect() {
    const attempt = this.retries + 1;
    this.releaseResources(); this.lifecycle++; this.ready = false;
    if (attempt > 3) { this.end('Não foi possível reconectar. Verifique a rede e inicie novamente.'); return; }
    this.retries = attempt;
    this.renderState('reconnecting');
    // A request whose result is unknown is not repeated: the user speaks again.
    this.el('error').textContent = 'Conexão perdida. Microfone desligado enquanto a sessão é recuperada; repita a última fala se ela não foi respondida.';
    this.reconnectTimer = setTimeout(() => { if (this.active) this.start(true); }, Math.min(1000 * 2 ** (attempt - 1), 4000));
  }

  tick() {
    const now = performance.now();
    const elapsed = Math.floor((now - this.startedAt) / 1000);
    this.el('duration').textContent = `${Math.floor(elapsed / 60).toString().padStart(2, '0')}:${(elapsed % 60).toString().padStart(2, '0')}`;
    if (now - this.startedAt >= MAX_CALL_MS) this.end('Chamada encerrada após 60 minutos. Você pode iniciar outra mantendo o contexto.');
    else if (now - this.lastActivity >= MAX_IDLE_MS) this.end('Chamada encerrada após 5 minutos sem atividade.');
  }

  onPcm({ samples, rms }) {
    if (!this.active || !this.ready || this.muted || !(samples instanceof Float32Array)) return;
    this.micLevel = Math.min(1, rms * 8);
    if (!this.vad.active) this.vad.silenceMs = Number(this.el('silence').value) || 800;
    const event = this.vad.push(samples, rms);
    if (event.started) {
      this.lastActivity = performance.now();
      this.utteranceSettings = this.settings();
      // Barge-in: the local audio stops now, the LLM turn is cancelled.
      this.interrupt(); this.renderState('user-speaking');
      this.el('config-status').textContent = 'Configuração aplicada a esta fala.';
    }
    if (event.utterance) {
      if (event.limited) this.el('config-status').textContent = 'Trecho de 30 segundos enviado. Continue falando em seguida.';
      this.enqueueUtterance(event.utterance, this.utteranceSettings || this.settings());
    }
  }

  /** UTF-16 offset in the assistant text that was actually played. */
  heardUnits() {
    let units = this.playedUnits || 0;
    if (this.playing && this.context) {
      const { start, end, startedAt, duration } = this.playing;
      const ratio = duration > 0 ? Math.min(1, Math.max(0, (this.context.currentTime - startedAt) / duration)) : 0;
      units = Math.max(units, start + Math.floor((end - start) * ratio));
    }
    return units;
  }

  interrupt() {
    const turnId = this.turnId;
    const playing = this.isPlayingAudio();
    const heard = turnId ? codePointLength((this.assistantText || '').slice(0, this.heardUnits())) : 0;
    this.generation++;
    this.abortAudio();
    if (this.assistantElement && (this.turnActive || playing)) this.assistantElement.dataset.interrupted = 'true';
    const open = this.socket?.readyState === WebSocket.OPEN;
    if (this.turnActive && !this.stopPromise && open) {
      // Wait for the acknowledgement before the next turn exists, so late
      // deltas can never be mistaken for the new answer.
      this.stopTurnId = turnId;
      this.stopPromise = new Promise(resolve => { this.stopResolve = resolve; });
      this.socket.send(JSON.stringify({ type: 'stop', session_id: this.session, turn_id: turnId, heard_chars: heard }));
      this.stopTimer = setTimeout(() => {
        this.end('O gateway não confirmou a interrupção. Inicie novamente para recuperar a sessão.');
      }, STOP_ACK_MS);
    } else if (!this.turnActive && playing && turnId && open) {
      // The server already finished this turn; it only learns how much of it
      // was heard, for the context of the next turn.
      this.socket.send(JSON.stringify({ type: 'stop', session_id: this.session, turn_id: turnId, heard_chars: heard }));
    }
    if (this.active && this.ready) this.renderState(this.restingState());
    return this.stopPromise || Promise.resolve();
  }

  /** Utterances are transcribed in order; none is dropped by a barge-in. */
  enqueueUtterance(frames, settings) {
    const life = this.lifecycle;
    const endedAt = performance.now();
    this.lastActivity = endedAt;
    this.pendingUtterances++;
    this.renderState('processing');
    this.sttChain = this.sttChain
      .then(() => this.transcribe(frames, settings, life, endedAt))
      .catch(() => {})
      .finally(() => { this.pendingUtterances = Math.max(0, this.pendingUtterances - 1); });
    return this.sttChain;
  }

  async transcribe(frames, settings, life, endedAt) {
    if (!this.alive(life)) return;
    const controller = new AbortController(); this.sttControllers.add(controller);
    let text = '';
    try {
      const body = new FormData(); body.append('file', encodeWav(frames), 'speech.wav');
      body.append('provider', settings.stt.provider); body.append('model', settings.stt.id);
      if (settings.language) body.append('language', settings.language);
      const started = performance.now();
      const response = await this.api('/api/voice/transcribe', { method: 'POST', body, signal: controller.signal });
      const result = await response.json();
      if (!this.alive(life)) return;
      this.el('stt-latency').textContent = `${Math.round(performance.now() - started)} ms`;
      if (typeof result.usage?.cost === 'number') {
        this.sttCost += result.usage.cost;
        this.el('cost').textContent = `US$ ${this.sttCost.toFixed(6)} em STT informado pelo provedor; conversa e síntese sem custo informado.`;
      }
      text = (result.text || '').trim();
    } catch (error) {
      if (this.alive(life) && error.name !== 'AbortError') this.showError(error);
    } finally { this.sttControllers.delete(controller); }
    if (!this.alive(life)) return;
    if (text) this.appendTranscript('Você', text);
    const combined = [this.carryText, text].filter(Boolean).join(' ');
    // Still speaking, or another utterance is queued: join them into one turn
    // instead of answering half a sentence.
    if (this.vad.active || this.pendingUtterances > 1) { this.carryText = combined; return; }
    this.carryText = '';
    if (!combined) { if (!this.turnActive && !this.isPlayingAudio()) this.renderState(this.restingState()); return; }
    await (this.stopPromise || Promise.resolve());
    if (!this.alive(life)) return;
    if (this.vad.active) { this.carryText = combined; return; }
    this.sendTurn(combined, settings, endedAt);
  }

  sendTurn(text, settings, endedAt) {
    if (this.socket?.readyState !== WebSocket.OPEN) {
      this.showError('Conexão com o gateway indisponível; a fala não foi enviada.'); return;
    }
    this.turnId = newTurnId(++this.turnSeq);
    this.activeSettings = settings; this.endedSpeechAt = endedAt;
    this.firstAudioAt = null; this.assistantText = ''; this.phraseBuffer = ''; this.audioLimited = false;
    this.consumedUnits = 0; this.playedUnits = 0; this.playing = null;
    this.assistantElement = null; this.turnActive = true; this.responseDone = false;
    const message = { content: text, provider: settings.provider, stream: true, voice: true, turn_id: this.turnId };
    if (settings.model) message.model = settings.model;
    this.socket.send(JSON.stringify(message)); this.renderState('processing');
  }

  onServerEvent(event) {
    if (event.type === 'stopped') {
      if (this.stopPromise && (!event.turn_id || event.turn_id === this.stopTurnId)) {
        this.turnActive = false; clearTimeout(this.stopTimer);
        const resolve = this.stopResolve; this.stopResolve = null; this.stopPromise = null; resolve?.();
      }
      return;
    }
    // Turn-level errors without a turn id (rate limit, invalid turn id) still
    // end the current turn instead of leaving it "thinking" forever.
    if (event.type === 'error' && !event.turn_id && this.turnActive && !this.stopPromise) {
      this.failTurn(event); return;
    }
    // Anything from another turn (late deltas after a barge-in) is dropped.
    if (!event.turn_id || event.turn_id !== this.turnId || this.stopPromise || !this.turnActive) return;
    if (event.type === 'delta') {
      this.assistantText += event.content || ''; this.updateTranscript();
      this.phraseBuffer += event.content || ''; this.queuePhrases(false);
    } else if (event.type === 'message') {
      const finalText = event.content || '';
      // /ws ends streamed deltas with the complete response. Do not
      // synthesize or append that response a second time.
      if (!this.assistantText) this.phraseBuffer = finalText;
      else if (finalText.startsWith(this.assistantText)) this.phraseBuffer += finalText.slice(this.assistantText.length);
      this.assistantText = finalText || this.assistantText; this.updateTranscript();
      this.queuePhrases(true); this.turnActive = false; this.responseDone = true;
      this.showServedBy(event);
      this.finishPlaybackIfEmpty();
    } else if (event.type === 'error') {
      this.failTurn(event);
    } else if (event.type === 'tool_started' && !this.source) this.renderState('processing');
    this.lastActivity = performance.now();
  }

  failTurn(event) {
    this.turnActive = false; this.abortAudio();
    this.showError(event.message || event.code || 'O provedor de conversa falhou.');
    this.renderState(this.restingState());
  }

  showServedBy(event) {
    const el = this.el('served-by');
    if (!event.provider) { el.textContent = ''; return; }
    const label = event.model ? `${event.provider} · ${event.model}` : event.provider;
    el.textContent = `Respondido por ${label}.`;
    el.dataset.fallback = String(Boolean(event.fallback));
    if (event.fallback) {
      el.textContent += ` O provedor escolhido (${this.activeSettings?.provider || '?'}) falhou e o fallback configurado pelo operador respondeu.`;
    }
  }

  appendTranscript(speaker, text) {
    const entry = document.createElement('p'); entry.className = 'voice-turn';
    const label = document.createElement('strong'); label.textContent = speaker;
    const content = document.createElement('span'); content.textContent = text;
    entry.append(label, content); this.el('transcript').append(entry);
    // Limit rendered DOM, while the gateway keeps the conversation history.
    while (this.el('transcript').children.length > 100) this.el('transcript').firstElementChild.remove();
    this.el('transcript').scrollTop = this.el('transcript').scrollHeight;
    return entry;
  }
  updateTranscript() {
    if (!this.assistantElement) this.assistantElement = this.appendTranscript('GarraIA', '');
    this.assistantElement.querySelector('span').textContent = this.assistantText;
    this.el('transcript').scrollTop = this.el('transcript').scrollHeight;
  }

  queuePhrases(final) {
    if (this.audioLimited) { this.phraseBuffer = ''; return; }
    while (this.phraseBuffer) {
      const before = this.phraseBuffer.length;
      const [phrase, rest] = takePhrase(this.phraseBuffer, final);
      if (!phrase) break;
      this.phraseBuffer = rest;
      const start = this.consumedUnits; this.consumedUnits += before - rest.length;
      if (this.textQueue.length >= MAX_AUDIO_QUEUE) {
        this.audioLimited = true; this.phraseBuffer = '';
        this.el('config-status').textContent = 'Resposta longa: o limite da fila de áudio foi atingido. A resposta completa permanece na transcrição.';
        break;
      }
      const speak = speakable(phrase);
      if (speak) this.textQueue.push({ text: speak, start, end: this.consumedUnits });
    }
    this.pumpSynthesis();
  }

  async pumpSynthesis() {
    if (this.synthesizing || !this.active || !this.ready || !this.context) return;
    const generation = this.generation;
    this.synthesizing = generation + 1;
    try {
      while (this.current(generation) && this.textQueue.length && this.audioQueue.length < 2) {
        const item = this.textQueue.shift();
        const settings = this.activeSettings;
        const controller = new AbortController(); this.ttsControllers.add(controller);
        try {
          const response = await this.api('/api/voice/synthesize', {
            method: 'POST', headers: { 'Content-Type': 'application/json' }, signal: controller.signal,
            body: JSON.stringify({ text: item.text, provider: settings.tts.provider, model: settings.tts.id, language: settings.language || undefined, voice: settings.voice || undefined }),
          });
          const contentType = response.headers.get('content-type') || '';
          if (!/^audio\//i.test(contentType)) {
            const body = await response.json().catch(() => ({}));
            throw new Error(body.error || 'O provedor não retornou áudio. A resposta continua disponível na transcrição.');
          }
          const bytes = await response.arrayBuffer();
          if (!this.current(generation)) return;
          if (bytes.byteLength > 10 * 1024 * 1024) throw new Error('Áudio maior que o limite de reprodução. Consulte a transcrição.');
          const buffer = await this.context.decodeAudioData(bytes);
          if (!this.current(generation)) return;
          this.audioQueue.push({ buffer, start: item.start, end: item.end }); this.playNext();
        } finally { this.ttsControllers.delete(controller); }
      }
    } catch (error) {
      if (this.current(generation) && error.name !== 'AbortError') {
        this.textQueue = []; this.audioLimited = true;
        this.showError('Falha na voz: ' + errorText(error) + ' A transcrição está disponível. Nenhum provedor alternativo foi acionado.');
      }
    } finally {
      if (this.synthesizing === generation + 1) this.synthesizing = null;
      if (this.current(generation)) this.finishPlaybackIfEmpty();
    }
  }

  playNext() {
    if (this.source || !this.audioQueue.length || !this.context || !this.active) return;
    const generation = this.generation;
    const item = this.audioQueue.shift();
    const source = this.source = this.context.createBufferSource(); source.buffer = item.buffer;
    source.connect(this.analyser);
    this.playing = { start: item.start, end: item.end, startedAt: this.context.currentTime, duration: item.buffer.duration };
    source.onended = () => {
      source.disconnect();
      if (!this.current(generation) || this.source !== source) return;
      this.playedUnits = Math.max(this.playedUnits || 0, item.end);
      this.source = null; this.playing = null;
      this.playNext(); this.pumpSynthesis(); this.finishPlaybackIfEmpty();
    };
    source.start(); this.lastActivity = performance.now();
    if (this.firstAudioAt === null) {
      this.firstAudioAt = performance.now();
      this.el('first-audio').textContent = `${Math.round(this.firstAudioAt - this.endedSpeechAt)} ms`;
    }
    this.renderState('speaking');
  }
  finishPlaybackIfEmpty() {
    if (this.responseDone && !this.isPlayingAudio() && this.active) this.renderState(this.restingState());
    else if (this.active) this.renderState(this.status);
  }
  abortAudio() {
    for (const controller of this.ttsControllers) controller.abort(); this.ttsControllers.clear();
    this.textQueue = []; this.audioQueue = []; this.phraseBuffer = ''; this.synthesizing = null; this.playing = null;
    if (this.source) { this.source.onended = null; try { this.source.stop(); this.source.disconnect(); } catch { /* already stopped */ } this.source = null; }
  }
  toggleMute() {
    this.muted = !this.muted; this.vad.reset(); this.micLevel = 0;
    this.stream?.getAudioTracks().forEach(track => { track.enabled = !this.muted; });
    this.renderState(this.source ? 'speaking' : this.turnActive ? 'processing' : this.restingState());
  }
  drawLevel() {
    if (!this.active || !this.context) return;
    let level = this.muted ? 0 : this.micLevel || 0;
    if (this.source && this.analyser) {
      const data = new Float32Array(this.analyser.fftSize); this.analyser.getFloatTimeDomainData(data);
      level = Math.min(1, Math.sqrt(data.reduce((n, v) => n + v * v, 0) / data.length) * 5);
    }
    this.el('orb').style.setProperty('--voice-level', String(level));
    this.el('level').value = this.muted ? 0 : this.micLevel || 0;
    this.animation = requestAnimationFrame(() => this.drawLevel());
  }
  releaseResources() {
    this.generation++; this.abortAudio(); this.vad.reset();
    for (const controller of this.sttControllers) controller.abort(); this.sttControllers.clear();
    this.sttChain = Promise.resolve(); this.pendingUtterances = 0; this.carryText = '';
    clearInterval(this.callTimer); clearTimeout(this.stopTimer); clearTimeout(this.reconnectTimer); clearTimeout(this.hiddenTimer);
    cancelAnimationFrame(this.animation);
    const resolve = this.stopResolve; this.stopResolve = this.stopPromise = null; resolve?.();
    this.turnActive = false;
    if (this.socket) { this.socket.onclose = this.socket.onerror = this.socket.onmessage = this.socket.onopen = null; try { this.socket.close(); } catch { /* closed */ } this.socket = null; }
    if (this.worklet) { this.worklet.port.onmessage = null; this.worklet.port.postMessage('stop'); this.worklet.disconnect(); this.worklet.port.close(); this.worklet = null; }
    this.input?.disconnect(); this.input = null; this.analyser?.disconnect(); this.analyser = null;
    this.stream?.getTracks().forEach(track => { track.onended = null; track.stop(); }); this.stream = null;
    if (this.context) { this.context.close().catch(() => {}); this.context = null; }
    this.el('orb').style.setProperty('--voice-level', '0'); this.el('level').value = 0;
  }
  end(message) {
    if (this.turnActive && this.socket?.readyState === WebSocket.OPEN) {
      this.socket.send(JSON.stringify({ type: 'stop', session_id: this.session, turn_id: this.turnId }));
    }
    this.lifecycle++; this.active = this.ready = false;
    this.releaseResources(); this.renderState('idle');
    if (message) this.el('config-status').textContent = message;
  }
}

const root = document.getElementById('voice-chat-page');
const chat = root ? new VoiceChat(root) : null;

// Seam de teste (#1754), no mesmo espirito do `window.__garraTts`: o spec
// Playwright `tests/playwright/webchat-voice-chat.spec.ts` exercita o VAD, o
// segmentador e a chamada por aqui, com microfone falso e `/ws` roteado. Nao e
// API publica. Congelado para o spec nao trocar a implementacao por engano.
window.__garraVoice = Object.freeze({
  VoiceActivityDetector, encodeWav, takePhrase, speakable, describePrices, codePointLength, chat,
});
