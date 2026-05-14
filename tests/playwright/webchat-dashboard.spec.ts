/**
 * GAR-618 / plan 0117 (PR-4): Playwright E2E tests — Garra Glass Dashboard page
 *
 * Covers:
 *  1. Dashboard nav button is visible in sidebar (data-testid="garra-dashboard-nav")
 *  2. Clicking Dashboard nav shows dashboard section and hides chat area
 *  3. Dashboard section renders metric cards
 *  4. Chat nav button returns to chat view
 *  5. GET /api/stats returns valid JSON shape
 */

import { test, expect, Page } from '@playwright/test';

// ── helpers ──────────────────────────────────────────────────────────────────

/** Navigate to the webchat UI */
async function openWebchat(page: Page) {
  await page.goto('/');
  // Wait for the main app shell
  await page.locator('.sidebar').waitFor({ state: 'visible', timeout: 10_000 });
}

// ── tests ─────────────────────────────────────────────────────────────────────

test.describe('Garra Glass Dashboard (GAR-618 PR-4)', () => {
  test.beforeEach(async ({ page }) => {
    await openWebchat(page);
  });

  // ── 1. Dashboard nav button visible ──────────────────────────────────────
  test('1. Dashboard nav button is visible in sidebar', async ({ page }) => {
    await expect(page.getByTestId('garra-dashboard-nav')).toBeVisible();
  });

  // ── 2. Clicking Dashboard nav reveals dashboard section ──────────────────
  test('2. Clicking Dashboard nav shows dashboard section and hides chat area', async ({ page }) => {
    await page.getByTestId('garra-dashboard-nav').click();
    // main uses data-testid="garra-page-dashboard" for the section
    await expect(page.getByTestId('garra-page-dashboard')).toBeVisible();
    // chat-area is a page-view that becomes inactive
    await expect(page.locator('.chat-area')).not.toHaveClass(/\bactive\b/);
  });

  // ── 3. Dashboard section renders metric cards ─────────────────────────────
  test('3. Dashboard section renders at least 4 metric cards', async ({ page }) => {
    await page.getByTestId('garra-dashboard-nav').click();
    await expect(page.getByTestId('garra-page-dashboard')).toBeVisible();
    // main renders 5 metric cards in the dashboard metrics-grid
    const cards = page.locator('[data-testid="garra-page-dashboard"] .metric-card');
    // wait for at least the first card to be in the DOM
    await expect(cards.first()).toBeVisible();
    expect(await cards.count()).toBeGreaterThanOrEqual(4);
  });

  // ── 4. Navigating back to chat restores chat area ────────────────────────
  test('4. Chat nav button returns to chat view', async ({ page }) => {
    await page.getByTestId('garra-dashboard-nav').click();
    await expect(page.getByTestId('garra-page-dashboard')).toBeVisible();

    // Use the sidebar-page-btn specifically — the header nav also has
    // data-page="chat" but has no click handler wired by setupPageRouter
    await page.locator('.sidebar-page-btn[data-page="chat"]').click();
    await expect(page.locator('.chat-area')).toHaveClass(/\bactive\b/);
    await expect(page.getByTestId('garra-page-dashboard')).not.toHaveClass(/\bactive\b/);
  });
});

// ── /api/stats shape test ─────────────────────────────────────────────────────

test.describe('GET /api/stats endpoint (GAR-618 PR-4)', () => {
  test('returns valid JSON with required fields', async ({ request }) => {
    const resp = await request.get('/api/stats');
    expect(resp.status()).toBe(200);
    const body = await resp.json();
    expect(typeof body.version).toBe('string');
    expect(typeof body.uptime_secs).toBe('number');
    expect(typeof body.active_sessions).toBe('number');
    expect(typeof body.gateway_status).toBe('string');
    expect(body.gateway_status).toBe('online');
  });
});
