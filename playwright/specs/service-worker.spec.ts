import { expect, test, type Page } from '@playwright/test';
import { gotoReady, loginAs } from '../fixtures/auth';

// The only spec that lets the service worker run (the dev server it talks to caches no assets)
test.use({ serviceWorkers: 'allow' });

const previewIds = (page: Page) =>
    page.evaluate(async () => (await (await caches.open('fp-previews')).keys()).map((request) => new URL(request.url).pathname));

/** Signs in and opens the timeline with the worker in control, so its previews are stored. */
async function openTimeline(page: Page) {
    await loginAs(page, 'alice');
    await gotoReady(page, '/');
    // The first load installs the worker; it controls the pages after it
    await page.evaluate(() => navigator.serviceWorker.ready);
    await expect.poll(() => page.evaluate(() => navigator.serviceWorker.controller != null)).toBe(true);
    await page.reload();
    await expect(page.getByTestId('photo-tile').first()).toBeVisible();
    await expect.poll(async () => (await previewIds(page)).length).toBeGreaterThan(0);
}

test.describe('service worker', () => {
    test('stores previews and nothing else', async ({ page }) => {
        await openTimeline(page);

        const previews = await previewIds(page);
        expect(previews.every((path) => /^\/api\/preview\/\d+$/.test(path)), previews.join()).toBe(true);
        const stored = await page.evaluate(async () => {
            const result: Record<string, string[]> = {};
            for (const name of await caches.keys()) {
                result[name] = (await (await caches.open(name)).keys()).map((request) => new URL(request.url).pathname);
            }
            return result;
        });
        expect(Object.keys(stored).sort()).toEqual(['fp-previews', 'fp-shell-dev']);
        expect(stored['fp-shell-dev']).toEqual(['/assets/offline.html']);
    });

    test('serves stored previews and shows the offline page without a connection', async ({ page, context, browserName }) => {
        test.skip(browserName === 'firefox', "Firefox's offline emulation doesn't reach the service worker's own requests");
        await openTimeline(page);
        const [cached] = await previewIds(page);

        await context.setOffline(true);
        expect(await page.evaluate(async (path) => (await fetch(path)).status, cached)).toBe(200);
        // A preview nobody stored is a plain network failure
        expect(await page.evaluate(() => fetch('/api/preview/999999').then(() => 'ok', () => 'failed'))).toBe('failed');

        await page.goto('/folders');
        await expect(page.getByTestId('offline-page')).toBeVisible();
        await expect(page.getByRole('heading', { name: "You're offline" })).toBeVisible();

        await context.setOffline(false);
        await page.getByRole('link', { name: 'Try again' }).click();
        await expect(page).toHaveURL(/\/folders$/);
        await expect(page.getByTestId('folder-card').first()).toBeVisible();
    });

    test('forgets the previews of removed photos', async ({ page }) => {
        await openTimeline(page);
        const [gone, ...kept] = await previewIds(page);
        const id = Number(gone.split('/').pop());

        await page.evaluate((id) => {
            document.body.dispatchEvent(new CustomEvent('fp:photos-removed', { bubbles: true, detail: { ids: [id] } }));
        }, id);

        await expect.poll(() => previewIds(page)).not.toContain(gone);
        expect(await previewIds(page)).toEqual(expect.arrayContaining(kept.slice(0, 3)));
    });

    test('signing out empties the preview cache', async ({ page }) => {
        await openTimeline(page);

        await page.getByTestId('user-menu-button').click();
        await page.getByTestId('user-menu-sign-out').click();
        await page.getByTestId('app-dialog').getByTestId('confirm-accept').click();
        await expect(page).toHaveURL('/login');

        await expect.poll(() => previewIds(page)).toEqual([]);
    });

    test('the settings page clears the stored previews', async ({ page }) => {
        await openTimeline(page);

        await page.goto('/settings');
        await page.getByTestId('settings-clear-previews').click();

        await expect(page.getByTestId('toasts')).toContainText('Cached previews cleared');
        expect(await previewIds(page)).toEqual([]);
    });

    test('opening the login page after the session ended empties the preview cache', async ({ page }) => {
        await openTimeline(page);

        // The session ends elsewhere (it expired, or another tab signed out)
        expect((await page.request.post('/api/logout')).ok()).toBe(true);
        await page.goto('/login');

        await expect.poll(() => previewIds(page)).toEqual([]);
    });
});
