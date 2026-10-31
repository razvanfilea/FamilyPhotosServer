import type { Page } from '@playwright/test';
import { expect, gotoReady } from '../fixtures/auth';
import { test } from '../fixtures/folders';
import { folderId, getSeed } from '../harness/seed';

// Seeded folders: alice's "Holiday" (8 photos, shared with bob, newest photo 2024-07) and
// "Private" (3, 2024-01), and the public "Family Trip" (5, 2023-07).
const SEEDED = ['Family Trip', 'Holiday', 'Private'];

const cards = (page: Page) => page.getByTestId('folder-card');
const card = (page: Page, name: string) => cards(page).filter({ has: page.getByTestId('folder-card-name').getByText(name, { exact: true }) });

/** The seeded folders in the order the list shows them */
async function seededOrder(page: Page) {
    const names = await page.getByTestId('folder-card-name').allTextContents();
    return names.filter((name) => SEEDED.includes(name));
}

test.describe('folders tab', () => {
    test('lists the folders with counts and types', async ({ alicePage: page }) => {
        await gotoReady(page, '/folders');
        await expect(page.getByRole('heading', { level: 1 })).toHaveText('Folders');

        const holiday = card(page, 'Holiday');
        await expect(holiday).toHaveAttribute('data-type', 'personal');
        await expect(holiday.getByTestId('folder-card-meta')).toHaveText('8 photos');
        await expect(holiday).toHaveAttribute('href', `/folders/${folderId('Holiday')}`);
        await expect(card(page, 'Private').getByTestId('folder-card-meta')).toHaveText('3 photos');
        await expect(card(page, 'Family Trip')).toHaveAttribute('data-type', 'family');
        await expect(card(page, 'Family Trip').getByTestId('folder-card-meta')).toHaveText('5 photos');
        await expect(holiday.locator('img')).toHaveAttribute('src', /^\/api\/preview\/\d+$/);
    });

    test('bob sees the shared folder as Shared', async ({ bobPage: page }) => {
        await gotoReady(page, '/folders');
        await expect(card(page, 'Holiday')).toHaveAttribute('data-type', 'shared');
        await expect(card(page, 'Family Trip')).toHaveAttribute('data-type', 'family');
        await expect(card(page, 'Private')).toHaveCount(0);
    });

    test('filter chips narrow the list and are remembered', async ({ alicePage: page }) => {
        await gotoReady(page, '/folders');

        await page.getByTestId('filter-chip-personal').click();
        await expect(card(page, 'Family Trip')).toHaveCount(0);
        await expect(card(page, 'Holiday')).toBeVisible();
        await expect(card(page, 'Private')).toBeVisible();

        await page.getByTestId('filter-chip-family').click();
        await expect(card(page, 'Holiday')).toHaveCount(0);
        await expect(card(page, 'Family Trip')).toBeVisible();

        await page.getByTestId('filter-chip-shared').click();
        await expect(page.getByTestId('folder-list').getByTestId('empty-state')).toContainText('No shared folders');
        await expect(page.getByTestId('new-folder')).toBeVisible();

        await page.getByTestId('filter-chip-family').click();
        await expect(card(page, 'Family Trip')).toBeVisible();
        await page.reload();
        await expect(page.getByTestId('filter-chip-family')).toHaveAttribute('aria-pressed', 'true');
        await expect(card(page, 'Holiday')).toHaveCount(0);
    });

    test('the Shared filter shows what others share with bob', async ({ bobPage: page }) => {
        await gotoReady(page, '/folders?filter=shared');
        await expect(card(page, 'Holiday')).toBeVisible();
        await expect(card(page, 'Family Trip')).toHaveCount(0);
    });

    test('every sort order', async ({ alicePage: page }) => {
        await gotoReady(page, '/folders');
        const sort = async (key: string) => {
            await page.getByTestId('folder-sort').click();
            await page.getByTestId(`folder-sort-option-${key}`).click();
            await expect(page.getByTestId('folder-sort-option-' + key)).toHaveAttribute('aria-checked', 'true');
        };

        expect(await seededOrder(page)).toEqual(['Family Trip', 'Holiday', 'Private']);
        await sort('name_desc');
        await expect.poll(() => seededOrder(page)).toEqual(['Private', 'Holiday', 'Family Trip']);
        await sort('count_desc');
        await expect.poll(() => seededOrder(page)).toEqual(['Holiday', 'Family Trip', 'Private']);
        await sort('date_desc');
        await expect.poll(() => seededOrder(page)).toEqual(['Holiday', 'Private', 'Family Trip']);
        await expect(page.getByTestId('folder-sort')).toContainText('Date (Newest)');

        // The order is remembered
        await page.reload();
        await expect(page.getByTestId('folder-sort')).toContainText('Date (Newest)');
        expect(await seededOrder(page)).toEqual(['Holiday', 'Private', 'Family Trip']);
    });

    test('grid and list views, remembered', async ({ alicePage: page }) => {
        await gotoReady(page, '/folders');
        const list = page.getByTestId('folder-list');
        await expect(list).toHaveAttribute('data-view', 'grid');
        await expect(card(page, 'Holiday').getByTestId('folder-card-meta')).toHaveText('8 photos');

        await page.getByTestId('folder-view-toggle').click();
        await expect(list).toHaveAttribute('data-view', 'list');
        await expect(card(page, 'Holiday').getByTestId('folder-card-meta')).toHaveText('8 photos • Personal');
        await expect(card(page, 'Family Trip').getByTestId('folder-card-meta')).toHaveText('5 photos • Family');

        await page.reload();
        await expect(page.getByTestId('folder-list')).toHaveAttribute('data-view', 'list');
        await page.getByTestId('folder-view-toggle').click();
        await expect(page.getByTestId('folder-list')).toHaveAttribute('data-view', 'grid');
    });

    test('search ignores accents and case, and keeps working after a re-fetch', async ({ alicePage: page, folders }) => {
        const name = folders.name('Vacanță');
        await folders.create('alice', name);
        await gotoReady(page, '/folders');

        const search = page.getByTestId('folder-search');
        // The other project may have a "Vacanță" folder at the same time, so search for this one's suffix
        await search.fill(folders.name('vacanta'));
        await expect(cards(page).filter({ visible: true })).toHaveCount(1);
        await expect(card(page, name)).toBeVisible();
        await expect(page.getByTestId('folder-search-clear')).toBeVisible();

        // The query survives re-rendering the list
        await page.getByTestId('folder-view-toggle').click();
        await expect(page.getByTestId('folder-list')).toHaveAttribute('data-view', 'list');
        await expect(cards(page).filter({ visible: true })).toHaveCount(1);
        await page.getByTestId('folder-view-toggle').click();

        await search.fill('HOLIDAY');
        await expect(cards(page).filter({ visible: true })).toHaveCount(1);

        await search.fill('no such folder');
        await expect(page.getByTestId('folders-no-match')).toBeVisible();
        await expect(cards(page).filter({ visible: true })).toHaveCount(0);

        await page.getByTestId('folder-search-clear').click();
        await expect(search).toHaveValue('');
        await expect(page.getByTestId('folders-no-match')).toBeHidden();
        await expect(card(page, 'Holiday')).toBeVisible();
    });

    test('the search can be prefilled from the URL', async ({ alicePage: page }) => {
        await gotoReady(page, '/folders?q=priv');
        await expect(page.getByTestId('folder-search')).toHaveValue('priv');
        await expect(card(page, 'Private')).toBeVisible();
        await expect(card(page, 'Holiday')).toBeHidden();
    });
});

test.describe('folder management', () => {
    test('create a folder and land on its empty page', async ({ alicePage: page, folders }) => {
        const name = folders.name('Created');
        await gotoReady(page, '/folders');
        await page.getByTestId('new-folder').click();

        const dialog = page.getByTestId('folder-name-dialog');
        await expect(dialog).toBeVisible();
        const confirm = page.getByTestId('folder-name-confirm');
        await expect(confirm).toBeDisabled();
        await page.getByTestId('folder-name-input').fill('   ');
        await expect(confirm).toBeDisabled();
        await page.getByTestId('folder-name-input').fill(name);
        await expect(confirm).toBeEnabled();
        await confirm.click();

        await expect(page).toHaveURL(/\/folders\/\d+$/);
        await expect(page.getByTestId('folder-title')).toHaveText(name);
        await expect(page.getByTestId('folder-page').getByTestId('empty-state')).toContainText('This folder is empty');
        await expect(page.getByTestId('folder-count')).toHaveText('0 photos');
        await expect(page.getByTestId('folder-badge')).toContainText('Personal');
        // Remove it again after the test
        const id = Number(new URL(page.url()).pathname.split('/').pop());
        const owner = await folders.as('alice');
        await owner.delete(`/api/folders/${id}`);
    });

    test('a new folder can be made visible to the family', async ({ alicePage: page, folders }) => {
        const name = folders.name('Family made');
        await gotoReady(page, '/folders');
        await page.getByTestId('new-folder').click();
        await page.getByTestId('folder-name-input').fill(name);
        await page.getByTestId('folder-visibility-family').check();
        await page.getByTestId('folder-name-confirm').click();

        await expect(page.getByTestId('folder-badge')).toContainText('Family');
        const id = Number(new URL(page.url()).pathname.split('/').pop());
        await (await folders.as('alice')).delete(`/api/folders/${id}`);
    });

    test('an existing name opens that folder instead of failing', async ({ alicePage: page, folders }) => {
        const name = folders.name('Twice');
        const id = await folders.create('alice', name);
        await gotoReady(page, '/folders');
        await page.getByTestId('new-folder').click();
        await page.getByTestId('folder-name-input').fill(name);
        await page.getByTestId('folder-name-confirm').click();
        await expect(page).toHaveURL(new RegExp(`/folders/${id}$`));
    });

    test('rename and change the visibility', async ({ alicePage: page, folders }) => {
        const id = await folders.create('alice', folders.name('Before'));
        const newName = folders.name('After');
        await gotoReady(page, `/folders/${id}`);

        await page.getByTestId('folder-edit').click();
        const input = page.getByTestId('folder-name-input');
        await expect(input).toHaveValue(folders.name('Before'));
        await expect(page.getByTestId('folder-visibility-warning')).toBeHidden();

        await input.fill(newName);
        await page.getByTestId('folder-visibility-family').check();
        await expect(page.getByTestId('folder-visibility-warning')).toHaveText('All photos in this folder will become visible to the whole family.');
        await page.getByTestId('folder-visibility-personal').check();
        await expect(page.getByTestId('folder-visibility-warning')).toBeHidden();
        await page.getByTestId('folder-visibility-family').check();
        await page.getByTestId('folder-name-confirm').click();

        await expect(page.getByTestId('toast')).toHaveText(/Folder renamed/);
        await expect(page.getByTestId('folder-title')).toHaveText(newName);
        await expect(page).toHaveTitle(`${newName} - Family Photos`);
        await expect(page.getByTestId('folder-badge')).toContainText('Family');
        // Public folders can't be shared
        await expect(page.getByTestId('folder-share')).toHaveCount(0);

        await page.getByTestId('folder-edit').click();
        await page.getByTestId('folder-visibility-personal').check();
        await expect(page.getByTestId('folder-visibility-warning')).toHaveText('This folder and its photos will become private to you.');
        await page.getByTestId('folder-name-cancel').click();
        await expect(page.getByTestId('folder-name-dialog')).toHaveCount(0);
    });

    test('renaming to a taken name shows the message in the dialog', async ({ alicePage: page, folders }) => {
        const taken = folders.name('Taken');
        await folders.create('alice', taken);
        const id = await folders.create('alice', folders.name('Free'));
        await gotoReady(page, `/folders/${id}`);

        await page.getByTestId('folder-edit').click();
        await page.getByTestId('folder-name-input').fill(taken);
        await page.getByTestId('folder-name-confirm').click();
        await expect(page.getByTestId('folder-name-error')).toHaveText('A folder with this name already exists');
        await expect(page.getByTestId('folder-name-dialog')).toBeVisible();

        await page.getByTestId('folder-name-input').fill('bad/name');
        await page.getByTestId('folder-name-confirm').click();
        await expect(page.getByTestId('folder-name-error')).toContainText('can\'t contain');
        // Typing clears the message
        await page.getByTestId('folder-name-input').fill('fine');
        await expect(page.getByTestId('folder-name-error')).toBeHidden();
    });

    test('delete an empty folder', async ({ alicePage: page, folders }) => {
        const name = folders.name('Doomed');
        const id = await folders.create('alice', name);
        await gotoReady(page, `/folders/${id}`);

        await page.getByTestId('folder-more').click();
        await page.getByTestId('folder-delete').click();
        await expect(page.getByTestId('app-dialog')).toContainText(name);
        await page.getByTestId('confirm-accept').click();

        await expect(page).toHaveURL(/\/folders$/);
        await expect(page.getByTestId('toast')).toHaveText(/Folder deleted/);
        await expect(card(page, name)).toHaveCount(0);
        // SQLite may hand the freed id to a folder a parallel spec creates, so check the name, not the status
        expect(await (await page.request.get(`/folders/${id}`)).text()).not.toContain(name);
    });

    test('a folder that only has trashed photos can not be deleted', async ({ alicePage: page, folders }) => {
        const id = await folders.create('alice', folders.name('Trashed inside'));
        const photoId = await folders.upload('alice', id);
        const alice = await folders.as('alice');
        expect((await alice.post('/api/trash', { data: [photoId] })).ok()).toBe(true);

        try {
            await gotoReady(page, `/folders/${id}`);
            await expect(page.getByTestId('folder-count')).toHaveText('0 photos');
            await page.getByTestId('folder-more').click();
            await page.getByTestId('folder-delete').click();
            await page.getByTestId('confirm-accept').click();

            await expect(page.getByTestId('toast')).toHaveText(/Move or delete the photos in this folder first/);
            await expect(page).toHaveURL(new RegExp(`/folders/${id}$`));
        } finally {
            await alice.delete(`/api/delete/${photoId}`);
        }
    });

    test('a folder with photos offers no delete', async ({ alicePage: page }) => {
        await gotoReady(page, `/folders/${folderId('Private')}`);
        await expect(page.getByTestId('folder-edit')).toBeVisible();
        await expect(page.getByTestId('folder-delete')).toHaveCount(0);
        await expect(page.getByTestId('folder-count')).toHaveText('3 photos');
        await expect(page.getByTestId('folder-upload')).toHaveAttribute('href', `/upload?folder_id=${folderId('Private')}`);
    });
});

test.describe('folder page permissions', () => {
    test('bob can open the shared folder but not manage it', async ({ bobPage: page }) => {
        await gotoReady(page, `/folders/${folderId('Holiday')}`);
        await expect(page.getByTestId('folder-title')).toHaveText('Holiday');
        await expect(page.getByTestId('folder-count')).toHaveText('8 photos');
        await expect(page.getByTestId('folder-badge')).toContainText('Shared by Alice');
        for (const testid of ['folder-edit', 'folder-share', 'folder-delete', 'folder-more', 'folder-upload']) {
            await expect(page.getByTestId(testid)).toHaveCount(0);
        }
        await expect(page.getByTestId('folder-access')).toBeVisible();
    });

    test('a folder that is not shared is not found', async ({ bobPage: page }) => {
        const response = await gotoReady(page, `/folders/${folderId('Private')}`);
        expect(response?.status()).toBe(404);
        await expect(page.getByTestId('not-found')).toBeVisible();
    });

    test('the badge shows the number of people a folder is shared with', async ({ alicePage: page }) => {
        await gotoReady(page, `/folders/${folderId('Holiday')}`);
        await expect(page.getByTestId('folder-badge')).toContainText('Shared (1)');
        await expect(page.getByTestId('folder-share')).toBeVisible();
        await expect(page.getByTestId('folder-edit')).toBeVisible();
        await expect(page.getByTestId('folder-upload')).toBeVisible();
    });

    test('public folders can be edited by anyone but not shared', async ({ bobPage: page }) => {
        await gotoReady(page, `/folders/${folderId('Family Trip')}`);
        await expect(page.getByTestId('folder-badge')).toContainText('Family');
        await expect(page.getByTestId('folder-edit')).toBeVisible();
        await expect(page.getByTestId('folder-upload')).toBeVisible();
        await expect(page.getByTestId('folder-share')).toHaveCount(0);
        await expect(page.getByTestId('folder-access')).toHaveCount(0);
    });
});

test.describe('folder page grid', () => {
    /** Ids of the non-trashed seeded photos of a folder */
    const folderPhotos = (name: string) => Object.values(getSeed().photos)
        .filter((p) => p.folderId === folderId(name) && !p.trashed)
        .map((p) => String(p.id))
        .sort();
    const tileIds = async (page: Page) =>
        (await page.getByTestId('photo-tile').evaluateAll((tiles) => tiles.map((t) => (t as HTMLElement).dataset.photoId!)))
            .sort();

    async function gotoFolderGrid(page: Page, id: number) {
        await gotoReady(page, `/folders/${id}`);
        await expect(page.getByTestId('photo-grid')).toHaveAttribute('data-ready', '');
    }

    test("shows the folder's photos", async ({ alicePage: page }) => {
        await gotoFolderGrid(page, folderId('Holiday'));
        const expected = folderPhotos('Holiday');
        expect(expected.length).toBeGreaterThan(0);
        await expect(page.getByTestId('photo-grid')).toHaveAttribute('data-context', `folder:${folderId('Holiday')}`);
        await expect(page.getByTestId('photo-tile')).toHaveCount(expected.length);
        expect(await tileIds(page)).toEqual(expected);
        await expect(page.getByTestId('grid-empty')).toBeHidden();
        await expect(page.getByTestId('folder-count')).toHaveText(`${expected.length} photos`);
    });

    test("a grantee sees the shared folder's photos", async ({ bobPage: page }) => {
        await gotoFolderGrid(page, folderId('Holiday'));
        const expected = folderPhotos('Holiday');
        await expect(page.getByTestId('photo-tile')).toHaveCount(expected.length);
        expect(await tileIds(page)).toEqual(expected);
    });

    test('an empty folder shows the empty state, with Upload only when allowed', async ({ alicePage: page, bobPage, folders }) => {
        const id = await folders.create('alice', folders.name('Empty grid'));
        const alice = await folders.as('alice');
        expect((await alice.post('/api/sharing', {
            data: { folder_id: id, grantee_id: 'bob', can_upload: false, can_delete: false },
        })).ok()).toBe(true);

        await gotoFolderGrid(page, id);
        await expect(page.getByTestId('photo-tile')).toHaveCount(0);
        await expect(page.getByTestId('grid-empty')).toBeVisible();
        await expect(page.getByTestId('grid-empty')).toContainText('This folder is empty');
        await expect(page.getByTestId('grid-empty-upload')).toHaveAttribute('href', `/upload?folder_id=${id}`);

        // View only: no Upload button
        await gotoFolderGrid(bobPage, id);
        await expect(bobPage.getByTestId('grid-empty')).toBeVisible();
        await expect(bobPage.getByTestId('grid-empty-upload')).toHaveCount(0);
    });
});
