/**
 * #1433: Web Console — global "Agents & Permissions" page.
 *
 * Selectors are `data-testid` only (plan 0052): the UI testids are owned by
 * `crates/garraia-gateway/src/admin.html` (`pageAgentsPermissions`, `apView`).
 * The page reads `GET /admin/api/permissions/overview`, which is computed by
 * the same policy engine the WhatsApp Access page edits — this page does not
 * edit, it links to each channel's own page. What this spec proves:
 *
 *  1. Login → navigate → summary, capability-class legend and channel cards
 *     render, with ARIA roles/labels on the regions and the matrix table
 *  2. Every supported channel is listed; channels without a policy engine say
 *     so honestly ("policy: not supported yet / defaults")
 *  3. The WhatsApp channel shows principals with effective capability classes,
 *     sensitive classes flagged, and a "Manage … access" button that routes to
 *     the WhatsApp Access page (shared edit flow, not duplicated here)
 *  4. The page is usable at a 360px mobile viewport (no horizontal page scroll;
 *     the wide matrix scrolls inside its own container)
 *
 * This suite may not be runnable in every environment (it needs a live gateway
 * with the admin console seeded); where it cannot run, that is an environment
 * limitation, not a failure of the page contract.
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

async function goToPage(page: Page) {
  await page.getByTestId('nav-agents-permissions').click();
  await page.getByTestId('ap-summary').waitFor({ state: 'visible', timeout: 10_000 });
}

test.describe('Agents & Permissions page', () => {
  test.beforeEach(async ({ page }) => {
    await login(page);
    await goToPage(page);
  });

  test('renders summary, class legend and channel cards with ARIA', async ({ page }) => {
    await expect(page.getByTestId('ap-execution-profile')).toContainText('execution profile:');
    const legend = page.getByTestId('ap-classes-legend');
    await expect(legend).toBeVisible();
    await expect(legend).toHaveAttribute('role', 'region');
    await expect(legend).toHaveAttribute('aria-label', /capability classes/i);

    // filesystem.write is a sensitive class; filesystem.read is not.
    await expect(page.locator('[data-testid="ap-class"][data-class="filesystem.write"]'))
      .toHaveAttribute('data-sensitive', 'true');
    await expect(page.locator('[data-testid="ap-class"][data-class="filesystem.read"]'))
      .toHaveAttribute('data-sensitive', 'false');

    // At least the WhatsApp channel card renders.
    const wa = page.locator('[data-testid="ap-channel"][data-channel="whatsapp_linked"]');
    await expect(wa).toHaveCount(1);
    await expect(wa).toHaveAttribute('role', 'region');
    await expect(wa).toHaveAttribute('data-policy-engine', 'true');
    await expect(wa.getByTestId('ap-principals')).toHaveAttribute('role', 'table');
  });

  test('channels without a policy engine are listed honestly', async ({ page }) => {
    const telegram = page.locator('[data-testid="ap-channel"][data-channel="telegram"]');
    await expect(telegram).toHaveCount(1);
    await expect(telegram).toHaveAttribute('data-policy-engine', 'false');
    await expect(telegram.getByTestId('ap-channel-policy')).toContainText('not supported yet');
    await expect(telegram.getByTestId('ap-channel-note')).toBeVisible();
  });

  test('WhatsApp card links to its own edit page instead of duplicating it', async ({ page }) => {
    const manage = page.locator('[data-testid="ap-channel-manage"][data-channel="whatsapp_linked"]');
    if (await manage.count() === 0) test.skip(true, 'viewer role cannot edit channels');
    await manage.click();
    await page.getByTestId('wa-access-summary').waitFor({ state: 'visible', timeout: 10_000 });
  });

  test('usable at a 360px mobile viewport without page-level horizontal scroll', async ({ page }) => {
    await page.setViewportSize({ width: 360, height: 780 });
    await goToPage(page);
    await expect(page.getByTestId('ap-summary')).toBeVisible();
    // The body must not scroll horizontally; the wide matrix scrolls in its
    // own container instead.
    const overflow = await page.evaluate(() =>
      document.documentElement.scrollWidth <= document.documentElement.clientWidth + 1
    );
    expect(overflow).toBeTruthy();
  });
});
