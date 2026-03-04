import { defineConfig, devices } from '@playwright/test';

export default defineConfig({
  testDir: './playwright',
  fullyParallel: false,
  retries: process.env.CI ? 1 : 0,
  reporter: process.env.CI ? 'github' : 'list',
  use: {
    baseURL: process.env.GARRAIA_BASE_URL ?? 'http://localhost:3888',
    trace: 'on-first-retry',
  },
  projects: [
    {
      name: 'chromium',
      use: { ...devices['Desktop Chrome'] },
    },
  ],
  // Start the gateway before tests when running locally
  // webServer is optional — skip if GARRAIA_SKIP_SERVER=1
  ...(process.env.GARRAIA_SKIP_SERVER
    ? {}
    : {
        webServer: {
          command: 'cargo run -p garraia-gateway',
          url: 'http://localhost:3888/health',
          reuseExistingServer: true,
          timeout: 60_000,
          cwd: '..',
          env: {
            ANTHROPIC_API_KEY: process.env.ANTHROPIC_API_KEY ?? 'sk-ant-test',
          },
        },
      }),
});
