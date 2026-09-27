/**
 * Seeds the admin account once, serially, before any test worker starts.
 *
 * On a cold gateway the admin UI shows the setup form (`needs_setup`), and
 * with two workers the first login of two different spec files races: both
 * submit the setup form, one wins, the loser is stuck on the setup view and
 * `#view-app` never becomes visible (seen on main at 2a6e9aed and on the
 * train at 16c19b2d). Doing the setup here, once, before `workers` exist
 * takes the race out of the picture — the helpers in the specs only ever
 * face the plain login form.
 */
import { request } from '@playwright/test';

const ADMIN_USER = process.env.GARRAIA_ADMIN_USER ?? 'admin';
const ADMIN_PASS = process.env.GARRAIA_ADMIN_PASS ?? 'admin123';
const BASE_URL = process.env.GARRAIA_BASE_URL ?? 'http://localhost:3888';

export default async function globalSetup(): Promise<void> {
  const ctx = await request.newContext({ baseURL: BASE_URL });
  try {
    const status = await ctx.get('/admin/api/setup/status');
    if (!status.ok()) {
      console.warn(`global-setup: setup/status returned ${status.status()}; skipping seed`);
      return;
    }
    if (!(await status.json())?.needs_setup) return; // already seeded
    const resp = await ctx.post('/admin/api/setup', {
      data: { username: ADMIN_USER, password: ADMIN_PASS },
    });
    if (!resp.ok()) {
      // A refusal here means someone seeded it between the probe and the
      // POST; login still works, so warn and move on.
      console.warn(`global-setup: seed returned ${resp.status()}; continuing`);
    }
  } finally {
    await ctx.dispose();
  }
}
