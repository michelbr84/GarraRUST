/**
 * GAR-300: Playwright E2E tests — MCP Manager Web UI
 *
 * Covers:
 *  1. Load /admin — login + navigate to MCP page
 *  2. Add MCP server via modal
 *  3. Verify token does NOT appear in plain text
 *  4. Edit existing MCP server
 *  5. Restart MCP server + visual status badge
 *  6. Delete MCP server (with confirm dialog)
 *  7. Error: submit form with empty name (validation)
 *  8. Error: duplicate server name
 *  9. Save as Template
 */

import { test, expect, Page } from '@playwright/test';

// ── helpers ──────────────────────────────────────────────────────────────────

const ADMIN_USER = process.env.GARRAIA_ADMIN_USER ?? 'admin';
const ADMIN_PASS = process.env.GARRAIA_ADMIN_PASS ?? 'admin123';

/** Login to admin panel and wait for dashboard */
async function login(page: Page) {
  await page.goto('/admin');

  // First-run setup or login form
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

  // Wait for the app shell to be visible
  await page.locator('#view-app').waitFor({ state: 'visible', timeout: 10_000 });
}

/** Navigate to the MCP page via sidebar */
async function goToMcpPage(page: Page) {
  await page.locator('[data-page="mcp"]').click();
  // Wait for content to load (the "+ Add MCP" button is always rendered)
  await page.getByRole('button', { name: '+ Add MCP' }).waitFor({ timeout: 8_000 });
}

/** Open the Add MCP form and return the form overlay locator */
async function openAddForm(page: Page) {
  await page.getByRole('button', { name: '+ Add MCP' }).click();
  // Form is inserted into #content-area as the first child card
  const form = page.locator('#content-area .card').first();
  await expect(form.locator('h3')).toContainText('Add MCP Server');
  return form;
}

// ── tests ─────────────────────────────────────────────────────────────────────

test.describe('MCP Manager Web UI (GAR-300)', () => {
  test.beforeEach(async ({ page }) => {
    await login(page);
    await goToMcpPage(page);
  });

  // ── 1. Page loads ──────────────────────────────────────────────────────────
  test('1. MCP page loads with Add button and template gallery', async ({ page }) => {
    await expect(page.getByRole('button', { name: '+ Add MCP' })).toBeVisible();
    // Page title — #page-title is the dynamic span updated by the admin JS
    // (admin.html:545 + 708). The previous combined selector with `.first()`
    // picked `<h1>🦜 GarraIA</h1>` from the banner because `.first()` takes
    // the first DOM-order match of any of the comma-separated selectors, not
    // the first in the selector list. Fixed in plan 0050 Lote 2 (GAR-438).
    await expect(page.locator('#page-title')).toContainText(/MCP/i);
    // Template gallery section
    await expect(page.locator('#content-area')).toContainText(/template/i);
  });

  // ── 2. Add MCP server ──────────────────────────────────────────────────────
  test('2. Add MCP server via modal', async ({ page }) => {
    const serverName = `e2e-test-${Date.now()}`;
    const form = await openAddForm(page);

    await form.locator('input[placeholder*="e.g. my-tool"]').fill(serverName);
    await form.locator('select').selectOption('stdio');
    await form.locator('input[placeholder*="Command"]').fill('echo');
    await form.locator('input[type="password"]').fill('sk-test-secret-token');
    await form.getByRole('button', { name: 'Add Server' }).click();

    // Toast "added" + form dismissed
    await expect(page.locator('.toast, [class*="toast"]').first()).toContainText(/added/i, { timeout: 8_000 });

    // Server appears in table
    await expect(page.locator('#content-area')).toContainText(serverName, { timeout: 10_000 });
  });

  // ── 3. Token not visible in plain text ─────────────────────────────────────
  test('3. Token does NOT appear in plain text in the UI', async ({ page }) => {
    const serverName = `e2e-token-check-${Date.now()}`;
    const plainToken = `sk-plaintext-token-${Date.now()}`;

    const form = await openAddForm(page);
    await form.locator('input[placeholder*="e.g. my-tool"]').fill(serverName);
    await form.locator('select').selectOption('stdio');
    await form.locator('input[placeholder*="Command"]').fill('echo');
    await form.locator('input[type="password"]').fill(plainToken);
    await form.getByRole('button', { name: 'Add Server' }).click();
    await page.locator('.toast, [class*="toast"]').first().waitFor({ timeout: 8_000 });

    // Navigate away and back to force a fresh load from API
    await page.locator('[data-page="dashboard"]').click();
    await goToMcpPage(page);

    const pageContent = await page.locator('#content-area').textContent();
    expect(pageContent).not.toContain(plainToken);

    // Cleanup
    const row = page.locator('#content-area').getByText(serverName).first().locator('..').locator('..');
    const deleteBtn = row.getByRole('button', { name: 'Delete' });
    page.once('dialog', d => d.accept());
    await deleteBtn.click();
  });

  // ── 4. Edit MCP server ─────────────────────────────────────────────────────
  test('4. Edit existing MCP server (timeout change)', async ({ page }) => {
    // Create a server to edit
    const serverName = `e2e-edit-${Date.now()}`;
    let form = await openAddForm(page);
    await form.locator('input[placeholder*="e.g. my-tool"]').fill(serverName);
    await form.locator('select').selectOption('stdio');
    await form.locator('input[placeholder*="Command"]').fill('echo');
    await form.getByRole('button', { name: 'Add Server' }).click();
    await page.locator('.toast, [class*="toast"]').first().waitFor({ timeout: 8_000 });

    // Find the Edit button for our server
    const serverRow = page.locator('#content-area').getByText(serverName).first().locator('../..');
    await serverRow.getByRole('button', { name: 'Edit' }).click();

    // Form in edit mode
    const editForm = page.locator('#content-area .card').first();
    await expect(editForm.locator('h3')).toContainText('Edit MCP Server');

    // Change timeout
    await editForm.locator('input[type="number"]').fill('60');
    await editForm.getByRole('button', { name: 'Save Changes' }).click();
    await expect(page.locator('.toast, [class*="toast"]').first()).toContainText(/updated/i, { timeout: 8_000 });
  });

  // ── 5. Restart MCP server ──────────────────────────────────────────────────
  test('5. Restart MCP server — status badge visible', async ({ page }) => {
    // Create a throwaway server
    const serverName = `e2e-restart-${Date.now()}`;
    const form = await openAddForm(page);
    await form.locator('input[placeholder*="e.g. my-tool"]').fill(serverName);
    await form.locator('select').selectOption('stdio');
    await form.locator('input[placeholder*="Command"]').fill('echo');
    await form.getByRole('button', { name: 'Add Server' }).click();
    await page.locator('.toast, [class*="toast"]').first().waitFor({ timeout: 8_000 });

    // Click Restart
    const serverRow = page.locator('#content-area').getByText(serverName).first().locator('../..');
    await serverRow.getByRole('button', { name: 'Restart' }).click();

    // Toast should appear
    await expect(page.locator('.toast, [class*="toast"]').first()).toBeVisible({ timeout: 5_000 });

    // Status badge is rendered in the row (running / stopped / error / starting)
    const badge = serverRow.locator('[class*="badge"], .status-badge, span').first();
    await expect(badge).toBeVisible();
  });

  // ── 6. Delete MCP server ──────────────────────────────────────────────────
  test('6. Delete MCP server (confirm dialog)', async ({ page }) => {
    const serverName = `e2e-delete-${Date.now()}`;
    const form = await openAddForm(page);
    await form.locator('input[placeholder*="e.g. my-tool"]').fill(serverName);
    await form.locator('select').selectOption('stdio');
    await form.locator('input[placeholder*="Command"]').fill('echo');
    await form.getByRole('button', { name: 'Add Server' }).click();
    await page.locator('.toast, [class*="toast"]').first().waitFor({ timeout: 8_000 });

    // Accept the confirm() dialog automatically
    page.once('dialog', dialog => dialog.accept());

    const serverRow = page.locator('#content-area').getByText(serverName).first().locator('../..');
    await serverRow.getByRole('button', { name: 'Delete' }).click();

    await expect(page.locator('.toast, [class*="toast"]').first()).toContainText(/deleted/i, { timeout: 8_000 });
    await expect(page.locator('#content-area')).not.toContainText(serverName, { timeout: 5_000 });
  });

  // ── 7. Validation: empty name ─────────────────────────────────────────────
  test('7. Validation: submitting form with empty name shows error', async ({ page }) => {
    const form = await openAddForm(page);
    // Leave name blank — submit directly
    await form.getByRole('button', { name: 'Add Server' }).click();

    // An inline error message should appear (not a toast — stays in form)
    const errEl = form.locator('div[style*="color"]');
    await expect(errEl).toContainText(/name.*required|required.*name/i, { timeout: 3_000 });
  });

  // ── 8. Cancel closes form ─────────────────────────────────────────────────
  test('8. Cancel button closes form without adding server', async ({ page }) => {
    const form = await openAddForm(page);
    await form.locator('input[placeholder*="e.g. my-tool"]').fill('should-not-be-added');
    await form.getByRole('button', { name: 'Cancel' }).click();

    // Form should be gone
    await expect(page.locator('#content-area .card h3')).not.toContainText('Add MCP Server');
    // No server was added
    await expect(page.locator('#content-area')).not.toContainText('should-not-be-added');
  });

  // ── 9. Save as Template ───────────────────────────────────────────────────
  test('9. Save as Template from Add form', async ({ page }) => {
    const tplName = `e2e-tpl-${Date.now()}`;
    const form = await openAddForm(page);
    await form.locator('input[placeholder*="e.g. my-tool"]').fill(tplName);
    await form.locator('select').selectOption('stdio');
    await form.locator('input[placeholder*="Command"]').fill('echo');

    // Click "Save as Template" button
    await form.getByRole('button', { name: /save as template/i }).click();

    // Template saved toast
    await expect(page.locator('.toast, [class*="toast"]').first()).toContainText(/template.*saved|saved.*template/i, { timeout: 5_000 });

    // Cancel the form to get back to gallery view
    await form.getByRole('button', { name: 'Cancel' }).click();

    // Template appears in gallery
    await expect(page.locator('#content-area')).toContainText(tplName, { timeout: 5_000 });
  });
});
