import { defineConfig, devices } from '@playwright/test';

// Browser performance spec (specs/perf.spec.ts) against a server you started on a large database:
// PERF=1 PERF_BASE_URL=http://localhost:8132 npm run test:perf. No harness server, no seeded data.
export default defineConfig({
    testDir: './specs',
    testMatch: /perf\.spec\.ts/,
    outputDir: '../node_modules/.cache/playwright/test-results-perf',
    reporter: 'list',
    workers: 1,
    timeout: 5 * 60 * 1000,
    use: { baseURL: process.env.PERF_BASE_URL, ...devices['Desktop Chrome'] },
});
