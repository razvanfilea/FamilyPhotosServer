import { defineConfig } from '@playwright/test';

// Unit tests of the pure JS helpers: no browser and no server needed.
export default defineConfig({
    testDir: './specs',
    testMatch: /unit-.*\.spec\.ts/,
    outputDir: '../node_modules/.cache/playwright/test-results-unit',
    reporter: 'list',
});
