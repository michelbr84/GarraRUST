/**
 * #1422: Web Console — rejected WhatsApp messages, safely.
 *
 * Selectors are `data-testid` only (plan 0052); the testids are owned by
 * `crates/garraia-gateway/src/admin.html` (`waRejectionsCard`). The card reads
 * `rejections` from `/admin/api/whatsapp/access` and resets through
 * `POST /admin/api/whatsapp/access/rejections/reset`.
 *
 *  1. Login → WhatsApp Access → the card renders with a total and one badge
 *     per reason (all reasons, even at zero) and the retention note
 *  2. No identity longer than the last 4 digits appears in the card
 *  3. Reset is two clicks (Reset → Confirm) and lands on zero
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

async function goToAccessPage(page: Page) {
  await page.getByTestId('nav-whatsapp-access').click();
  await page.getByTestId('wa-rejections').waitFor({ state: 'visible', timeout: 10_000 });
}

test.describe('WhatsApp Access page: rejected messages', () => {
  test.beforeEach(async ({ page }) => {
    await login(page);
    await goToAccessPage(page);
  });

  test('renders the total, one badge per reason and the retention note', async ({ page }) => {
    await expect(page.getByTestId('wa-rejections-total')).toContainText('rejected');
    const reasons = ['restricted_policy', 'unresolved_lid', 'blocked_user', 'channel_disabled', 'prompt_injection'];
    for (const r of reasons) {
      await expect(page.locator(`[data-testid="wa-rejection-count"][data-reason="${r}"]`)).toHaveCount(1);
    }
    await expect(page.getByTestId('wa-rejections-retention')).toContainText('since the gateway booted');
    // Either the empty state or rows — never both missing.
    const empty = await page.getByTestId('wa-rejections-empty').count();
    const rows = await page.getByTestId('wa-rejection-row').count();
    expect(empty + rows).toBeGreaterThan(0);
  });

  test('never shows more than the last 4 digits of an identity', async ({ page }) => {
    const html = await page.getByTestId('wa-rejections').innerHTML();
    // A masked identity is `…1234`; anything with 5+ consecutive digits is a leak.
    expect(html).not.toMatch(/\d{5,}/);
    const rows = page.getByTestId('wa-rejection-row');
    const n = await rows.count();
    for (let i = 0; i < n; i++) {
      const last4 = await rows.nth(i).getAttribute('data-last4');
      expect(last4 ?? '').toMatch(/^…(\d{4}|\?{4})$/);
    }
  });

  test('reset takes two clicks and lands on zero', async ({ page }) => {
    const reset = page.getByTestId('wa-rejections-reset');
    if ((await reset.count()) === 0) test.skip(true, 'role cannot edit the policy');
    if (await reset.isDisabled()) {
      // Nothing to clear: the button is disabled by design (no implicit action).
      await expect(page.getByTestId('wa-rejections-total')).toHaveText('0 rejected');
      return;
    }
    await reset.click();
    await expect(page.getByTestId('wa-rejections-reset-confirm')).toBeVisible();
    await page.getByTestId('wa-rejections-reset-confirm').click();
    await expect(page.getByTestId('wa-rejections-total')).toHaveText('0 rejected', { timeout: 10_000 });
    await expect(page.getByTestId('wa-rejections-empty')).toBeVisible();
  });
});
