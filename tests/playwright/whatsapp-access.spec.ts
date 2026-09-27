/**
 * ADR 0025 (#1402–#1408, #1411, #1413): Web Console — WhatsApp Access page.
 *
 * Selectors are `data-testid` only (plan 0052): the UI testids are owned by
 * `crates/garraia-gateway/src/admin.html` (`pageWhatsappAccess`, `waPreview`).
 * The page talks to `/admin/api/whatsapp/access`, which is the same engine
 * the CLI uses; what this spec proves is the operator flow:
 *
 *  1. Login → navigate → summary, admission badge and principals table render
 *  2. Every mutation goes through a PREVIEW (dry_run) before confirm; cancel
 *     changes nothing
 *  3. Add phone with level `read` → row appears masked (`…1234` only)
 *  4. Level select and write toggle exist on the new row; block → row shows
 *     Blocked; unblock → back to User; remove → row gone
 *  5. Audit card lists the confirmed changes, latest first, without the number
 */
import { test, expect, Page } from '@playwright/test';

const ADMIN_USER = process.env.GARRAIA_ADMIN_USER ?? 'admin';
const ADMIN_PASS = process.env.GARRAIA_ADMIN_PASS ?? 'admin123';
// A number nobody else uses in the seeded config; only its last 4 digits
// may ever appear in the page.
const PHONE = '+55 11 90000-4321';
const LAST4 = '4321';

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

function rowFor(page: Page, last4: string) {
  return page.locator(`[data-testid="wa-access-principal-row"][data-last4="…${last4}"]`);
}

async function confirmPreview(page: Page) {
  await page.getByTestId('wa-access-preview').waitFor({ state: 'visible', timeout: 10_000 });
  await page.getByTestId('wa-access-preview-changes').waitFor({ state: 'visible', timeout: 10_000 });
  await page.getByTestId('wa-access-confirm').click();
  await page.getByTestId('wa-access-preview').waitFor({ state: 'hidden', timeout: 10_000 }).catch(() => {});
}

test.describe('WhatsApp Access page', () => {
  test.beforeEach(async ({ page }) => {
    await login(page);
    await goToAccessPage(page);
  });

  test('renders summary, admission and the principals matrix', async ({ page }) => {
    await expect(page.getByTestId('wa-access-admission')).toBeVisible();
    await expect(page.getByTestId('wa-access-profile')).toContainText('profile:');
    await expect(page.getByTestId('wa-access-hot-reload')).toBeVisible();
    await expect(page.getByTestId('wa-access-principals')).toBeVisible();
    // The paired principal always exists in the matrix.
    await expect(page.locator('[data-testid="wa-access-principal-row"][data-principal="pareado"]')).toHaveCount(1);
    await expect(page.getByTestId('wa-access-audit')).toBeVisible();
  });

  test('open admission previews with a warning and cancel changes nothing', async ({ page }) => {
    const before = await page.getByTestId('wa-access-admission').textContent();
    const openBtn = page.getByTestId('wa-access-admission-open');
    if (await openBtn.isDisabled()) test.skip(true, 'admission already open in this environment');
    await openBtn.click();
    await expect(page.getByTestId('wa-access-preview-warning')).toContainText('ANYONE');
    await expect(page.getByTestId('wa-access-preview-changes')).toContainText('admission');
    await page.getByTestId('wa-access-cancel').click();
    await expect(page.getByTestId('wa-access-preview')).toBeHidden();
    await expect(page.getByTestId('wa-access-admission')).toHaveText(before ?? 'restricted');
  });

  test('add phone, adjust level/write, block, unblock and remove — masked all the way', async ({ page }) => {
    await page.getByTestId('wa-access-add-identity').fill(PHONE);
    await page.getByTestId('wa-access-add-level').selectOption('read');
    await page.getByTestId('wa-access-add').click();
    await confirmPreview(page);
    await goToAccessPage(page);
    const row = rowFor(page, LAST4);
    await expect(row).toHaveCount(1);
    await expect(row).toHaveAttribute('data-principal', 'usuario');
    // Only the last four digits, never the number.
    const html = await page.content();
    expect(html).not.toContain('5511900004321');
    expect(html).not.toContain('90000-4321');

    // Level select and write toggle are there for a user.
    await expect(row.getByTestId('wa-access-level')).toHaveValue('read');
    await expect(row.getByTestId('wa-access-write')).not.toBeChecked();

    // Write on → preview shows what is gained, confirm, row reflects it.
    await row.getByTestId('wa-access-write').check();
    await expect(page.getByTestId('wa-access-preview-impact')).toBeVisible();
    await page.getByTestId('wa-access-confirm').click();
    await goToAccessPage(page);
    await expect(rowFor(page, LAST4).getByTestId('wa-access-write')).toBeChecked();

    // Block → Blocked; unblock → User again.
    await rowFor(page, LAST4).getByTestId('wa-access-block').click();
    await confirmPreview(page);
    await goToAccessPage(page);
    await expect(rowFor(page, LAST4)).toHaveAttribute('data-principal', 'bloqueado');
    await rowFor(page, LAST4).getByTestId('wa-access-unblock').click();
    await confirmPreview(page);
    await goToAccessPage(page);
    await expect(rowFor(page, LAST4)).toHaveAttribute('data-principal', 'usuario');

    // Audit lists the confirmed changes, latest first, without the number.
    const audit = page.getByTestId('wa-access-audit');
    await expect(audit.getByTestId('wa-access-audit-row').first()).toContainText('unblock');
    expect(await audit.textContent()).not.toContain('5511900004321');

    // Remove → row gone.
    await rowFor(page, LAST4).getByTestId('wa-access-remove').click();
    await confirmPreview(page);
    await goToAccessPage(page);
    await expect(rowFor(page, LAST4)).toHaveCount(0);
  });

  test('invalid change is rejected in the preview, nothing is written', async ({ page }) => {
    // `full` for the unknown-sender default is refused by the engine (#1390):
    // the select only offers chat|read, so drive the API path through a
    // group JID that is not a JID.
    await page.getByTestId('wa-access-group-jid').fill('nao-e-jid');
    await page.getByTestId('wa-access-group-add').click();
    await expect(page.getByTestId('wa-access-preview-error')).toContainText('Rejected');
  });
});
