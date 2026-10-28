import { expect, gotoReady } from '../fixtures/auth';
import { test } from '../fixtures/folders';
import { folderId } from '../harness/seed';

const dialog = (page: import('@playwright/test').Page) => page.getByTestId('sharing-dialog');
const member = (page: import('@playwright/test').Page, name: string) =>
    page.getByTestId('sharing-member').filter({ hasText: name });

test.describe('sharing', () => {
    test('add a member, change the permission, remove them', async ({ alicePage: page, bobPage, folders }) => {
        const name = folders.name('Shared by e2e');
        const id = await folders.create('alice', name);
        await gotoReady(page, `/folders/${id}`);
        await expect(page.getByTestId('folder-badge')).toContainText('Personal');

        // Add bob: view only
        await page.getByTestId('folder-share').click();
        await expect(dialog(page)).toContainText('Manage access');
        await expect(page.getByTestId('sharing-owner')).toContainText('Alice');
        await expect(page.getByTestId('sharing-owner')).toContainText('Owner');
        await expect(page.getByTestId('sharing-add-user')).toContainText(['Bob', 'Carol']);
        await page.getByTestId('sharing-add').click();
        await page.getByTestId('sharing-add-user').filter({ hasText: 'Bob' }).click();
        await expect(page.getByTestId('toast')).toHaveText(/Member added/);
        await expect(member(page, 'Bob').getByTestId('sharing-permission')).toHaveText('View only');
        await expect(page.getByTestId('sharing-add-user')).toHaveCount(1);
        await expect(page.getByTestId('folder-badge')).toContainText('Shared (1)');

        // He sees it under Shared, without upload
        await gotoReady(bobPage, '/folders?filter=shared');
        await expect(bobPage.getByTestId('folder-card').filter({ hasText: name })).toHaveAttribute('data-type', 'shared');
        await bobPage.getByTestId('folder-card').filter({ hasText: name }).click();
        await expect(bobPage.getByTestId('folder-title')).toHaveText(name);
        await expect(bobPage.getByTestId('folder-upload')).toHaveCount(0);
        await expect(bobPage.getByTestId('folder-page')).toHaveAttribute('data-can-delete', 'false');

        // Can upload & delete
        await member(page, 'Bob').getByTestId('sharing-member-menu').click();
        await expect(member(page, 'Bob').getByTestId('sharing-permission-view')).toHaveAttribute('aria-checked', 'true');
        await member(page, 'Bob').getByTestId('sharing-permission-upload-delete').click();
        await expect(page.getByTestId('toast').last()).toHaveText(/Permissions updated/);
        await expect(member(page, 'Bob').getByTestId('sharing-permission')).toHaveText('Can upload & delete');
        // The menu stays open for the next change
        await expect(member(page, 'Bob').getByTestId('sharing-permission-upload-delete')).toHaveAttribute('aria-checked', 'true');

        await bobPage.reload();
        await expect(bobPage.getByTestId('folder-upload')).toBeVisible();
        await expect(bobPage.getByTestId('folder-page')).toHaveAttribute('data-can-delete', 'true');

        // Can upload only
        await member(page, 'Bob').getByTestId('sharing-permission-upload').click();
        await expect(member(page, 'Bob').getByTestId('sharing-permission')).toHaveText('Can upload');

        // Remove him: asks first
        await member(page, 'Bob').getByTestId('sharing-remove').click();
        await expect(member(page, 'Bob')).toContainText('Remove Bob');
        await member(page, 'Bob').getByTestId('sharing-remove-confirm').click();
        await expect(page.getByTestId('toast').last()).toHaveText(/Member removed/);
        await expect(page.getByTestId('sharing-member')).toHaveCount(0);
        await expect(page.getByTestId('folder-badge')).toContainText('Personal');

        const response = await bobPage.goto(`/folders/${id}`);
        expect(response?.status()).toBe(404);
    });

    test('expiry presets and custom dates', async ({ alicePage: page, folders }) => {
        const id = await folders.create('alice', folders.name('Expiring'));
        await gotoReady(page, `/folders/${id}`);
        await page.getByTestId('folder-share').click();
        await page.getByTestId('sharing-add').click();
        await page.getByTestId('sharing-add-expiry').selectOption('week');
        await page.getByTestId('sharing-add-user').filter({ hasText: 'Bob' }).click();
        await expect(page.getByTestId('toast')).toHaveText(/Member added/);
        await expect(member(page, 'Bob').getByTestId('sharing-expiry-label')).toHaveText(/^Expires \d{1,2} \w{3} \d{4}$/);
        const week = await member(page, 'Bob').getByTestId('sharing-expiry-label').textContent();

        // Another preset
        await member(page, 'Bob').getByTestId('sharing-member-menu').click();
        await member(page, 'Bob').getByTestId('sharing-expiry').selectOption('month');
        await expect(member(page, 'Bob').getByTestId('sharing-expiry-label')).not.toHaveText(week!);

        // A date in the past is refused
        await member(page, 'Bob').getByTestId('sharing-expiry').selectOption('custom');
        await member(page, 'Bob').getByTestId('sharing-expiry-date').fill('2020-01-01');
        await member(page, 'Bob').getByTestId('sharing-expiry-set').click();
        await expect(page.getByTestId('toast').last()).toHaveText('Expiry must be in the future');
        await expect(member(page, 'Bob').getByTestId('sharing-expiry-label')).not.toHaveText(week!);

        // A date in the future is taken as the end of that day
        await member(page, 'Bob').getByTestId('sharing-expiry-date').fill('2099-12-31');
        await member(page, 'Bob').getByTestId('sharing-expiry-set').click();
        await expect(member(page, 'Bob').getByTestId('sharing-expiry-label')).toHaveText('Expires 31 Dec 2099');

        // Back to no expiry
        await member(page, 'Bob').getByTestId('sharing-expiry').selectOption('none');
        await expect(member(page, 'Bob').getByTestId('sharing-expiry-label')).toHaveCount(0);
        await expect(member(page, 'Bob').getByTestId('sharing-permission')).toHaveText('View only');
    });

    test('a custom expiry can be chosen when adding', async ({ alicePage: page, folders }) => {
        const id = await folders.create('alice', folders.name('Custom add'));
        await gotoReady(page, `/folders/${id}`);
        await page.getByTestId('folder-share').click();
        await page.getByTestId('sharing-add').click();
        await page.getByTestId('sharing-add-expiry').selectOption('custom');
        await page.getByTestId('sharing-add-user').filter({ hasText: 'Carol' }).click();
        await expect(page.getByTestId('toast')).toHaveText('Pick an expiry date');
        await expect(page.getByTestId('sharing-member')).toHaveCount(0);

        await page.getByTestId('sharing-add-expiry-date').fill('2099-01-15');
        await page.getByTestId('sharing-add-user').filter({ hasText: 'Carol' }).click();
        await expect(member(page, 'Carol').getByTestId('sharing-expiry-label')).toHaveText('Expires 15 Jan 2099');
    });

    test('a shared folder with nobody left to add hides the add section', async ({ alicePage: page, folders }) => {
        const id = await folders.create('alice', folders.name('Everyone'));
        const alice = await folders.as('alice');
        for (const grantee of ['bob', 'carol']) {
            expect((await alice.post('/api/sharing', { data: { folder_id: id, grantee_id: grantee } })).ok()).toBe(true);
        }
        await gotoReady(page, `/folders/${id}`);
        await page.getByTestId('folder-share').click();
        await expect(page.getByTestId('sharing-member')).toHaveCount(2);
        await expect(page.getByTestId('sharing-add')).toHaveCount(0);
        await expect(page.getByTestId('folder-badge')).toContainText('Shared (2)');
    });

    test('members see who has access, read only', async ({ bobPage: page }) => {
        await gotoReady(page, `/folders/${folderId('Holiday')}`);
        await page.getByTestId('folder-access').click();
        await expect(dialog(page)).toContainText('People with access');
        await expect(page.getByTestId('sharing-owner')).toContainText('Alice');
        await expect(member(page, 'Bob').getByTestId('sharing-permission')).toHaveText('View only');
        for (const testid of ['sharing-add', 'sharing-remove', 'sharing-member-menu']) {
            await expect(page.getByTestId(testid)).toHaveCount(0);
        }
        await page.getByTestId('sharing-close').click();
        await expect(page.getByTestId('app-dialog')).toBeHidden();
    });

    test('an expired share hides the folder from bob', async ({ alicePage: page, bobPage, folders }) => {
        // The API only accepts a future expiry, so let a share run out a few seconds from now
        const name = folders.name('Short lived');
        const id = await folders.create('alice', name);
        const alice = await folders.as('alice');
        const expiresAt = Math.floor(Date.now() / 1000) + 3;
        expect((await alice.post('/api/sharing', {
            data: { folder_id: id, grantee_id: 'bob', expires_at: expiresAt },
        })).ok()).toBe(true);
        expect((await bobPage.request.get(`/folders/${id}`)).status()).toBe(200);

        await expect.poll(async () => (await bobPage.request.get(`/folders/${id}`)).status(), {
            timeout: 10_000,
        }).toBe(404);
        await gotoReady(bobPage, '/folders');
        await expect(bobPage.getByTestId('folder-card').filter({ hasText: name })).toHaveCount(0);

        // The owner still sees bob (to renew or remove him), but he no longer counts
        await gotoReady(page, `/folders/${id}`);
        await expect(page.getByTestId('folder-badge')).toContainText('Personal');
        await page.getByTestId('folder-share').click();
        await expect(member(page, 'Bob').getByTestId('sharing-expiry-label')).toContainText('Expired');
        await expect(page.getByTestId('folder-badge')).toContainText('Personal');
    });

    test('an expiry in the past is rejected', async ({ alicePage: page, folders }) => {
        const id = await folders.create('alice', folders.name('Long lived'));
        const alice = await folders.as('alice');
        const inAYear = Math.floor(Date.now() / 1000) + 365 * 86400;
        expect((await alice.post('/api/sharing', {
            data: { folder_id: id, grantee_id: 'bob', expires_at: inAYear },
        })).ok()).toBe(true);

        const rejected = await alice.post('/api/sharing', {
            data: { folder_id: id, grantee_id: 'carol', expires_at: 1000 },
        });
        expect(rejected.status()).toBe(400);
        expect(await rejected.text()).toBe('Expiry must be in the future');
        await gotoReady(page, `/folders/${id}`);
        await expect(page.getByTestId('folder-badge')).toContainText('Shared (1)');
    });
});
