/**
 * #1409 / #1415: Web Console — Sessions page shows, per session, the
 * principal, the EFFECTIVE mode and the active project (name only), and the
 * "Capabilities" button opens the capabilities panel of THAT conversation.
 *
 * Selectors are `data-testid` only (plan 0052); the testids are owned by
 * `crates/garraia-gateway/src/admin.html` (`pageSessions`,
 * `showSessionCapabilities`). The page reads `/admin/api/sessions` and
 * `/admin/api/capabilities?session_id=`, the same registry `garra_status`
 * returns to the model.
 *
 *  1. Login → Sessions → the three new columns render
 *  2. A session exists (seeded through `POST /api/sessions` with mode
 *     `search` when the gateway has none) → its row shows a mode and a
 *     project cell, never a filesystem path
 *  3. "Capabilities" on a DEDICATED seeded `search` session (pinned by id,
 *     not `.first()`, so spec order cannot turn the checks off) → panel with title, counts and rows; `file_write` is
 *     `denied` under `search`; Close hides it
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

async function goToSessions(page: Page) {
  await page.getByTestId('nav-sessions').click();
  await page.locator('#page-title').waitFor({ state: 'visible', timeout: 10_000 }).catch(() => {});
  // The page renders after the fetch: the sessions table, or the empty state
  // when the gateway has no session yet (the order of the specs decides that
  // in CI, so never assume one or the other).
  await expect
    .poll(async () => (await page.getByTestId('sessions-table').count()) + (await page.getByTestId('sessions-empty').count()), { timeout: 15_000 })
    .toBeGreaterThan(0);
}

/** Seeds one web session in mode `search` if the console shows none. Best effort:
 *  the assertions below skip (never fail) when the environment has no session. */
async function ensureASession(page: Page): Promise<boolean> {
  if ((await page.getByTestId('session-row').count()) > 0) return true;
  const resp = await page.request.post('/api/sessions', { data: { mode: 'search' } }).catch(() => null);
  if (!resp || !resp.ok()) return false;
  await goToSessions(page);
  return (await page.getByTestId('session-row').count()) > 0;
}

/** Seeds a DEDICATED web session in mode `search` and returns its id, or
 *  `null` when the environment does not allow seeding. The capabilities test
 *  pins its row by this id instead of taking `.first()`: with `.first()` the
 *  #1915 assertions sat behind `if (modo === 'search')` and silently stopped
 *  running whenever another spec created a session in another mode first. */
async function seedSearchSession(page: Page): Promise<string | null> {
  const resp = await page.request.post('/api/sessions', { data: { mode: 'search' } }).catch(() => null);
  if (!resp || !resp.ok()) return null;
  const body = await resp.json().catch(() => null);
  const id = body && typeof body.session_id === 'string' ? body.session_id : null;
  if (!id) return null;
  await goToSessions(page);
  return id;
}

test.describe('Sessions page: principal, effective mode and capabilities', () => {
  test.beforeEach(async ({ page }) => {
    await login(page);
    await goToSessions(page);
  });

  test('renders the principal, effective mode and project columns', async ({ page }) => {
    // The columns only exist when there is a table, i.e. at least one session.
    if (!(await ensureASession(page))) test.skip(true, 'no session in this environment and seeding is not allowed');
    const headers = page.getByTestId('sessions-table').locator('thead th');
    await expect(headers.filter({ hasText: 'Principal' })).toHaveCount(1);
    await expect(headers.filter({ hasText: 'Effective mode' })).toHaveCount(1);
    await expect(headers.filter({ hasText: 'Project' })).toHaveCount(1);
  });

  test('a session row shows a mode and a project cell, never a path', async ({ page }) => {
    if (!(await ensureASession(page))) test.skip(true, 'no session in this environment and seeding is not allowed');
    const row = page.getByTestId('session-row').first();
    await expect(row.getByTestId('session-effective-mode')).not.toHaveText('');
    await expect(row.getByTestId('session-project')).not.toHaveText('');
    await expect(row.getByTestId('session-principal')).not.toHaveText('');
    // The project cell is a name (or a placeholder), never a filesystem path.
    const projeto = (await row.getByTestId('session-project').textContent()) ?? '';
    expect(projeto).not.toMatch(/^\/|^[A-Za-z]:\\/);
    // A web session has no WhatsApp principal.
    const canal = (await row.textContent()) ?? '';
    if (!canal.includes('whatsapp_linked')) await expect(row.getByTestId('session-principal')).toHaveText('—');
  });

  test('Capabilities opens the panel of that conversation, file_write denied under search, Close hides it', async ({ page }) => {
    const sessionId = await seedSearchSession(page);
    if (!sessionId) test.skip(true, 'seeding a search session is not allowed in this environment');
    const row = page.locator(`[data-testid="session-row"][data-session-id="${sessionId}"]`);
    await expect(row).toHaveCount(1);
    // The seeded session is in `search`: the assertions below are unconditional.
    await expect(row.getByTestId('session-effective-mode')).toHaveText('search');
    await row.getByTestId('session-capabilities').click();
    const panel = page.getByTestId('session-capabilities-panel');
    await expect(panel).toBeVisible();
    await expect(page.getByTestId('session-capabilities-title')).toContainText('Capabilities of');
    await expect(page.getByTestId('session-capabilities-title')).toContainText((sessionId ?? '').slice(0, 12));
    await expect(page.getByTestId('session-capabilities-counts')).toContainText('visible');
    await expect
      .poll(async () => await page.getByTestId('session-capability-row').count(), { timeout: 15_000 })
      .toBeGreaterThan(0);
    // Under `search` file_write is denied, never "absent".
    const fileWrite = page.locator('[data-testid="session-capability-row"][data-capability="file_write"]');
    await expect(fileWrite).toHaveCount(1);
    await expect(fileWrite).toHaveAttribute('data-state', 'denied');
    // #1915: the row names the layer that denied it, not just "policy".
    await expect(fileWrite).toHaveAttribute('data-denied-by', /^(mode|mode_and_principal_ceiling)$/);
    await expect(fileWrite.getByTestId('session-capability-denied-by')).toContainText('session mode');
    // The panel never carries a filesystem path or a secret-looking value.
    const html = (await panel.innerHTML()) ?? '';
    expect(html).not.toMatch(/\/home\/|\/Users\/|[A-Za-z]:\\Users/);
    await page.getByTestId('session-capabilities-close').click();
    await expect(page.getByTestId('session-capabilities-title')).toHaveCount(0);
  });
});
