import type { Page } from '@playwright/test';
import { expect, gotoReady, loginAs, test, user } from '../fixtures/auth';
import { apiAs } from '../fixtures/library';

const toast = (page: Page, text: string) => page.getByTestId('toast').filter({ hasText: text });

/** The number of columns of the first month of the timeline */
const columnsOf = (page: Page) => page.locator('.month__grid').first().evaluate(
    (grid) => getComputedStyle(grid).gridTemplateColumns.split(' ').length);

const prefs = async (page: Page) =>
    new URLSearchParams(decodeURIComponent((await page.context().cookies()).find((c) => c.name === 'fp_prefs')?.value ?? ''));

test.describe('account and about', () => {
    test('shows the account and the version', async ({ alicePage: page }) => {
        await gotoReady(page, '/settings');
        await expect(page.getByTestId('settings-name')).toHaveText('Alice');
        await expect(page.getByTestId('settings-user-id')).toHaveText('alice');
        await expect(page.getByTestId('settings-version')).toHaveText(/^Family Photos server v\d+\.\d+\.\d+/);
        await expect(page.getByTestId('settings-asset-version')).toContainText('Assets');

        await page.getByTestId('settings-shortcuts').click();
        await expect(page.getByTestId('shortcuts-dialog')).toBeVisible();
    });

    test('sign out asks first, then returns to the login page', async ({ page }) => {
        await loginAs(page, 'alice');
        await gotoReady(page, '/settings');

        await page.getByTestId('settings-sign-out').click();
        await expect(page.getByTestId('app-dialog')).toContainText('Sign out?');
        await page.getByTestId('confirm-cancel').click();
        await expect(page).toHaveURL(/\/settings$/);

        await page.getByTestId('settings-sign-out').click();
        await page.getByTestId('confirm-accept').click();
        await expect(page).toHaveURL(/\/login/);
        await expect(page.getByTestId('login-form')).toBeVisible();
    });
});

test.describe('display', () => {
    test('the grid size changes the columns of the timeline', async ({ alicePage: page }) => {
        await gotoReady(page, '/settings');
        await expect(page.getByTestId('setting-zoom-1')).toBeChecked();

        const columns: number[] = [];
        for (const level of [0, 1, 2, 3]) {
            await page.getByTestId(`setting-zoom-${level}`).check({ force: true });
            await expect(toast(page, 'Saved').last()).toBeVisible();
            expect((await prefs(page)).get('zoom') ?? '1').toBe(String(level));

            await gotoReady(page, '/');
            await expect(page.getByTestId('photo-grid')).toHaveAttribute('data-zoom', String(level));
            await expect(page.getByTestId('photo-tile').first()).toBeVisible();
            columns.push(await columnsOf(page));
            await gotoReady(page, '/settings');
            await expect(page.getByTestId(`setting-zoom-${level}`)).toBeChecked();
        }
        // More photos per row with every step
        expect(columns).toEqual([...columns].sort((a, b) => a - b));
        expect(new Set(columns).size).toBe(4);
    });

    test('the default filter, folder sort and folder view are saved', async ({ alicePage: page }) => {
        await gotoReady(page, '/settings');

        await page.getByTestId('setting-filter').selectOption('family');
        await page.getByTestId('setting-fsort').selectOption('count_desc');
        await page.getByTestId('fview-list').check();
        await expect(toast(page, 'Saved').first()).toBeVisible();
        const saved = await prefs(page);
        expect(saved.get('filter')).toBe('family');
        expect(saved.get('fsort')).toBe('count_desc');
        expect(saved.get('fview')).toBe('list');

        await gotoReady(page, '/');
        await expect(page.getByTestId('filter-chip-family')).toHaveAttribute('aria-pressed', 'true');
        await gotoReady(page, '/folders');
        await expect(page.getByTestId('folder-list')).toHaveAttribute('data-view', 'list');

        await gotoReady(page, '/settings');
        await expect(page.getByTestId('setting-filter')).toHaveValue('family');
        await expect(page.getByTestId('setting-fsort')).toHaveValue('count_desc');
        await expect(page.getByTestId('fview-list')).toBeChecked();
    });
});

test.describe('change password', () => {
    // dave is used by no other spec: changing the password ends his other sessions for good
    test.describe.configure({ mode: 'serial' });
    test.skip(({ isMobile }) => isMobile, 'Changes dave\'s password');

    const original = user('dave').password;
    const changed = 'dave-new-password';
    async function fill(page: Page, current: string, next: string, confirmation = next) {
        await page.getByTestId('password-current').fill(current);
        await page.getByTestId('password-new').fill(next);
        await page.getByTestId('password-confirm').fill(confirmation);
        await page.getByTestId('password-submit').click();
    }

    const loginStatus = async (playwright: Parameters<typeof apiAs>[0], baseURL: string | undefined, password: string) => {
        const request = await playwright.request.newContext({ baseURL });
        const response = await request.post('/api/login', { form: { user_id: 'dave', password } });
        await request.dispose();
        return response.status();
    };

    test('validates the form before sending it', async ({ page }) => {
        await loginAs(page, 'dave');
        await gotoReady(page, '/settings');

        await fill(page, original, 'short');
        await expect(page.getByTestId('password-error')).toHaveText('Password must be at least 8 characters');
        // The typed values stay and the cursor goes to the field the error is about
        await expect(page.getByTestId('password-current')).toHaveValue(original);
        await expect(page.getByTestId('password-new')).toHaveValue('short');
        await expect(page.getByTestId('password-new')).toBeFocused();
        await fill(page, original, 'long-enough-1', 'long-enough-2');
        await expect(page.getByTestId('password-error')).toHaveText("The passwords don't match");
        await expect(page.getByTestId('password-confirm')).toBeFocused();
        await fill(page, 'not-the-password', 'long-enough-1');
        await expect(page.getByTestId('password-error')).toHaveText('Current password is incorrect');
        await expect(page.getByTestId('password-current')).toBeFocused();
        await page.getByTestId('password-new').fill('x');
        await expect(page.getByTestId('password-error')).toBeHidden();
    });

    test('changes the password, keeps this session and ends the others', async ({ page, playwright, baseURL }) => {
        await loginAs(page, 'dave');
        const otherSession = await apiAs(playwright, baseURL, 'dave');
        await gotoReady(page, '/settings');

        let changedPassword = false;
        try {
            await fill(page, original, changed);
            await expect(toast(page, 'Password changed')).toBeVisible();
            changedPassword = true;

            // This session still works, the other one doesn't
            expect((await page.request.get('/api/profile')).status()).toBe(200);
            expect((await otherSession.get('/api/profile')).status()).toBe(401);

            expect(await loginStatus(playwright, baseURL, original)).toBe(401);
            expect(await loginStatus(playwright, baseURL, changed)).toBe(200);
        } finally {
            // Restore the password for the other projects (Firefox) that run this spec
            if (changedPassword) {
                const restored = await page.request.post('/api/profile/password', {
                    data: { current_password: changed, new_password: original },
                });
                expect(restored.status()).toBe(204);
            }
            await otherSession.dispose();
        }
        expect(await loginStatus(playwright, baseURL, original)).toBe(200);
    });
});
