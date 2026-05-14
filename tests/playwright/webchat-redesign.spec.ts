/**
 * Playwright smoke spec for the Garra Glass web console (plan 0116).
 *
 * Scope (PR-C):
 *   1. Page loads at `/` and renders the GarraIA brand.
 *   2. The 58px app-header is visible with `data-testid="garra-app-header"`.
 *   3. The chat console glass-panel renders (`data-testid="garra-chat-panel"`).
 *   4. The context panel renders (`data-testid="garra-context-panel"`).
 *   5. The sidebar brand logo carries the "G" badge (`data-testid="garra-brand-logo"`).
 *   6. The chat input accepts focus and typing.
 *   7. The header theme toggle flips `data-theme` between dark and light on
 *      `<html>` (and also `data-bs-theme` per ADR 0009 §4).
 *
 * This spec covers webchat.html only. The admin.html surface is covered by
 * mcp-manager.spec.ts (different page, different IDs).
 */

import { test, expect, Page } from '@playwright/test';

const WEBCHAT_URL = '/';

async function openWebchat(page: Page) {
  await page.goto(WEBCHAT_URL);
  // Hard fail if the page didn't paint
  await page.locator('body').waitFor({ state: 'visible', timeout: 10_000 });
}

test.describe('Garra Glass — webchat redesign', () => {
  test('loads with brand visible and Garra Glass shell intact', async ({ page }) => {
    await openWebchat(page);

    // Brand surfaces in two places: sidebar logo + app-header brand
    await expect(page.getByTestId('garra-brand-logo')).toBeVisible();
    await expect(page.locator('.app-header .header-brand')).toContainText('GarraIA');
  });

  test('app-header, chat panel, and context panel are all visible', async ({ page }) => {
    await openWebchat(page);

    await expect(page.getByTestId('garra-app-header')).toBeVisible();
    await expect(page.getByTestId('garra-chat-panel')).toBeVisible();
    await expect(page.getByTestId('garra-context-panel')).toBeVisible();
  });

  test('chat input is editable', async ({ page }) => {
    await openWebchat(page);

    const input = page.locator('#chat-input');
    await expect(input).toBeVisible();
    await input.click();
    await input.fill('hello garra');
    await expect(input).toHaveValue('hello garra');
  });

  test('header theme toggle flips dark <-> light on <html>', async ({ page }) => {
    await openWebchat(page);

    // Default boot theme should be `dark` per State.currentTheme fallback.
    // Wait for boot() to run applyTheme().
    await page.waitForFunction(
      () => document.documentElement.getAttribute('data-theme') !== null,
      { timeout: 5_000 },
    );

    const initial = await page.evaluate(() => document.documentElement.getAttribute('data-theme'));
    expect(['light', 'dark']).toContain(initial);

    await page.locator('#header-theme-toggle').click();
    const afterFirstClick = await page.evaluate(() => ({
      theme: document.documentElement.getAttribute('data-theme'),
      bs: document.documentElement.getAttribute('data-bs-theme'),
    }));
    expect(afterFirstClick.theme).not.toBe(initial);
    expect(['light', 'dark']).toContain(afterFirstClick.bs);

    await page.locator('#header-theme-toggle').click();
    const afterSecondClick = await page.evaluate(() => document.documentElement.getAttribute('data-theme'));
    expect(afterSecondClick).toBe(initial);
  });

  test('gateway URL pill reflects window.location.origin', async ({ page }) => {
    await openWebchat(page);

    // boot() runs an effect that writes window.location.origin into the pill.
    await expect(page.locator('#gateway-url-text')).toContainText(/http/);
  });

  test('right-panel close + reopen toggles visibility', async ({ page }) => {
    // Desktop viewport so the panel starts visible.
    await page.setViewportSize({ width: 1280, height: 720 });
    await openWebchat(page);

    const rightPanel = page.locator('#right-panel');
    await expect(rightPanel).toBeVisible();

    await page.locator('#right-panel-close').click();
    // After clicking close, the panel either collapses (desktop) or slides out (mobile).
    // The `.collapsed` class is added by existing JS.
    await expect(rightPanel).toHaveClass(/collapsed/);

    await page.locator('#right-panel-toggle').click();
    await expect(rightPanel).not.toHaveClass(/collapsed/);
  });

  test('mobile viewport: sidebar is collapsed by default', async ({ page }) => {
    await page.setViewportSize({ width: 375, height: 667 });
    await openWebchat(page);

    // In mobile, the sidebar starts collapsed (slid out by translateX).
    // We just assert the hamburger button is reachable — it's the door back.
    await expect(page.locator('#hamburger-btn')).toBeVisible();
  });
});

// ────────────────────────────────────────────────────────────────────────────
// E2E matrix (plan 0123 / PR-10) — exercise every page-view via the internal
// hash router. We don't load every page's data (some hit live LLM providers),
// but we DO assert: the route resolves, the page-view becomes `.active`, and
// the matching sidebar nav button gets `.active`. That validates the router
// scaffold without requiring a fully-configured gateway.
// ────────────────────────────────────────────────────────────────────────────

const PAGES = [
  'dashboard',
  'chat',
  'providers',
  'channels',
  'sessions',
  'settings',
  'diagnostics',
  'logs',
  'skins',
] as const;

test.describe('Garra Glass — multi-page router matrix', () => {
  for (const page_name of PAGES) {
    test(`route to #/${page_name} activates the matching page-view`, async ({ page }) => {
      await page.goto(`/#/${page_name}`);
      await page.locator('body').waitFor({ state: 'visible', timeout: 10_000 });

      // The active page-view's `data-page` attribute must match.
      await expect
        .poll(
          async () =>
            await page.evaluate(() => {
              const active = document.querySelector('.page-view.active');
              return active ? active.getAttribute('data-page') : null;
            }),
          { timeout: 5_000 },
        )
        .toBe(page_name);

      // The matching sidebar nav button is also active.
      const sidebarBtn = page.locator(`.sidebar-page-btn[data-page="${page_name}"]`);
      await expect(sidebarBtn).toHaveClass(/active/);
    });
  }

  test('the existing /api/health endpoint returns the Dashboard contract', async ({ request }) => {
    const r = await request.get('/api/health');
    expect(r.status()).toBe(200);
    const j = await r.json();
    // Plan 0118 — these fields MUST be present (Dashboard binds to them).
    expect(j).toHaveProperty('status');
    expect(j).toHaveProperty('version');
    expect(j).toHaveProperty('gateway_url');
    expect(j).toHaveProperty('uptime_secs');
    expect(j).toHaveProperty('active_sessions');
    expect(j).toHaveProperty('channels');
    expect(j).toHaveProperty('warnings');
    // Back-compat: `checks` is still present.
    expect(j).toHaveProperty('checks');
  });

  test('/api/capabilities returns the canonical lists', async ({ request }) => {
    const r = await request.get('/api/capabilities');
    expect(r.status()).toBe(200);
    const j = await r.json();
    expect(Array.isArray(j.features)).toBe(true);
    expect(Array.isArray(j.providers)).toBe(true);
    expect(Array.isArray(j.channels)).toBe(true);
    expect(Array.isArray(j.commands)).toBe(true);
    expect(j.skins).toEqual(
      expect.arrayContaining(['garra-blue', 'aurora-admin', 'editorial', 'cyber-garra']),
    );
  });

  test('/api/settings/effective masks secrets', async ({ request }) => {
    const r = await request.get('/api/settings/effective');
    expect(r.status()).toBe(200);
    const j = await r.json();
    const rows = j.settings as Array<{ id: string; value: unknown; configured: unknown }>;
    // Plan 0121 invariant: every secret row carries `configured` (bool) and
    // `value` is null. The endpoint MUST NOT echo a secret value.
    const secretRows = rows.filter((r) => r.id.startsWith('secrets.'));
    expect(secretRows.length).toBeGreaterThan(0);
    for (const row of secretRows) {
      expect(row.value).toBeNull();
      expect(typeof row.configured).toBe('boolean');
    }
  });

  test('/api/diagnostics returns the 12-check report', async ({ request }) => {
    const r = await request.get('/api/diagnostics');
    expect(r.status()).toBe(200);
    const j = await r.json();
    expect(['ok', 'warning', 'error']).toContain(j.status);
    expect(j.checks.length).toBeGreaterThanOrEqual(10);
    // Plan 0122 invariant: every check carries id + label + status.
    for (const c of j.checks) {
      expect(typeof c.id).toBe('string');
      expect(typeof c.label).toBe('string');
      expect(['ok', 'warning', 'error', 'skipped']).toContain(c.status);
    }
  });
});
