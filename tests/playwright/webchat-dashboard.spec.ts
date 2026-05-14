/**
 * GAR-618 / plan 0117 (PR-4): Playwright E2E tests — Garra Glass Dashboard page
 *
 * Covers:
 *  1. Dashboard nav button is visible in sidebar
 *  2. Clicking Dashboard nav shows dashboard section and hides chat area
 *  3. All 4 MetricCards render with their data-testid anchors
 *  4. Chat nav returns to chat view
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
    await expect(page.getByTestId('garra-dashboard-section')).toBeVisible();
    await expect(page.locator('.chat-area')).toBeHidden();
  });

  // ── 3. All 4 MetricCards present ─────────────────────────────────────────
  test('3. All 4 MetricCards render with correct data-testid anchors', async ({ page }) => {
    await page.getByTestId('garra-dashboard-nav').click();
    await expect(page.getByTestId('metric-card-status')).toBeVisible();
    await expect(page.getByTestId('metric-card-uptime')).toBeVisible();
    await expect(page.getByTestId('metric-card-version')).toBeVisible();
    await expect(page.getByTestId('metric-card-sessions')).toBeVisible();
  });

  // ── 4. Navigating back to chat restores chat area ────────────────────────
  test('4. Chat nav button returns to chat view', async ({ page }) => {
    await page.getByTestId('garra-dashboard-nav').click();
    await expect(page.getByTestId('garra-dashboard-section')).toBeVisible();

    // Click a nav button that has data-section="chat" (or any that isn't dashboard)
    const chatNavBtn = page.locator('[data-section="chat"]').first();
    if (await chatNavBtn.count() > 0) {
      await chatNavBtn.click();
    } else {
      // Fallback: click the logo / new-chat button which triggers showSection('chat')
      await page.locator('.sidebar-logo, #new-chat-btn').first().click();
    }
    await expect(page.locator('.chat-area')).toBeVisible();
    await expect(page.getByTestId('garra-dashboard-section')).toBeHidden();
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
