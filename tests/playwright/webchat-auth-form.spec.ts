/**
 * #1100 (PR #1101): com a autenticacao exigida, o form de gateway key
 * aparece para o usuario digitar a chave.
 *
 * O CSS esconde `.gateway-key-form` com `display: none`. Antes do fix o boot
 * limpava o estilo inline (`display = ''`), que reverteria ao CSS escondido —
 * o usuario via "Gateway API Key required." sem nenhum campo visivel para
 * digitar a chave, e o WebSocket reconectava em loop com 401 (#1100).
 *
 * Os mocks `page.route` interceptam no browser, entao o spec funciona tanto
 * contra o gateway real do CI quanto em verificacao local com o console
 * servido estaticamente (o webchat.html e auto-contido, ports inline).
 */
import { test, expect } from '@playwright/test';

test.describe('#1100 — form de gateway key', () => {
  test('auth exigida no boot revela o form e avisa em vez de conectar', async ({ page }) => {
    await page.route('**/api/auth-check', (route) =>
      route.fulfill({
        status: 200,
        contentType: 'application/json',
        body: JSON.stringify({ auth_required: true }),
      }),
    );

    await page.goto('/');
    // Sem chave guardada, o boot nao tenta conectar — ele mostra o form.
    await expect(page.locator('#auth-section')).toBeVisible({ timeout: 10_000 });
    await expect(page.locator('#chat-messages')).toContainText('Gateway API Key required.');
  });

  test('auth nao exigida deixa o form escondido', async ({ page }) => {
    await page.route('**/api/auth-check', (route) =>
      route.fulfill({
        status: 200,
        contentType: 'application/json',
        body: JSON.stringify({ auth_required: false }),
      }),
    );

    await page.goto('/');
    await expect(page.locator('#auth-section')).toBeHidden({ timeout: 10_000 });
  });
});
