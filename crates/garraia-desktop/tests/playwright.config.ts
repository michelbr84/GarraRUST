import { defineConfig, devices } from '@playwright/test';

// Teste da UI do Assistente de Primeiro Uso (#1636, fatia 3) SEM Tauri e SEM
// gateway: a spec abre `ui/onboarding.html` por file:// com um `window.__TAURI__`
// falso e dirige a tela emitindo os eventos NDJSON gravados (fatia 1). Config
// proprio — separado do `tests/playwright.config.ts` do Web Console, que sobe o
// gateway; aqui nada de servidor, nada de globalSetup.
export default defineConfig({
  testDir: '.',
  fullyParallel: true,
  retries: process.env.CI ? 1 : 0,
  reporter: process.env.CI ? 'github' : 'list',
  use: {
    trace: 'on-first-retry',
  },
  projects: [
    {
      name: 'chromium',
      use: { ...devices['Desktop Chrome'] },
    },
  ],
});
