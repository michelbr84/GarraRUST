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
const telas = ['tela-boas-vindas', 'tela-provedor', 'tela-modo', 'tela-qr', 'tela-personalidade', 'tela-pronto'];

let modoEscolhido = null;   // "proprio" | "dedicado"
let numeroPessoal = null;   // +digitos normalizado (modo dedicado)
let confirmoAberto = false; // aviso do modo proprio marcado
let pareado = false;        // o `done` chegou
let erroMostrado = false;   // um `error` ja apareceu nesta tentativa
let provedorEscolhido = null; // "ollama" | "openrouter" | null (#1700)
let provedorTestado = false;  // o teste de conectividade passou nesta sessao
let voltarParaPronto = false; // a tela de provedor foi aberta pela tela Pronto
let gatewayDeFora = null;     // #1816: { versao, versao_app } quando o gateway nao e do app

function anunciar(texto) { $('live-region').textContent = texto; }

function mostrar(tela) {
  for (const t of telas) $(t).classList.toggle('ativa', t === tela);
  const h = document.querySelector('#' + tela + ' h1');
  if (h) h.focus({ preventScroll: false });
}

// Marca que a tela de primeiro uso apareceu (roteiro manual / smoke CI).
invoke('onboarding_marcar_exibido');

// ── Boas-vindas ──────────────────────────────────────────────────────────────
$('btn-comecar').addEventListener('click', () => abrirProvedor(false));
$('btn-modo-voltar').addEventListener('click', () => mostrar('tela-provedor'));

// #1816: ja havia um gateway GarraIA rodando fora deste app. Dizer qual e o que
// isso muda vem ANTES de qualquer passo — o teste inteiro seria contra ele.
// Monta um aviso so com texto e <code> — nada que venha de fora (a versao que
// o servico da porta disse, o motivo do provedor) vira HTML.
function preencher(el, partes) {
  el.textContent = '';
  for (const p of partes) {
    if (p && typeof p === 'object') {
      const n = document.createElement(p.tag);
      n.textContent = p.texto;
      el.appendChild(n);
    } else {
      el.appendChild(document.createTextNode(String(p)));
    }
  }
}
function partesGatewayDeFora(g) {
  const partes = ['Já existe um Garra rodando nesta máquina (versão ' + g.versao + '), fora deste app — é ele que vai atender.'];
  if (g.versao !== g.versao_app) {
    partes.push(' Ele é de outra versão (este app é a ' + g.versao_app + '). Para usar o Garra deste app, encerre o outro (por exemplo ',
      { tag: 'code', texto: 'systemctl --user stop garraia' }, ') e abra o app de novo.');
  }
  return partes;
}
invoke('gateway_estado').then((g) => {
  if (g && g.origem === 'externo' && typeof g.versao === 'string') {
    gatewayDeFora = g;
    const el = $('bv-aviso-gateway');
    preencher(el, partesGatewayDeFora(g));
    el.classList.add('ativo');
  }
}).catch(() => {});

// ── Quem responde (#1700) ────────────────────────────────────────────────────
// A chave colada vai UMA vez para o backend (que a entrega pelo stdin do
// `config set-routing`) e o campo e limpo em seguida: ela nunca volta para ca.
function esconderErroProv() { const e = $('prov-erro'); e.classList.remove('ativo'); e.textContent = ''; }
function mostrarErroProv(msg) { const e = $('prov-erro'); e.textContent = msg; e.classList.add('ativo'); anunciar(msg); }

function atualizarBotaoProv() {
  const btn = $('btn-prov-testar');
  if (provedorTestado) { btn.disabled = false; btn.textContent = 'Continuar'; return; }
  if (provedorEscolhido === 'ollama') {
    btn.disabled = !$('ollama-modelo').value;
    btn.textContent = 'Salvar e testar';
  } else if (provedorEscolhido === 'openrouter') {
    btn.disabled = !$('openrouter-chave').value.trim() || !$('openrouter-modelo').value.trim();
    btn.textContent = 'Salvar e testar';
  } else {
    // Sem escolha nova: com um provedor ja configurado, so testa.
    btn.disabled = !$('prov-atual').classList.contains('ativo');
    btn.textContent = 'Testar e continuar';
  }
}

function selecionarProvedor(p) {
  provedorEscolhido = p;
  provedorTestado = false;
  esconderErroProv();
  $('prov-ok').classList.add('hidden');
  $('btn-prov-ollama').setAttribute('aria-pressed', String(p === 'ollama'));
  $('btn-prov-openrouter').setAttribute('aria-pressed', String(p === 'openrouter'));
  $('config-ollama').classList.toggle('hidden', p !== 'ollama');
  $('config-openrouter').classList.toggle('hidden', p !== 'openrouter');
  if (p === 'openrouter') $('openrouter-chave').focus();
  atualizarBotaoProv();
}

async function abrirProvedor(daTelaPronto) {
  voltarParaPronto = daTelaPronto;
  mostrar('tela-provedor');
  esconderErroProv();
  let st = null;
  try { st = await invoke('onboarding_provedor_status'); } catch (_e) { st = null; }
  st = st || {};
  const atual = $('prov-atual');
  if (st.configurado) {
    atual.textContent = 'Já configurado: ' + st.provedor + (st.modelo ? ' · ' + st.modelo : '') +
      '. Teste para confirmar que ele responde, ou escolha outro abaixo.';
    atual.classList.add('ativo');
  } else {
    atual.textContent = '';
    atual.classList.remove('ativo');
  }
  const sel = $('ollama-modelo');
  sel.innerHTML = '';
  const modelos = Array.isArray(st.ollama_modelos) ? st.ollama_modelos : [];
  for (const m of modelos) {
    const o = document.createElement('option');
    o.value = m; o.textContent = m;
    sel.appendChild(o);
  }
  const temOllama = Boolean(st.ollama_disponivel) && modelos.length > 0;
  $('btn-prov-ollama').disabled = !temOllama;
  $('prov-ollama-sub').textContent = temOllama
    ? 'Roda neste computador: sem chave e sem custo por mensagem. Encontrei ' + modelos.length + ' modelo(s).'
    : (st.ollama_disponivel
      ? 'O Ollama está rodando, mas sem nenhum modelo de conversa. Baixe um (ex.: ollama pull qwen3.5:2b) e volte aqui.'
      : 'Não encontrei o Ollama neste computador (porta 11434). Instale em ollama.com, ou use o OpenRouter.');
  if (!$('openrouter-modelo').value) $('openrouter-modelo').value = st.modelo_padrao_openrouter || '';
  atualizarBotaoProv();
}

$('btn-prov-ollama').addEventListener('click', () => selecionarProvedor('ollama'));
$('btn-prov-openrouter').addEventListener('click', () => selecionarProvedor('openrouter'));
$('ollama-modelo').addEventListener('change', atualizarBotaoProv);
$('openrouter-chave').addEventListener('input', () => { provedorTestado = false; esconderErroProv(); atualizarBotaoProv(); });
$('openrouter-modelo').addEventListener('input', () => { provedorTestado = false; esconderErroProv(); atualizarBotaoProv(); });
$('btn-prov-voltar').addEventListener('click', () => mostrar(voltarParaPronto ? 'tela-pronto' : 'tela-boas-vindas'));

function seguirDoProvedor() {
  if (voltarParaPronto) {
    voltarParaPronto = false;
    mostrar('tela-pronto');
    finalizar(); // reaplica o modo (idempotente), reinicia o gateway e testa de novo
  } else {
    mostrar('tela-modo');
  }
}

$('btn-prov-depois').addEventListener('click', () => { provedorTestado = false; seguirDoProvedor(); });

$('btn-prov-testar').addEventListener('click', async () => {
  if (provedorTestado) { seguirDoProvedor(); return; }
  const btn = $('btn-prov-testar');
  btn.disabled = true;
  esconderErroProv();
  $('prov-ok').classList.add('hidden');
  try {
    if (provedorEscolhido === 'ollama') {
      await invoke('onboarding_configurar_provedor', { provedor: 'ollama', modelo: $('ollama-modelo').value, chave: null });
    } else if (provedorEscolhido === 'openrouter') {
      const chave = $('openrouter-chave').value;
      $('openrouter-chave').value = '';
      await invoke('onboarding_configurar_provedor', { provedor: 'openrouter', modelo: $('openrouter-modelo').value, chave });
    }
  } catch (e) {
    mostrarErroProv(typeof e === 'string' ? e : 'Não deu para salvar o provedor.');
    atualizarBotaoProv();
    return;
  }
  $('prov-testando').classList.remove('hidden');
  anunciar('Testando o modelo.');
  let r = null;
  try { r = await invoke('onboarding_testar_provedor'); } catch (e) { r = { ok: false, motivo: String(e) }; }
  $('prov-testando').classList.add('hidden');
  if (r && r.ok) {
    provedorTestado = true;
    $('prov-ok-texto').textContent = 'Respondeu' + (r.modelo ? ' (' + r.modelo + ')' : '') +
      (r.latencia_ms != null ? ' em ' + (r.latencia_ms / 1000).toFixed(1) + ' s' : '') + '.';
    $('prov-ok').classList.remove('hidden');
    anunciar('O modelo respondeu. Pode continuar.');
  } else {
    provedorTestado = false;
    mostrarErroProv('O modelo não respondeu: ' + ((r && r.motivo) || 'erro desconhecido') + '. ' + ((r && r.dica) || ''));
  }
  atualizarBotaoProv();
});

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
// A instrucao do primeiro "oi" depende do modo (#1813): no proprio numero, o
// dono fala com o Garra pela conversa "Voce"; no dedicado, do celular pessoal.
function instrucaoDoModo() {
  return modoEscolhido === 'dedicado'
    ? 'Do seu <b>celular pessoal</b>, mande um <b>“oi”</b> para o número do Garra e veja ele responder.'
    : 'No WhatsApp, abra a conversa <b>“Você”</b> (conversa com você mesmo) e mande um <b>“oi”</b> — ' +
      'ou peça para alguém te mandar uma mensagem.';
}

async function finalizar() {
  const txt = $('pronto-status-texto');
  const status = $('pronto-status');
  const aviso = $('pronto-aviso');
  txt.textContent = 'Ligando o Garra…';
  status.classList.remove('alerta');
  status.style.color = '';
  aviso.classList.remove('ativo');
  $('btn-pronto-provedor').classList.add('hidden');
  $('pronto-instrucao').innerHTML = instrucaoDoModo();
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
  } catch (e) {
    txt.textContent = 'Ainda não ficou pronto';
    const el = $('pronto-erro');
    el.textContent = 'O canal foi vinculado, mas faltou autorizar/ligar o Garra: ' + String(e) +
      '. Abra os Ajustes e tente de novo.';
    el.classList.add('ativo');
    return;
  }

  // #1816: com um gateway de fora do app, quem precisa reiniciar para ligar o
  // WhatsApp e ELE — o app nao reinicia o que nao e dele.
  if (gatewayDeFora) {
    const g = $('pronto-aviso-gateway');
    preencher(g, ['Quem atende é o Garra que já estava rodando nesta máquina (versão ' + gatewayDeFora.versao +
      '). Reinicie esse Garra para ele ligar o WhatsApp (por exemplo ',
    { tag: 'code', texto: 'systemctl --user restart garraia' }, ').']);
    g.classList.add('ativo');
  }

  // #1700: "conectado" nao e "conversa". O teste e uma chamada de verdade ao
  // modelo padrao; so o resultado volta (nunca a chave nem a resposta).
  txt.textContent = 'Testando se o Garra consegue conversar…';
  let r = null;
  try { r = await invoke('onboarding_testar_provedor'); } catch (e) { r = { ok: false, motivo: String(e) }; }
  if (r && r.ok) {
    txt.textContent = 'Garra conectado e pronto para conversar';
    status.style.color = 'var(--garra-ok)';
    anunciar('Garra conectado e pronto para conversar.');
    return;
  }
  txt.textContent = 'WhatsApp conectado — mas o Garra ainda não conversa';
  status.classList.add('alerta');
  preencher(aviso, [{ tag: 'b', texto: 'Falta quem responde.' },
    ' O Garra recebe suas mensagens, mas não vai responder até um modelo de IA funcionar. Motivo: ' +
    ((r && r.motivo) || 'nenhum provedor configurado') + '. ' + ((r && r.dica) || '')]);
  aviso.classList.add('ativo');
  $('btn-pronto-provedor').classList.remove('hidden');
  anunciar('WhatsApp conectado, mas o Garra ainda não conversa. Configure quem responde.');
}
$('btn-pronto-provedor').addEventListener('click', () => abrirProvedor(true));
$('btn-abrir-garra').addEventListener('click', () => invoke('onboarding_fechar'));
