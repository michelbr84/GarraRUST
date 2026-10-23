/**
 * Assistente de Primeiro Uso (#1636, fatia 3) — teste da UI sem Tauri.
 *
 * Abre `ui/onboarding.html` por file:// com um `window.__TAURI__` falso e dirige
 * o fluxo emitindo os eventos NDJSON que o `garra whatsapp link --machine`
 * (fatia 1) produz — inclusive um `qr` REAL gerado pelo motor da fatia 1
 * (`fixtures/qr-line.json`, 33x33). Prova:
 *   1. navegacao Boas-vindas -> Modo -> QR -> Personalidade -> Pronto
 *   2. o canvas do QR desenha a matriz (pixels opacos escuros != 0)
 *   3. modo "proprio" exige o aviso marcado; "dedicado" exige o numero pessoal,
 *      validado antes de parear; ambos chegam aos comandos (invoke gravado)
 *   4. o caminho de erro (QR expirado) mostra mensagem acionavel + "gerar novo"
 *
 * Seletores sao `data-testid` estaveis (plan 0052). Nenhum gateway sobe. O
 * script e externo (CSP `script-src 'self'`); o file:// carrega `onboarding.js`
 * do mesmo diretorio sem CSP (a CSP real vive no tauri.conf.json, so no app).
 */
import { test, expect, type Page } from '@playwright/test';
import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';

const PAGE_URL = new URL('../ui/onboarding.html', import.meta.url).href;
const QR_LINE = readFileSync(
  fileURLToPath(new URL('./fixtures/qr-line.json', import.meta.url)),
  'utf8',
).trim();

/** Fake `window.__TAURI__`, instalado ANTES de a pagina carregar. */
async function instalarTauriFalso(page: Page) {
  await page.addInitScript(() => {
    const invokes: Array<{ cmd: string; args: unknown }> = [];
    const listeners: Record<string, Array<(e: { payload: unknown }) => void>> = {};
    (window as unknown as { __invokes: typeof invokes }).__invokes = invokes;
    (window as unknown as { __TAURI__: unknown }).__TAURI__ = {
      core: {
        invoke(cmd: string, args: unknown) {
          invokes.push({ cmd, args });
          if (cmd === 'onboarding_estado') {
            return Promise.resolve({ primeiro_uso: true, modo: null, concluido: false });
          }
          if (cmd === 'onboarding_validar_numero') {
            // Mimetiza o core: digitos (8..15) viram +digitos; senao rejeita.
            const digits = String((args as { numero?: string })?.numero ?? '').replace(/\D/g, '');
            return digits.length >= 8 && digits.length <= 15
              ? Promise.resolve('+' + digits)
              : Promise.reject('numero invalido');
          }
          return Promise.resolve();
        },
      },
      event: {
        listen(ev: string, cb: (e: { payload: unknown }) => void) {
          (listeners[ev] ||= []).push(cb);
          return Promise.resolve(() => {});
        },
      },
    };
    (window as unknown as { __emit: (ev: string, payload: unknown) => void }).__emit = (ev, payload) => {
      for (const cb of listeners[ev] || []) cb({ payload });
    };
  });
}

async function invokes(page: Page): Promise<Array<{ cmd: string; args: unknown }>> {
  return page.evaluate(() => (window as unknown as { __invokes: Array<{ cmd: string; args: unknown }> }).__invokes);
}

async function emitir(page: Page, ev: string, payload: unknown) {
  await page.evaluate(
    ([e, p]) => (window as unknown as { __emit: (ev: string, payload: unknown) => void }).__emit(e as string, p),
    [ev, payload] as const,
  );
}

async function pixelsEscurosDoCanvas(page: Page): Promise<number> {
  return page.evaluate(() => {
    const c = document.getElementById('qr-canvas') as HTMLCanvasElement;
    const ctx = c.getContext('2d')!;
    const d = ctx.getImageData(0, 0, c.width, c.height).data;
    let dark = 0;
    // So conta pixel OPACO e escuro: um canvas intocado e rgb(0,0,0,0)
    // (transparente), que leria como "escuro" sem o teste de alpha.
    for (let i = 0; i < d.length; i += 4) {
      if (d[i + 3] > 0 && d[i] < 128 && d[i + 1] < 128 && d[i + 2] < 128) dark++;
    }
    return dark;
  });
}

/** Avanca da tela de QR ate "Pronto" com os eventos NDJSON gravados. */
async function pareiaAtePronto(page: Page) {
  await expect(page.getByTestId('tela-qr')).toBeVisible();
  expect((await invokes(page)).some((i) => i.cmd === 'wa_iniciar_pareamento')).toBeTruthy();

  await emitir(page, 'wa://ndjson', JSON.stringify({ schema: 1, event: 'status', status: 'connecting' }));
  await expect(page.getByTestId('qr-status')).toContainText('Preparando');

  expect(await pixelsEscurosDoCanvas(page)).toBe(0);
  await emitir(page, 'wa://ndjson', QR_LINE);
  await expect(page.getByTestId('qr-status')).toContainText('Aponte a câmera');
  expect(await pixelsEscurosDoCanvas(page)).toBeGreaterThan(100);

  await emitir(page, 'wa://ndjson', JSON.stringify({ schema: 1, event: 'status', status: 'done' }));
  await expect(page.getByTestId('tela-personalidade')).toBeVisible();

  await page.getByTestId('persona-nome').fill('Papa-Léguas');
  await page.getByTestId('persona-estilo').getByText('Profissional').click();
  await page.getByTestId('btn-persona-continuar').click();
  await expect(page.getByTestId('tela-pronto')).toBeVisible();
}

test.beforeEach(async ({ page }) => {
  await instalarTauriFalso(page);
  await page.goto(PAGE_URL);
});

test('modo proprio exige o aviso marcado e so entao abre a admissao', async ({ page }) => {
  await expect(page.getByTestId('tela-boas-vindas')).toBeVisible();
  await page.getByTestId('btn-comecar').click();
  await expect(page.getByTestId('tela-modo')).toBeVisible();

  // Escolher "proprio" revela o aviso; Continuar fica travado ate confirmar.
  await page.getByTestId('btn-modo-proprio').click();
  await expect(page.getByTestId('config-proprio')).toBeVisible();
  await expect(page.getByTestId('btn-modo-continuar')).toBeDisabled();
  await page.getByTestId('proprio-confirmo').check();
  await expect(page.getByTestId('btn-modo-continuar')).toBeEnabled();
  await page.getByTestId('btn-modo-continuar').click();

  await pareiaAtePronto(page);

  const chamadas = await invokes(page);
  expect(chamadas.find((i) => i.cmd === 'onboarding_aplicar_persona')?.args).toMatchObject({
    nome: 'Papa-Léguas', estilo: 'profissional', idioma: 'pt_br',
  });
  // F3: proprio so aplica com confirmacao explicita.
  expect(chamadas.find((i) => i.cmd === 'onboarding_aplicar_modo')?.args).toMatchObject({
    modo: 'proprio', confirmoAberto: true,
  });
  await expect(page.getByTestId('pronto-status')).toContainText('conectado');

  await page.getByTestId('btn-abrir-garra').click();
  expect((await invokes(page)).some((i) => i.cmd === 'onboarding_fechar')).toBeTruthy();
});

test('modo dedicado valida e autoriza o numero pessoal', async ({ page }) => {
  await page.getByTestId('btn-comecar').click();
  await page.getByTestId('btn-modo-dedicado').click();
  await expect(page.getByTestId('config-dedicado')).toBeVisible();

  await page.getByTestId('numero-pessoal').fill('+55 11 99999-8888');
  await page.getByTestId('btn-modo-continuar').click();

  await pareiaAtePronto(page);

  const chamadas = await invokes(page);
  expect(chamadas.some((i) => i.cmd === 'onboarding_validar_numero')).toBeTruthy();
  // F1: o numero normalizado chega ao comando que autoriza o dono.
  expect(chamadas.find((i) => i.cmd === 'onboarding_aplicar_modo')?.args).toMatchObject({
    modo: 'dedicado', numeroPessoal: '+5511999998888',
  });
  await expect(page.getByTestId('pronto-status')).toContainText('conectado');
});

test('numero pessoal invalido e recusado antes de parear', async ({ page }) => {
  await page.getByTestId('btn-comecar').click();
  await page.getByTestId('btn-modo-dedicado').click();
  // 7 digitos: passa o gate de comprimento da tela, mas o validador E.164
  // (8..15 digitos) recusa — o erro aparece antes de qualquer pareamento.
  await page.getByTestId('numero-pessoal').fill('1234567');
  await page.getByTestId('btn-modo-continuar').click();

  await expect(page.getByTestId('modo-erro')).toBeVisible();
  await expect(page.getByTestId('tela-qr')).toBeHidden();
  expect((await invokes(page)).some((i) => i.cmd === 'wa_iniciar_pareamento')).toBeFalsy();
});

test('caminho de erro: QR expirado mostra mensagem acionavel e botao de novo', async ({ page }) => {
  await page.getByTestId('btn-comecar').click();
  await page.getByTestId('btn-modo-proprio').click();
  await page.getByTestId('proprio-confirmo').check();
  await page.getByTestId('btn-modo-continuar').click();
  await expect(page.getByTestId('tela-qr')).toBeVisible();

  await emitir(page, 'wa://ndjson', JSON.stringify({
    schema: 1, event: 'error', code: 'qr_expired', message: 'No QR was scanned.',
  }));

  const erro = page.getByTestId('qr-erro');
  await expect(erro).toBeVisible();
  await expect(erro).toContainText('expirou');
  await expect(page.getByTestId('btn-tentar-novamente')).toBeVisible();

  await page.getByTestId('btn-tentar-novamente').click();
  await expect(erro).toBeHidden();
  const reinicios = (await invokes(page)).filter((i) => i.cmd === 'wa_iniciar_pareamento');
  expect(reinicios.length).toBeGreaterThanOrEqual(2);
});
