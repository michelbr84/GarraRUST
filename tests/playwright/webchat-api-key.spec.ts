/**
 * #1045 (PR-A): o console web manda a `gateway.api_key` nas chamadas `/api/*`.
 *
 * Cobre:
 *  1. Com chave guardada, toda chamada same-origin para `/api/` sai com
 *     `Authorization: Bearer <chave>`.
 *  2. Sem chave, nenhuma chamada ganha o header — o shim e um passa-direto.
 *  3. O shim nao toca o que esta fora de `/api/`, para a chave nao ir junto
 *     de requisicao que nao precisa dela.
 *
 * O PR-B liga o gate no servidor. Este PR entra antes justamente para o
 * console nao morrer entre os dois merges.
 */

import { test, expect, Page } from '@playwright/test';

const CHAVE = 'chave-de-teste-do-console';

/** Abre o console ja com (ou sem) a chave em localStorage. */
async function abrirConsole(page: Page, chave: string | null) {
  await page.addInitScript((valor) => {
    if (valor === null) {
      window.localStorage.removeItem('garraia.gateway_key');
    } else {
      window.localStorage.setItem('garraia.gateway_key', valor as string);
    }
  }, chave);
  await page.goto('/');
  await page.locator('.sidebar').waitFor({ state: 'visible', timeout: 10_000 });
}

/** Junta as requisicoes da pagina como `{ caminho, authorization }`. */
function observarRequisicoes(page: Page) {
  const vistas: { caminho: string; auth: string | undefined }[] = [];
  page.on('request', (req) => {
    let url: URL;
    try {
      url = new URL(req.url());
    } catch {
      return;
    }
    vistas.push({ caminho: url.pathname, auth: req.headers()['authorization'] });
  });
  return vistas;
}

test.describe('#1045 PR-A — console manda a chave em /api/*', () => {
  test('com chave, toda chamada /api/ leva Authorization: Bearer', async ({ page }) => {
    const vistas = observarRequisicoes(page);
    await abrirConsole(page, CHAVE);
    // Uma chamada explicita, alem das que a pagina faz sozinha ao abrir.
    await page.evaluate(() => fetch('/api/status').then((r) => r.status));
    await page.waitForTimeout(1_000);

    const deApi = vistas.filter((v) => v.caminho.startsWith('/api/'));
    expect(deApi.length, 'a pagina nao chamou nenhuma rota /api/').toBeGreaterThan(0);

    const semHeader = deApi.filter((v) => v.auth !== `Bearer ${CHAVE}`).map((v) => v.caminho);
    expect(semHeader, `chamadas /api/ sem o header: ${semHeader.join(', ')}`).toEqual([]);
  });

  test('sem chave, nada muda: nenhuma chamada leva Authorization', async ({ page }) => {
    const vistas = observarRequisicoes(page);
    await abrirConsole(page, null);
    await page.evaluate(() => fetch('/api/status').then((r) => r.status));
    await page.waitForTimeout(1_000);

    const comHeader = vistas.filter((v) => v.auth).map((v) => v.caminho);
    expect(comHeader, `chamadas com header sem chave configurada: ${comHeader.join(', ')}`).toEqual([]);
  });

  test('a chave nao vai junto do que esta fora de /api/', async ({ page }) => {
    const vistas = observarRequisicoes(page);
    await abrirConsole(page, CHAVE);
    await page.evaluate(async () => {
      await fetch('/health').catch(() => {});
      await fetch('/admin/api/me').catch(() => {});
    });
    await page.waitForTimeout(1_000);

    const foraDeApi = vistas.filter((v) => !v.caminho.startsWith('/api/') && v.auth);
    expect(
      foraDeApi.map((v) => v.caminho),
      'a chave saiu numa requisicao que nao e /api/',
    ).toEqual([]);
  });
});
