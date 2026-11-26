/**
 * #1996: o roteador do Web Console (`navigate()` em
 * `crates/garraia-gateway/src/admin.html`) so despacha para paginas que o
 * mapa `pages` declara como propriedade PROPRIA.
 *
 * `page` vem de `location.hash`, ou seja, de qualquer link que o admin abrir.
 * Antes, o filtro era `if (!pages[page])`, e `pages` e um objeto literal: os
 * nomes herdados de `Object.prototype` passavam. `#__proto__` lancava
 * `TypeError` e deixava a tela presa no "loading"; `#constructor` e
 * `#toString` nao lancavam, mas nao desenhavam pagina nenhuma.
 *
 * A prova e o titulo e o hash, nao a ausencia de `pageerror`: o dashboard
 * carrega o Chart.js de CDN, e num runner sem rede ele lanca
 * `ReferenceError` por conta propria. No caminho antigo o titulo ja denuncia
 * o defeito — `pageTitle('__proto__')` devolvia o proprio `Object.prototype`.
 *
 * O contrato afirmado aqui e o do roteador, por isso os seletores sao os ids
 * estruturais da casca (`#page-title`, `#view-app`) e o proprio
 * `location.hash` — nenhum texto de botao ou placeholder (plan 0052).
 */
import { test, expect, Page } from '@playwright/test';

const ADMIN_USER = process.env.GARRAIA_ADMIN_USER ?? 'admin';
const ADMIN_PASS = process.env.GARRAIA_ADMIN_PASS ?? 'admin123';

async function login(page: Page) {
  await page.goto('/admin');
  const setupBtn = page.locator('#setup-btn');
  const loginBtn = page.locator('#login-btn');
  if (await setupBtn.isVisible({ timeout: 3000 }).catch(() => false)) {
    await page.fill('#setup-username', ADMIN_USER);
    await page.fill('#setup-password', ADMIN_PASS);
    await page.fill('#setup-password2', ADMIN_PASS);
    await setupBtn.click();
  } else {
    await loginBtn.waitFor({ state: 'visible', timeout: 10_000 });
    await page.fill('#login-username', ADMIN_USER);
    await page.fill('#login-password', ADMIN_PASS);
    await loginBtn.click();
  }
  await page.locator('#view-app').waitFor({ state: 'visible', timeout: 10_000 });
}

/** Nomes que todo objeto literal herda de `Object.prototype`. */
const HERDADOS = ['__proto__', 'constructor', 'toString', 'hasOwnProperty', 'valueOf'];

test.describe('Roteador do admin: hash com nome herdado de Object.prototype', () => {
  for (const nome of HERDADOS) {
    test(`#${nome} cai no dashboard`, async ({ page }) => {
      await login(page);
      // Parte de uma pagina que nao e o dashboard: so assim "caiu no
      // dashboard" prova que o roteador decidiu, e nao que nada aconteceu.
      await page.evaluate(() => { location.hash = '#about'; });
      await expect(page.locator('#page-title')).toHaveText('About');

      await page.evaluate((h) => { location.hash = '#' + h; }, nome);

      await expect(page.locator('#page-title')).toHaveText('Dashboard');
      await expect.poll(() => page.evaluate(() => location.hash)).toBe('#dashboard');
    });
  }

  test('hash desconhecido continua caindo no dashboard', async ({ page }) => {
    await login(page);
    await page.evaluate(() => { location.hash = '#about'; });
    await expect(page.locator('#page-title')).toHaveText('About');
    await page.evaluate(() => { location.hash = '#pagina_que_nao_existe'; });
    await expect(page.locator('#page-title')).toHaveText('Dashboard');
  });

  test('abrir o painel direto em #__proto__ nao trava no loading', async ({ page }) => {
    await login(page);
    // `goto` para a mesma URL com outro hash e navegacao no mesmo documento;
    // o `reload` e que refaz o boot (`init()` -> `showApp()`), o caminho de
    // quem abre o link numa aba nova com a sessao ativa.
    await page.goto('/admin#__proto__');
    await page.reload();
    await page.locator('#view-app').waitFor({ state: 'visible', timeout: 10_000 });

    await expect(page.locator('#page-title')).toHaveText('Dashboard');
    await expect.poll(() => page.evaluate(() => location.hash)).toBe('#dashboard');
  });
});
