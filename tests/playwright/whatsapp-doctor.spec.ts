/**
 * #1420: Web Console — "Test WhatsApp" card on the WhatsApp Access page.
 *
 * Selectors are `data-testid` only (plan 0052): the UI testids are owned by
 * `crates/garraia-gateway/src/admin.html` (`waDoctorCard`, `waDoctorRender`,
 * `waDoctorAction`). The card calls `GET /admin/api/whatsapp/doctor`, which
 * runs the SAME engine as `garraia doctor whatsapp`; what this spec proves is
 * the operator flow, on a gateway whose WhatsApp channel is NOT configured
 * (the CI case):
 *
 *  1. Login → navigate → the card renders; "Run test" produces one row per
 *     check with a status in the diagnostics vocabulary
 *  2. Every warning/error row carries a next step and one safe action
 *  3. Terminal steps are shown/copied, never executed; navigation actions
 *     open the page where the problem is fixed
 *  4. Nothing that looks like a secret reaches the card's HTML
 */
import { test, expect, Page } from '@playwright/test';

const ADMIN_USER = process.env.GARRAIA_ADMIN_USER ?? 'admin';
const ADMIN_PASS = process.env.GARRAIA_ADMIN_PASS ?? 'admin123';

const STATUSES = ['ok', 'warning', 'error', 'not_configured'];
const CONFIG_LINES = ['whatsapp.access', 'execution.profile', 'files.workspace', 'mcp.visibility', 'provider.default'];
// Key-like tokens, bearer headers, or long opaque blobs. Env var NAMES that a
// next step may cite (GARRAIA_VAULT_PASSPHRASE, GARRAIA_GATEWAY_API_KEY) are
// short and underscore-only, so they never match.
const SECRET_PATTERNS = [
  /sk-[A-Za-z0-9_-]{8,}/,
  /Bearer\s+[A-Za-z0-9._~+/=-]{8,}/i,
  /[A-Za-z0-9+/]{40,}={0,2}/,
];

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
  await page.getByTestId('wa-access-summary').waitFor({ state: 'visible', timeout: 10_000 });
}

async function runDoctor(page: Page) {
  await page.getByTestId('wa-doctor-run').click();
  await page.getByTestId('wa-doctor-row').first().waitFor({ state: 'visible', timeout: 20_000 });
  return page.getByTestId('wa-doctor-row');
}

test.describe('WhatsApp doctor card', () => {
  test.beforeEach(async ({ page }) => {
    await login(page);
    await goToAccessPage(page);
  });

  test('runs the doctor and renders one row per check, each non-green row with a next step and an action', async ({ page }) => {
    await expect(page.getByTestId('wa-doctor')).toBeVisible();
    // The card sits at the top: right after the summary, before the matrix.
    const order = await page.evaluate(() => {
      const ids = ['wa-access-summary', 'wa-doctor', 'wa-access-principals'];
      const tops = ids.map((id) => document.querySelector(`[data-testid="${id}"]`)?.getBoundingClientRect().top ?? NaN);
      return tops;
    });
    expect(order[0]).toBeLessThan(order[1]);
    expect(order[1]).toBeLessThan(order[2]);

    const rows = await runDoctor(page);
    const n = await rows.count();
    expect(n).toBeGreaterThanOrEqual(5);

    const ids = await rows.evaluateAll((trs) => trs.map((t) => t.getAttribute('data-id')));
    expect(ids).toContain('whatsapp.linked');
    expect(ids).toContain('whatsapp.gateway');
    if (ids.includes('config')) {
      // The config did not load: the single `config` line stands in for the
      // five config-dependent ones.
      for (const id of CONFIG_LINES) expect(ids).not.toContain(id);
    } else {
      for (const id of CONFIG_LINES) expect(ids).toContain(id);
    }

    for (let i = 0; i < n; i++) {
      const row = rows.nth(i);
      const status = await row.getAttribute('data-status');
      expect(STATUSES).toContain(status);
      await expect(row.getByTestId('wa-doctor-status')).toBeVisible();
      if (status === 'warning' || status === 'error') {
        const step = ((await row.getByTestId('wa-doctor-next-step').textContent()) ?? '').trim();
        expect(step.length).toBeGreaterThan(1);
        expect(step).not.toBe('—');
        await expect(row.getByTestId('wa-doctor-action')).toHaveCount(1);
      }
    }

    const summary = page.getByTestId('wa-doctor-summary');
    await expect(summary).toBeVisible();
    expect(['ok', 'warning', 'error']).toContain(await summary.getAttribute('data-status'));
    await expect(summary).toContainText('v');
  });

  test('a terminal step is shown or copied, never executed; navigation actions open the fixing page', async ({ page }) => {
    await runDoctor(page);

    // Without a linked device `whatsapp.linked` is red and its step is the
    // link command — a terminal action, offered as text only.
    const linkedRow = page.locator('[data-testid="wa-doctor-row"][data-id="whatsapp.linked"]');
    await expect(linkedRow).toHaveCount(1);
    const linkedStatus = await linkedRow.getAttribute('data-status');
    if (linkedStatus === 'error' || linkedStatus === 'warning') {
      const step = ((await linkedRow.getByTestId('wa-doctor-next-step').textContent()) ?? '').trim();
      expect(step).toContain('whatsapp');
      let dialogText: string | null = null;
      page.on('dialog', async (d) => { dialogText = d.message(); await d.accept(); });
      const action = linkedRow.getByTestId('wa-doctor-action');
      await expect(action).toHaveText(/copy/i);
      await action.click();
      // Either the clipboard took it (toast) or the prompt fallback showed it;
      // in both cases the page stays where it is and nothing ran.
      await expect
        .poll(async () => dialogText !== null || (await page.locator('.toast').count()) > 0, { timeout: 5000 })
        .toBe(true);
      if (dialogText !== null) expect(dialogText).toContain(step.slice(0, 20));
      await expect(page.getByTestId('wa-doctor')).toBeVisible();
    }

    // A config-side line that is not green navigates to where it is fixed.
    const mcp = page.locator('[data-testid="wa-doctor-row"][data-id="mcp.visibility"]');
    if ((await mcp.count()) === 1 && (await mcp.getAttribute('data-status')) !== 'ok') {
      await mcp.getByTestId('wa-doctor-action').click();
      await expect(page.locator('#page-title')).toHaveText('MCP Servers');
      await goToAccessPage(page);
    }
  });

  test('nothing that looks like a secret reaches the card', async ({ page }) => {
    await runDoctor(page);
    const html = await page.getByTestId('wa-doctor').innerHTML();
    for (const re of SECRET_PATTERNS) expect(html).not.toMatch(re);
    // And no full phone number: the doctor only ever emits counts.
    expect(html).not.toMatch(/\+?\d{2}[\s()-]*\d{2}[\s()-]*9?\d{4}[\s-]?\d{4}/);
  });
});
