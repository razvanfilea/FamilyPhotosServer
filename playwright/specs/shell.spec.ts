import type { Page } from '@playwright/test';
import { expect, gotoReady, loginAs, test } from '../fixtures/auth';
import { folderId } from '../harness/seed';

interface Destination {
    path: string;
    heading: string;
    title?: string;
    /** The rail/sidebar item representing the page */
    nav: string;
    /** The primary tab (bottom nav) the page lives under, if any */
    tab?: string;
}

function destinations(): Destination[] {
    return [
        { path: '/', heading: 'Photos', nav: 'photos', tab: 'photos' },
        { path: '/folders', heading: 'Folders', nav: 'folders', tab: 'folders' },
        { path: `/folders/${folderId('Holiday')}`, heading: 'Holiday', nav: 'folders', tab: 'folders' },
        { path: '/utilities', heading: 'Utilities', nav: 'utilities', tab: 'utilities' },
        { path: '/favorites', heading: 'Favorites', nav: 'favorites', tab: 'utilities' },
        { path: '/trash', heading: 'Trash', nav: 'trash', tab: 'utilities' },
        { path: '/duplicates', heading: 'Duplicates', nav: 'utilities', tab: 'utilities' },
        { path: '/large-files', heading: 'Large Files', nav: 'utilities', tab: 'utilities' },
        { path: '/settings', heading: 'Settings', nav: 'utilities', tab: 'utilities' },
        { path: '/upload', heading: 'Upload Photos', title: 'Upload', nav: 'upload' },
    ];
}

const NOT_FOUND_PATH = '/does-not-exist';

/** Collects requests to other origins and console errors (CSP violations are logged as errors). */
function watchPage(page: Page, baseURL: string) {
    const external: string[] = [];
    const problems: string[] = [];
    page.on('request', (request) => {
        const url = request.url();
        if (!url.startsWith(`${baseURL}/`) && !url.startsWith('data:') && !url.startsWith('blob:')) {
            external.push(url);
        }
    });
    page.on('console', (message) => {
        // Chrome logs the 404 status of the not-found page itself
        const expected404 = message.text().startsWith('Failed to load resource')
            && message.location().url.endsWith(NOT_FOUND_PATH);
        if (message.type() === 'error' && !expected404) problems.push(message.text());
    });
    page.on('pageerror', (error) => problems.push(error.message));
    return { external, problems };
}

const currentItems = (page: Page) => page.locator('[aria-current="page"]:visible');

async function expectPage(page: Page, destination: Destination) {
    const response = await gotoReady(page, destination.path);
    expect(response?.status()).toBe(200);
    await expect(page.getByTestId('topbar').getByRole('heading', { level: 1 })).toHaveText(destination.heading);
    await expect(page).toHaveTitle(`${destination.title ?? destination.heading} - Family Photos`);
}

test.describe('navigation shell', () => {
    test('the bottom nav (375 px), rail (900 px) and sidebar (1440 px) highlight the destination', async ({ alicePage: page, isMobile }) => {
        test.skip(isMobile, 'Viewport widths are set explicitly on the desktop project');
        const rail = page.getByTestId('nav-rail');
        for (const destination of destinations()) {
            await expectPage(page, destination);

            await page.setViewportSize({ width: 375, height: 800 });
            await expect(page.getByTestId('bottom-nav')).toBeVisible();
            await expect(rail).toBeHidden();
            await expect(page.getByTestId('topbar-upload')).toBeVisible();
            if (destination.tab) {
                await expect(currentItems(page)).toHaveCount(1);
                await expect(currentItems(page)).toHaveAttribute('data-testid', `bottom-nav-${destination.tab}`);
            } else {
                await expect(currentItems(page)).toHaveCount(0);
            }

            await page.setViewportSize({ width: 900, height: 800 });
            await expect(rail).toBeVisible();
            await expect(page.getByTestId('bottom-nav')).toBeHidden();
            await expect(page.getByTestId('topbar-upload')).toBeHidden();
            await expect(rail.getByTestId('nav-favorites')).toBeHidden();
            if (destination.nav === 'favorites' || destination.nav === 'trash') {
                // Favorites/Trash have no rail item: Utilities is shown as their parent
                await expect(currentItems(page)).toHaveCount(0);
                await expect(rail.getByTestId('nav-utilities')).toHaveClass(/fp-nav-item--parent/);
            } else {
                await expect(currentItems(page)).toHaveCount(1);
                await expect(currentItems(page)).toHaveAttribute('data-testid', `nav-${destination.nav}`);
            }

            await page.setViewportSize({ width: 1440, height: 900 });
            await expect(rail.getByText('Family Photos')).toBeVisible();
            await expect(rail.getByTestId('nav-favorites')).toBeVisible();
            await expect(rail.getByTestId('nav-trash')).toBeVisible();
            await expect(currentItems(page)).toHaveCount(1);
            await expect(currentItems(page)).toHaveAttribute('data-testid', `nav-${destination.nav}`);
        }
    });

    test('the bottom navigation works on phones', async ({ alicePage: page, isMobile }) => {
        test.skip(!isMobile, 'Phone layout');
        await page.goto('/');
        await page.getByTestId('bottom-nav-folders').click();
        await expect(page).toHaveURL('/folders');
        await expect(page.getByTestId('bottom-nav-folders')).toHaveAttribute('aria-current', 'page');
        await page.getByTestId('topbar-upload').click();
        await expect(page).toHaveURL('/upload');
    });

    test('every page is same-origin only and has no CSP violations', async ({ page, baseURL }) => {
        const { external, problems } = watchPage(page, baseURL!);

        // Logged out
        const login = await gotoReady(page, '/login');
        expect(login?.headers()['content-security-policy']).toContain("script-src 'self'");
        await page.getByTestId('login-password-toggle').click();
        await expect(page.getByTestId('login-password')).toHaveAttribute('type', 'text');
        await gotoReady(page, NOT_FOUND_PATH);
        await expect(page.getByTestId('not-found')).toBeVisible();

        await loginAs(page, 'alice');
        for (const destination of destinations()) {
            const response = await gotoReady(page, destination.path);
            expect(response?.headers()['content-security-policy']).toContain("script-src 'self'");
            await expect(page.getByTestId('topbar')).toBeVisible();
            // The app module booted (the user menu component is mounted)
            await page.getByTestId('user-menu-button').click();
            await expect(page.getByTestId('user-menu-items')).toBeVisible();
            await page.keyboard.press('Escape');
            await expect(page.getByTestId('user-menu-items')).toBeHidden();
        }
        await gotoReady(page, NOT_FOUND_PATH);
        await expect(page.getByTestId('not-found')).toBeVisible();

        expect(external).toEqual([]);
        expect(problems).toEqual([]);
    });
});

test.describe('dialogs and toasts', () => {
    test('signing out asks for confirmation', async ({ page }) => {
        await loginAs(page, 'alice');
        await gotoReady(page, '/utilities');

        await page.getByTestId('user-menu-button').click();
        await page.getByTestId('user-menu-sign-out').click();
        const dialog = page.getByTestId('app-dialog');
        await expect(dialog).toBeVisible();
        await expect(dialog.getByRole('heading')).toHaveText('Sign out?');

        await dialog.getByTestId('confirm-cancel').click();
        await expect(dialog).toBeHidden();
        await expect(page).toHaveURL('/utilities');

        await page.getByTestId('user-menu-button').click();
        await page.getByTestId('user-menu-sign-out').click();
        await dialog.getByTestId('confirm-accept').click();
        await expect(page).toHaveURL('/login');

        await page.goto('/');
        await expect(page).toHaveURL('/login?next=%2F');
    });

    test('a running confirm action can\'t be dismissed', async ({ page }) => {
        await loginAs(page, 'alice');
        await gotoReady(page, '/utilities');

        let release!: () => void;
        const released = new Promise<void>((resolve) => { release = resolve; });
        await page.route('**/api/logout', async (route) => {
            await released;
            await route.continue();
        });

        await page.getByTestId('user-menu-button').click();
        await page.getByTestId('user-menu-sign-out').click();
        const dialog = page.getByTestId('app-dialog');
        await dialog.getByTestId('confirm-accept').click();
        await expect(dialog.getByTestId('confirm-accept')).toBeDisabled();

        await page.keyboard.press('Escape');
        await page.mouse.click(5, 5);
        await expect(dialog).toBeVisible();

        release();
        await expect(page).toHaveURL('/login');
    });

    test('Escape closes the dialog', async ({ alicePage: page }) => {
        await gotoReady(page, '/');
        await page.getByTestId('user-menu-button').click();
        await page.getByTestId('user-menu-shortcuts').click();
        await expect(page.getByTestId('shortcuts-dialog')).toBeVisible();
        await page.keyboard.press('Escape');
        await expect(page.getByTestId('app-dialog')).toBeHidden();
    });

    test('? opens the shortcuts dialog, except while typing', async ({ alicePage: page }) => {
        await gotoReady(page, '/');
        await page.keyboard.press('?');
        await expect(page.getByTestId('shortcuts-dialog')).toBeVisible();

        await page.getByTestId('shortcuts-close').click();
        await expect(page.getByTestId('app-dialog')).toBeHidden();

        // Typing in a text field doesn't trigger shortcuts
        await page.evaluate(() => {
            const input = document.createElement('input');
            input.dataset.testid = 'scratch-input';
            document.getElementById('main')!.prepend(input);
        });
        await page.getByTestId('scratch-input').pressSequentially('a?');
        await expect(page.getByTestId('scratch-input')).toHaveValue('a?');
        await expect(page.getByTestId('app-dialog')).toBeHidden();
    });

    test('server errors show a generic toast', async ({ alicePage: page }) => {
        await gotoReady(page, '/');
        await page.route('**/ui/dialogs/shortcuts', (route) =>
            route.fulfill({ status: 500, body: 'Database error: secret details' }));

        await page.getByTestId('user-menu-button').click();
        await page.getByTestId('user-menu-shortcuts').click();
        const toast = page.getByTestId('toast');
        await expect(toast).toHaveText(/Something went wrong/);
        await expect(toast).toHaveAttribute('data-type', 'error');
        await expect(page.getByTestId('app-dialog')).toBeHidden();

        await toast.getByRole('button', { name: 'Dismiss' }).click();
        await expect(toast).toHaveCount(0);
    });

    test('bad request errors show the server message', async ({ alicePage: page }) => {
        await gotoReady(page, '/');
        await page.route('**/ui/dialogs/shortcuts', (route) =>
            route.fulfill({ status: 400, body: 'Folder name is too long' }));

        await page.getByTestId('user-menu-button').click();
        await page.getByTestId('user-menu-shortcuts').click();
        await expect(page.getByTestId('toast')).toHaveText(/Folder name is too long/);
    });

    test('network errors show a connection toast', async ({ alicePage: page }) => {
        await gotoReady(page, '/');
        await page.route('**/ui/dialogs/shortcuts', (route) => route.abort('connectionrefused'));

        await page.getByTestId('user-menu-button').click();
        await page.getByTestId('user-menu-shortcuts').click();
        await expect(page.getByTestId('toast')).toHaveText(/Failed to connect to server/);
    });

    test('toasts requested with fp:toast (HX-Trigger) are shown and auto-dismissed', async ({ alicePage: page }) => {
        await gotoReady(page, '/');
        await page.evaluate(() => {
            document.body.dispatchEvent(new CustomEvent('fp:toast', {
                bubbles: true,
                detail: { message: 'Saved', type: 'success' },
            }));
        });
        const toast = page.getByTestId('toast');
        await expect(toast).toHaveText(/Saved/);
        await expect(toast).toHaveAttribute('data-type', 'success');
        await expect(toast).toHaveCount(0, { timeout: 6000 });
    });
});
