'use strict';
// Assistente de Primeiro Uso (#1636) — logica da webview.
//
// Externo (nao inline) de proposito: a CSP do app (`tauri.conf.json`) e
// `script-src 'self'`, entao nenhuma pagina do desktop carrega script inline.
// A navegacao espelha a maquina pura de `garraia-desktop-core::onboarding`
// (testada no CI); aqui ela e dirigida por cliques e pelos eventos NDJSON que a
// casca repassa (`wa://ndjson`). O QR vem como MATRIZ (`modules`) e e
// rasterizado no canvas; a credencial nunca chega aqui.

// Superficie Tauri com no-ops seguros: a UI nunca quebra fora da casca, e o
// teste Playwright injeta um __TAURI__ falso que emite NDJSON gravado.
const T = window.__TAURI__ || {};
const invoke = (cmd, args) => (T.core?.invoke ? T.core.invoke(cmd, args) : Promise.resolve());
const listen = (ev, cb) => (T.event?.listen ? T.event.listen(ev, cb) : Promise.resolve(() => {}));

const $ = (id) => document.getElementById(id);
const telas = ['tela-boas-vindas', 'tela-modo', 'tela-qr', 'tela-personalidade', 'tela-pronto'];

let modoEscolhido = null;   // "proprio" | "dedicado"
let numeroPessoal = null;   // +digitos normalizado (modo dedicado)
let confirmoAberto = false; // aviso do modo proprio marcado
let pareado = false;        // o `done` chegou
let erroMostrado = false;   // um `error` ja apareceu nesta tentativa

function anunciar(texto) { $('live-region').textContent = texto; }

function mostrar(tela) {
  for (const t of telas) $(t).classList.toggle('ativa', t === tela);
  const h = document.querySelector('#' + tela + ' h1');
  if (h) h.focus({ preventScroll: false });
}

// Marca que a tela de primeiro uso apareceu (roteiro manual / smoke CI).
invoke('onboarding_marcar_exibido');

// ── Boas-vindas ──────────────────────────────────────────────────────────────
$('btn-comecar').addEventListener('click', () => mostrar('tela-modo'));
$('btn-modo-voltar').addEventListener('click', () => mostrar('tela-boas-vindas'));

// ── Modo ───────────────────────────────────────────────────────────────────
function esconderErroModo() { const e = $('modo-erro'); e.classList.remove('ativo'); e.textContent = ''; }
function mostrarErroModo(msg) { const e = $('modo-erro'); e.textContent = msg; e.classList.add('ativo'); anunciar(msg); }

function atualizarContinuar() {
  const btn = $('btn-modo-continuar');
  if (modoEscolhido === 'proprio') {
    btn.disabled = !$('proprio-confirmo').checked;
  } else if (modoEscolhido === 'dedicado') {
    btn.disabled = $('numero-pessoal').value.trim().length < 6;
  } else {
    btn.disabled = true;
  }
}

function selecionarModo(modo) {
  modoEscolhido = modo;
  esconderErroModo();
  $('btn-modo-proprio').setAttribute('aria-pressed', String(modo === 'proprio'));
  $('btn-modo-dedicado').setAttribute('aria-pressed', String(modo === 'dedicado'));
  $('config-proprio').classList.toggle('hidden', modo !== 'proprio');
  $('config-dedicado').classList.toggle('hidden', modo !== 'dedicado');
  if (modo === 'dedicado') $('numero-pessoal').focus();
  atualizarContinuar();
}

$('btn-modo-proprio').addEventListener('click', () => selecionarModo('proprio'));
$('btn-modo-dedicado').addEventListener('click', () => selecionarModo('dedicado'));
$('proprio-confirmo').addEventListener('change', () => { confirmoAberto = $('proprio-confirmo').checked; atualizarContinuar(); });
$('numero-pessoal').addEventListener('input', () => { esconderErroModo(); atualizarContinuar(); });

$('btn-modo-continuar').addEventListener('click', async () => {
  const btn = $('btn-modo-continuar');
  if (modoEscolhido === 'proprio') {
    if (!$('proprio-confirmo').checked) return;
    confirmoAberto = true;
    numeroPessoal = null;
  } else if (modoEscolhido === 'dedicado') {
    btn.disabled = true;
    try {
      // Valida/normaliza no Rust ANTES de parear — erro aparece na hora.
      numeroPessoal = await invoke('onboarding_validar_numero', { numero: $('numero-pessoal').value });
      if (!numeroPessoal) throw new Error('numero vazio');
    } catch (e) {
      mostrarErroModo(typeof e === 'string' ? e : 'Numero invalido. Use o formato com o codigo do pais, ex.: +55 11 99999-8888.');
      btn.disabled = false;
      return;
    }
  } else {
    return;
  }
  iniciarPareamento();
  mostrar('tela-qr');
});

// ── QR ───────────────────────────────────────────────────────────────────────
function iniciarPareamento() {
  pareado = false;
  erroMostrado = false;
  esconderErro();
  setStatus('Preparando a conexão…', true);
  $('btn-tentar-novamente').style.display = 'none';
  invoke('wa_iniciar_pareamento').catch((e) => mostrarErro(String(e)));
}

function setStatus(texto, pulsando) {
  $('qr-status-texto').textContent = texto;
  $('qr-status').querySelector('.pulso').style.visibility = pulsando ? 'visible' : 'hidden';
  anunciar(texto);
}

function esconderErro() { const e = $('qr-erro'); e.classList.remove('ativo'); e.textContent = ''; }
function mostrarErro(msg) {
  erroMostrado = true;
  const e = $('qr-erro'); e.textContent = msg; e.classList.add('ativo');
  setStatus('Não deu para conectar.', false);
  $('btn-tentar-novamente').style.display = 'block';
  anunciar(msg);
}

// Mensagem acionavel por codigo de erro (contrato da fatia 1).
function mensagemDeErro(code, message) {
  switch (code) {
    case 'qr_expired': return 'O QR code expirou antes de ser lido. Gere um novo e escaneie mais rápido.';
    case 'session_dead': return 'Esta conexão não vale mais. Gere um QR novo para reconectar.';
    case 'missing_node': return 'Faltou o Node.js, que acompanha o Garra. Reinstale pelo instalador oficial do site.';
    case 'missing_dependencies': return 'A ponte do WhatsApp está incompleta. Reinstale o Garra pelo instalador oficial.';
    case 'cancelled': return '';
    default: return message || 'Algo deu errado ao conectar. Tente gerar um QR novo.';
  }
}

// Desenha a matriz (linhas de "0"/"1", 1 = modulo escuro) num canvas, com
// zona de silencio de 4 modulos — o motor e o MESMO `qr::modules` da fatia 1.
function desenharQr(modules, width) {
  const canvas = $('qr-canvas');
  const ctx = canvas.getContext('2d');
  const n = Array.isArray(modules) ? modules.length : 0;
  ctx.fillStyle = '#ffffff';
  ctx.fillRect(0, 0, canvas.width, canvas.height);
  if (!n) return;
  const quiet = 4;
  const total = (width || n) + quiet * 2;
  const cell = Math.floor(canvas.width / total);
  const origem = Math.floor((canvas.width - cell * total) / 2) + quiet * cell;
  ctx.fillStyle = '#000000';
  for (let y = 0; y < n; y++) {
    const linha = modules[y] || '';
    for (let x = 0; x < linha.length; x++) {
      if (linha[x] === '1') ctx.fillRect(origem + x * cell, origem + y * cell, cell, cell);
    }
  }
}

function aoConcluirPareamento() {
  pareado = true;
  setStatus('Conectado!', false);
  mostrar('tela-personalidade');
}

// Trata cada linha NDJSON do sidecar (repassada pela casca).
function tratarNdjson(linha) {
  let obj;
  try { obj = JSON.parse(linha); } catch (_e) { return; }
  switch (obj.event) {
    case 'qr':
      esconderErro();
      $('btn-tentar-novamente').style.display = 'none';
      desenharQr(obj.modules, obj.width);
      setStatus('Aponte a câmera do WhatsApp para o código.', true);
      break;
    case 'status':
      switch (obj.status) {
        case 'done': aoConcluirPareamento(); break;
        case 'authenticated':
        case 'session_saved':
        case 'channel_enabled':
          setStatus('Conectando a sua conta…', true); break;
        case 'waiting_qr':
          setStatus('Aguardando a leitura do código…', true); break;
        case 'connecting':
        case 'detecting_node':
        case 'installing_deps':
        case 'deps_ready':
        case 'starting_bridge':
          setStatus('Preparando a conexão…', true); break;
        default: break; // status desconhecido: nao mexe
      }
      break;
    case 'error': {
      const msg = mensagemDeErro(obj.code, obj.message);
      if (msg) mostrarErro(msg);
      break;
    }
    case 'log':
    default:
      break; // cosmetico / desconhecido
  }
}

listen('wa://ndjson', (e) => tratarNdjson(typeof e.payload === 'string' ? e.payload : JSON.stringify(e.payload)));
listen('wa://fim', () => {
  if (!pareado && !erroMostrado) {
    mostrarErro('A conexão foi encerrada antes de concluir. Tente gerar um QR novo.');
  }
});

$('btn-tentar-novamente').addEventListener('click', iniciarPareamento);
$('btn-qr-voltar').addEventListener('click', () => {
  invoke('wa_cancelar_pareamento');
  mostrar('tela-modo');
});

// ── Personalidade ────────────────────────────────────────────────────────────
$('btn-persona-continuar').addEventListener('click', async () => {
  const btn = $('btn-persona-continuar');
  const nome = $('persona-nome').value.trim();
  const estilo = document.querySelector('input[name=estilo]:checked')?.value || 'amigavel';
  const idioma = $('persona-idioma').value || 'pt_br';
  btn.disabled = true;
  $('persona-erro').classList.remove('ativo');
  try {
    await invoke('onboarding_aplicar_persona', { nome, estilo, idioma });
    mostrar('tela-pronto');
    finalizar();
  } catch (e) {
    const el = $('persona-erro');
    el.textContent = 'Não deu para salvar a persona: ' + String(e);
    el.classList.add('ativo');
  } finally {
    btn.disabled = false;
  }
});

// ── Pronto ─────────────────────────────────────────────────────────────────
async function finalizar() {
  const txt = $('pronto-status-texto');
  txt.textContent = 'Ligando o Garra…';
  anunciar('Ligando o Garra.');
  $('pronto-erro').classList.remove('ativo');
  try {
    // So diz "pronto" depois de a admissao + autorizacao (modo dedicado) e o
    // restart terem dado certo — a tela nao pode mentir.
    await invoke('onboarding_aplicar_modo', {
      modo: modoEscolhido,
      numeroPessoal: numeroPessoal,
      confirmoAberto: confirmoAberto,
    });
    txt.textContent = 'Garra conectado';
    $('pronto-status').style.color = 'var(--garra-ok)';
    // #1700: "conectado" nao e "conversa". Sem provedor de LLM no config, o
    // Garra recebe as mensagens e nunca responde — o doctor do WhatsApp ja
    // julga isso; a tela Pronto precisa dizer antes do primeiro "oi" sem
    // resposta. So o booleano volta daqui (a chave nunca atravessa a webview).
    try {
      const temProvedor = await invoke('onboarding_provedor_configurado');
      if (!temProvedor) {
        const aviso = $('pronto-aviso');
        aviso.innerHTML =
          '<b>Último passo:</b> falta dizer quem responde. Sem provedor de LLM ' +
          'configurado, o Garra recebe suas mensagens e <b>não responde</b>. ' +
          'No terminal, rode <code>garraia init</code> (Windows: <code>"C:\\Program Files\\garraia-desktop\\garraia.exe" init</code>) ' +
          'e depois reabra o Garra.';
        aviso.classList.add('ativo');
        anunciar('Garra conectado, mas sem provedor de LLM: o Garra não vai responder até configurar. Veja o aviso na tela.');
      } else {
        anunciar('Garra conectado e pronto para receber mensagens.');
      }
    } catch (_) {
      // Falhou a checagem (ex.: config ilegível): não inventa — mantém o
      // anúncio completo e deixa o doctor/settings mostrarem o estado real.
      anunciar('Garra conectado e pronto para receber mensagens.');
    }
  } catch (e) {
    txt.textContent = 'Ainda não ficou pronto';
    const el = $('pronto-erro');
    el.textContent = 'O canal foi vinculado, mas faltou autorizar/ligar o Garra: ' + String(e) +
      '. Abra os Ajustes e tente de novo.';
    el.classList.add('ativo');
  }
}
$('btn-abrir-garra').addEventListener('click', () => invoke('onboarding_fechar'));
