import { defineConfig, devices } from '@playwright/test';
import { mkdtempSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';

// Every run gets its own server (port FP_E2E_PORT, default 8081) with fresh storage and database
// in a unique temp directory, so it never touches a developer's server and several checkouts can
// run the suite at once. Workers inherit these variables from the runner process.
process.env.FP_E2E_PORT ??= '8081';
process.env.FP_E2E_RUN_DIR ??= mkdtempSync(join(tmpdir(), 'fp-e2e-'));

const port = process.env.FP_E2E_PORT;
const baseURL = `http://localhost:${port}`;
const unitSpecs = /unit-.*\.spec\.ts/;
// Firefox is a coordinator check before merges, not part of the default run: FP_E2E_FIREFOX=1 adds it
// (install it once with `PLAYWRIGHT_BROWSERS_PATH=0 npx playwright install firefox`).
const firefox = process.env.FP_E2E_FIREFOX
    ? [{ name: 'firefox', testIgnore: unitSpecs, use: { ...devices['Desktop Firefox'] } }]
    : [];

export default defineConfig({
    testDir: './specs',
    outputDir: '../node_modules/.cache/playwright/test-results',
    fullyParallel: false,
    forbidOnly: !!process.env.CI,
    retries: process.env.CI ? 1 : 0,
    reporter: process.env.CI ? [['list'], ['html', { outputFolder: './playwright-report', open: 'never' }]] : 'list',
    globalSetup: './harness/global-setup.ts',
    globalTeardown: './harness/global-teardown.ts',
    use: {
        baseURL,
        trace: 'on-first-retry',
    },
    projects: [
        { name: 'unit', testMatch: unitSpecs },
        { name: 'chromium', testIgnore: unitSpecs, use: { ...devices['Desktop Chrome'] } },
        { name: 'Mobile Chrome', testIgnore: unitSpecs, use: { ...devices['Pixel 7'] } },
        ...firefox,
    ],
    webServer: {
        command: 'node playwright/harness/serve.mjs',
        cwd: '..',
        url: `${baseURL}/login`,
        reuseExistingServer: false,
        timeout: 15 * 60 * 1000,
        stdout: 'pipe',
        stderr: 'pipe',
        gracefulShutdown: { signal: 'SIGTERM', timeout: 5000 },
    },
});
