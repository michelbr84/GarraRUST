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
