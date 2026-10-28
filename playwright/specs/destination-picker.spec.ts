import type { Page } from '@playwright/test';
import { expect, gotoReady } from '../fixtures/auth';
import { test } from '../fixtures/folders';
import { folderId } from '../harness/seed';

// The picker is opened inside the move dialog, from a page that has no filter chips of its own
const PAGE = '/utilities';

async function openMoveDialog(page: Page, query = 'count=2') {
    await gotoReady(page, PAGE);
    await page.evaluate(
        (q) => (window as any).htmx.ajax('GET', `/ui/dialogs/move?${q}`, { target: '#app-dialog-content', swap: 'innerHTML' }),
        query,
    );
    await expect(page.getByTestId('move-dialog')).toBeVisible();
}

const picker = (page: Page) => page.getByTestId('destination-picker');
const value = (page: Page) => picker(page).evaluate((el: any) => el.fpPicker.getValue());
const folderCard = (page: Page, name: string) =>
    picker(page).getByTestId('folder-card').filter({ has: page.getByTestId('folder-card-name').getByText(name, { exact: true }) });

test.describe('destination picker', () => {
    test('the move dialog has a title, a showcase slot and a confirm button', async ({ alicePage: page }) => {
        await openMoveDialog(page, 'count=3');
        await expect(page.getByTestId('move-title')).toHaveText('Move 3 photos');
        await expect(page.getByTestId('move-confirm')).toHaveText('Move photos');
        await expect(page.locator('[data-slot="showcase"]')).toHaveCount(1);
        await page.getByTestId('move-cancel').click();
        await expect(page.getByTestId('app-dialog')).toBeHidden();
    });

    test('defaults to the personal gallery', async ({ alicePage: page }) => {
        await openMoveDialog(page);
        await expect(page.getByTestId('dest-personal')).toHaveAttribute('aria-pressed', 'true');
        await expect(page.getByTestId('dest-family')).toHaveAttribute('aria-pressed', 'false');
        await expect(page.getByTestId('dest-new')).toBeVisible();
        await expect(page.getByTestId('move-confirm')).toBeEnabled();
        expect(await value(page)).toEqual({ kind: 'root', isPublic: false });
    });

    test('the filter chips preselect the gallery', async ({ alicePage: page }) => {
        await openMoveDialog(page);

        await picker(page).getByTestId('filter-chip-family').click();
        await expect(page.getByTestId('dest-family')).toHaveAttribute('aria-pressed', 'true');
        await expect(page.getByTestId('dest-personal')).toBeHidden();
        expect(await value(page)).toEqual({ kind: 'root', isPublic: true });

        await picker(page).getByTestId('filter-chip-personal').click();
        await expect(page.getByTestId('dest-personal')).toHaveAttribute('aria-pressed', 'true');
        await expect(page.getByTestId('dest-family')).toBeHidden();
        expect(await value(page)).toEqual({ kind: 'root', isPublic: false });

        // Shared: nothing is chosen until a folder is, and folders can't be created
        await picker(page).getByTestId('filter-chip-shared').click();
        expect(await value(page)).toBeNull();
        await expect(page.getByTestId('move-confirm')).toBeDisabled();
        await expect(page.getByTestId('dest-personal')).toBeHidden();
        await expect(page.getByTestId('dest-family')).toBeHidden();
        await expect(page.getByTestId('dest-new')).toBeHidden();

        // All keeps the current choice, and shows everything
        await picker(page).getByTestId('filter-chip-all').click();
        expect(await value(page)).toBeNull();
        await expect(page.getByTestId('dest-personal')).toBeVisible();
        await expect(page.getByTestId('dest-family')).toBeVisible();
        await expect(page.getByTestId('dest-new')).toBeVisible();
        await page.getByTestId('dest-family').click();
        await expect(page.getByTestId('move-confirm')).toBeEnabled();
        await picker(page).getByTestId('filter-chip-all').click();
        expect(await value(page)).toEqual({ kind: 'root', isPublic: true });
    });

    test('the chip of the saved filter decides the first choice', async ({ alicePage: page }) => {
        await page.context().addCookies([{ name: 'fp_prefs', value: 'filter=family', domain: 'localhost', path: '/' }]);
        await openMoveDialog(page);
        await expect(picker(page).getByTestId('filter-chip-family')).toHaveAttribute('aria-pressed', 'true');
        expect(await value(page)).toEqual({ kind: 'root', isPublic: true });
    });

    test('lists the folders alice can upload to', async ({ alicePage: page }) => {
        await openMoveDialog(page);
        for (const name of ['Holiday', 'Private', 'Family Trip']) {
            await expect(folderCard(page, name)).toBeVisible();
        }
        // Buttons, not links
        await expect(folderCard(page, 'Holiday')).toHaveJSProperty('tagName', 'BUTTON');
    });

    test('only lists folders bob can upload to', async ({ bobPage: page }) => {
        await openMoveDialog(page);
        // Holiday is shared with bob as view only
        await expect(folderCard(page, 'Family Trip')).toBeVisible();
        await expect(folderCard(page, 'Holiday')).toHaveCount(0);
        await expect(folderCard(page, 'Private')).toHaveCount(0);

        await picker(page).getByTestId('filter-chip-shared').click();
        await expect(picker(page).getByTestId('empty-state')).toContainText('No folders to choose');
    });

    test('choosing a folder collapses the list into a selected row', async ({ alicePage: page }) => {
        await openMoveDialog(page);
        await folderCard(page, 'Holiday').click();

        await expect(page.getByTestId('dest-selected')).toBeVisible();
        await expect(page.getByTestId('dest-selected-name')).toHaveText('Holiday');
        await expect(page.getByTestId('dest-selected-type')).toHaveText('Personal');
        await expect(picker(page).getByTestId('folder-list')).toBeHidden();
        await expect(folderCard(page, 'Holiday')).toBeHidden();
        await expect(page.getByTestId('dest-new')).toBeHidden();
        await expect(page.getByTestId('dest-personal')).toHaveAttribute('aria-pressed', 'false');
        expect(await value(page)).toMatchObject({ kind: 'folder', folderId: folderId('Holiday'), name: 'Holiday', isPublic: false });

        await page.getByTestId('dest-clear').click();
        await expect(page.getByTestId('dest-selected')).toBeHidden();
        await expect(folderCard(page, 'Holiday')).toBeVisible();
        await expect(page.getByTestId('dest-personal')).toHaveAttribute('aria-pressed', 'true');

        await folderCard(page, 'Family Trip').click();
        await expect(page.getByTestId('dest-selected-type')).toHaveText('Family');
        expect(await value(page)).toMatchObject({ kind: 'folder', isPublic: true });
    });

    test('a folder can be preselected', async ({ alicePage: page }) => {
        await openMoveDialog(page, `count=1&folder_id=${folderId('Private')}`);
        await expect(page.getByTestId('dest-selected-name')).toHaveText('Private');
        expect(await value(page)).toMatchObject({ kind: 'folder', folderId: folderId('Private') });
    });

    test('a folder without upload permission is not preselected', async ({ bobPage: page }) => {
        await openMoveDialog(page, `count=1&folder_id=${folderId('Holiday')}`);
        expect(await value(page)).toEqual({ kind: 'root', isPublic: false });
    });

    test('create new folder returns a new destination without creating it', async ({ alicePage: page, folders }) => {
        const name = folders.name('Planned');
        await openMoveDialog(page);
        await page.getByTestId('dest-new').click();

        // The dialog opens on top of the move dialog, which stays
        const nameDialog = page.getByTestId('folder-name-dialog');
        await expect(nameDialog).toBeVisible();
        await expect(page.getByTestId('move-dialog')).toBeAttached();
        await page.getByTestId('folder-name-input').fill(name);
        await page.getByTestId('folder-visibility-family').check();
        await page.getByTestId('folder-name-confirm').click();
        await expect(nameDialog).toHaveCount(0);

        await expect(page.getByTestId('dest-selected-name')).toHaveText(name);
        await expect(page.getByTestId('dest-selected-type')).toHaveText('Family');
        await expect(page.getByTestId('move-confirm')).toBeEnabled();
        expect(await value(page)).toEqual({ kind: 'new', name, isPublic: true });

        // Nothing exists on the server yet
        const alice = await folders.as('alice');
        const sync = await (await alice.post('/api/sync/folders', { data: {} })).json();
        expect(sync.map((f: { name: string }) => f.name)).not.toContain(name);
    });

    test('dismissing the new folder dialog keeps the choice', async ({ alicePage: page }) => {
        await openMoveDialog(page);
        await page.getByTestId('dest-new').click();
        await expect(page.getByTestId('folder-name-dialog')).toBeVisible();
        await page.keyboard.press('Escape');
        await expect(page.getByTestId('folder-name-dialog')).toHaveCount(0);
        await expect(page.getByTestId('move-dialog')).toBeVisible();
        expect(await value(page)).toEqual({ kind: 'root', isPublic: false });
    });

    test('resolveDestination creates the folder of a new destination', async ({ alicePage: page, folders }) => {
        const name = folders.name('Resolved');
        await gotoReady(page, PAGE);
        const resolved = await page.evaluate(async (folderName) => {
            const { resolveDestination } = await import('/assets/js/destination-picker/destination.js');
            return [
                await resolveDestination({ kind: 'root', isPublic: true }),
                await resolveDestination({ kind: 'folder', folderId: 7 }),
                await resolveDestination({ kind: 'new', name: folderName, isPublic: false }),
            ];
        }, name);
        expect(resolved[0]).toEqual({ makePublic: true });
        expect(resolved[1]).toEqual({ folderId: 7 });
        const created = resolved[2].folderId;
        expect(created).toBeGreaterThan(0);

        // Same name again: the server returns the same folder
        const again = await page.evaluate(async (folderName) => {
            const { resolveDestination } = await import('/assets/js/destination-picker/destination.js');
            return resolveDestination({ kind: 'new', name: folderName, isPublic: false });
        }, name);
        expect(again.folderId).toBe(created);

        const alice = await folders.as('alice');
        expect((await alice.delete(`/api/folders/${created}`)).status()).toBe(204);
    });

    test('the picker can be loaded on its own for the upload page', async ({ alicePage: page }) => {
        await gotoReady(page, PAGE);
        await page.evaluate(() => {
            document.body.insertAdjacentHTML('beforeend', '<div id="host"></div>');
            return (window as any).htmx.ajax('GET', '/ui/destination-picker?mode=upload', { target: '#host', swap: 'innerHTML' });
        });
        await expect(page.locator('#host').getByTestId('destination-picker')).toHaveAttribute('data-mode', 'upload');
        await expect.poll(() => value(page).catch(() => undefined)).toEqual({ kind: 'root', isPublic: false });
    });
});
