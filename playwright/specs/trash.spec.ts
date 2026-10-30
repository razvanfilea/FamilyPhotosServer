import type { APIRequestContext, Page, PlaywrightWorkerArgs } from '@playwright/test';
import { expect, gotoReady, loginAs } from '../fixtures/auth';
import { test } from '../fixtures/folders';
import { apiAs, photoInfo, uploadPhoto } from '../fixtures/library';
import { photo } from '../harness/seed';
import { plural } from '../../assets/js/core/format.js';

const tile = (page: Page, id: number) => page.locator(`[data-testid="photo-tile"][data-photo-id="${id}"]`);
const toast = (page: Page, text: string) => page.getByTestId('toast').filter({ hasText: text });

async function expectEmpty(page: Page) {
    await expect(page.getByTestId('grid-empty')).toBeVisible();
    await expect(page.getByTestId('grid-empty')).toContainText('Trash is empty');
}

/** The number of photos in the trash (other specs trash and restore alice's photos for a moment) */
async function trashTotal(page: Page) {
    return Number(await page.getByTestId('photo-grid').getAttribute('data-total'));
}

/** Selects with the check circle */
async function select(page: Page, ...ids: number[]) {
    for (const id of ids) {
        await tile(page, id).scrollIntoViewIfNeeded();
        await tile(page, id).locator('.photo-tile__check').click();
    }
}

async function gotoTrash(page: Page) {
    await gotoReady(page, '/trash');
    await expect(page.getByTestId('photo-grid')).toHaveAttribute('data-ready', '');
}

test.describe('trash page (read only)', () => {
    test('shows the trashed photos with their days left', async ({ alicePage: page }) => {
        const trashed = photo('alice/alice-f.jpg');
        await gotoTrash(page);

        await expect(page.getByTestId('trash-title')).toHaveText('Trash');
        await expect(page.getByTestId('trash-info')).toHaveText('Photos in trash are automatically deleted after 30 days');
        await expect(page.getByTestId('trash-count')).toHaveText(plural(await trashTotal(page), 'photo'));
        await expect(tile(page, trashed.id).locator('.photo-tile__badge')).toHaveText('30 days');
        await expect(page.getByTestId('trash-restore-all')).toBeEnabled();
        await expect(page.getByTestId('trash-empty')).toBeEnabled();
        await expect(page.getByTestId('nav-trash')).toHaveAttribute('aria-current', 'page');
    });

    test('a tile opens the viewer in the trash context', async ({ alicePage: page }) => {
        const trashed = photo('alice/alice-f.jpg');
        await gotoTrash(page);

        await tile(page, trashed.id).click();
        await expect(page.getByTestId('viewer')).toHaveAttribute('data-photo-id', String(trashed.id));
        await expect(page).toHaveURL(new RegExp(`/trash\\?photo=${trashed.id}$`));
        await expect(page.getByTestId('viewer-restore')).toBeVisible();
        await expect(page.getByTestId('viewer-delete-forever')).toBeVisible();
        await expect(page.getByTestId('viewer-favorite')).toBeHidden();
    });

    test('shows an empty state when the trash is empty', async ({ bobPage: page }) => {
        await gotoTrash(page);
        await expectEmpty(page);
        await expect(page.getByTestId('trash-count')).toHaveText('0 photos');
        await expect(page.getByTestId('trash-restore-all')).toBeDisabled();
        await expect(page.getByTestId('trash-empty')).toBeDisabled();
    });
});

// Restore all and Empty trash act on the whole trash, which includes other specs' (and the public)
// photos. They run for real, but only the id lists are narrowed to the photos this test trashed.
// erin is used by no other spec; Mobile would race with Desktop on her trash.
test.describe('trash actions', () => {
    test.skip(({ isMobile }) => isMobile, 'Shares erin\'s trash with the desktop run');

    /** Uploads and trashes `count` photos as erin; the ids are the only ones the page may act on. */
    async function trashPhotos(
        page: Page,
        playwright: PlaywrightWorkerArgs['playwright'],
        baseURL: string | undefined,
        count: number,
        narrow = true,
    ) {
        await loginAs(page, 'erin');
        const erin = await apiAs(playwright, baseURL, 'erin');
        const time = Date.UTC(2034, 0, 1) / 1000;
        const ids: number[] = [];
        for (let i = 0; i < count; i++) {
            ids.push(await uploadPhoto(erin, `erin-trash-${i}.jpg`, time - i * 86400));
        }
        expect((await erin.post('/api/trash', { data: ids })).ok()).toBe(true);
        if (narrow) {
            await page.route('**/ui/trash/deletable-ids', (route) => route.fulfill({ json: ids }));
            await page.route(/\/api\/context\/ids\?ctx=trash/, (route) => route.fulfill({ json: ids }));
        }
        return { erin, ids };
    }

    const exists = async (erin: APIRequestContext, id: number) => (await erin.get(`/api/photos/${id}/info`)).status() === 200;

    test('Restore all restores the photos', async ({ page, playwright, baseURL }) => {
        const { erin, ids } = await trashPhotos(page, playwright, baseURL, 2);
        try {
            await gotoTrash(page);
            await page.getByTestId('trash-restore-all').click();
            await expect(toast(page, '2 photos restored')).toBeVisible();

            for (const id of ids) {
                expect((await photoInfo(erin, id)).photo.trashed_on).toBeNull();
            }
            await expect(page).toHaveURL(/\/trash$/);
            for (const id of ids) await expect(tile(page, id)).toHaveCount(0);
        } finally {
            await erin.post('/api/trash', { data: ids });
            for (const id of ids) await erin.delete(`/api/delete/${id}`);
            await erin.dispose();
        }
    });

    test('Empty trash asks first and deletes the photos for good', async ({ page, playwright, baseURL }) => {
        const { erin, ids } = await trashPhotos(page, playwright, baseURL, 2);
        try {
            await gotoTrash(page);
            await page.getByTestId('trash-empty').click();
            const dialog = page.getByTestId('delete-dialog');
            await expect(dialog).toContainText('Delete 2 photos permanently?');
            await expect(dialog).toContainText('cannot be recovered');
            await page.getByTestId('delete-cancel').click();
            await expect(dialog).toBeHidden();
            for (const id of ids) expect(await exists(erin, id)).toBe(true);

            await page.getByTestId('trash-empty').click();
            await page.getByTestId('delete-confirm').click();
            await expect(toast(page, '2 photos deleted permanently')).toBeVisible();
            for (const id of ids) expect(await exists(erin, id)).toBe(false);
            for (const id of ids) await expect(tile(page, id)).toHaveCount(0);
        } finally {
            for (const id of ids) await erin.delete(`/api/delete/${id}`);
            await erin.dispose();
        }
    });

    // The failure is the one thing that is answered by the test: the ids are the user's own
    test('Empty trash reports photos that could not be deleted', async ({ page, playwright, baseURL }) => {
        const { erin, ids } = await trashPhotos(page, playwright, baseURL, 1);
        try {
            await page.route('**/api/delete/*', (route) => route.fulfill({ status: 404 }));
            await gotoTrash(page);

            await page.getByTestId('trash-empty').click();
            await page.getByTestId('delete-confirm').click();
            // Nothing was deleted: the dialog stays open with the error
            await expect(page.getByTestId('delete-error')).toContainText("You can't permanently delete some of the selected photos");
            await expect(page.getByTestId('delete-dialog')).toBeVisible();
        } finally {
            await page.unroute('**/api/delete/*');
            for (const id of ids) await erin.delete(`/api/delete/${id}`);
            await erin.dispose();
        }
    });

    test('the selection bar restores, then deletes for good', async ({ page, playwright, baseURL }) => {
        const { erin, ids } = await trashPhotos(page, playwright, baseURL, 3, false);
        const [first, second, third] = ids;
        try {
            await gotoTrash(page);
            const before = await trashTotal(page);
            await select(page, first, second);

            // Only what the trash can do
            await expect(page.getByTestId('selection-restore')).toBeVisible();
            await expect(page.getByTestId('selection-delete-forever')).toBeVisible();
            await expect(page.getByTestId('selection-delete')).toBeHidden();
            await expect(page.getByTestId('selection-favorite')).toBeHidden();

            await page.getByTestId('selection-restore').click();
            await expect(toast(page, '2 photos restored')).toBeVisible();
            await expect(tile(page, first)).toHaveCount(0);
            await expect(tile(page, second)).toHaveCount(0);
            await expect(page.getByTestId('trash-count')).toHaveText(plural(before - 2, 'photo'));
            for (const id of [first, second]) expect((await photoInfo(erin, id)).photo.trashed_on).toBeNull();

            await select(page, third);
            await page.keyboard.press('Delete');
            const dialog = page.getByTestId('delete-dialog');
            await expect(dialog).toContainText('permanently');
            await page.getByTestId('delete-confirm').click();
            await expect(toast(page, '1 photo deleted permanently')).toBeVisible();
            expect((await erin.get(`/api/photos/${third}/info`)).status()).toBe(404);
        } finally {
            await erin.post('/api/trash', { data: [first, second] });
            for (const id of ids) await erin.delete(`/api/delete/${id}`);
            await erin.dispose();
        }
    });

    test('a photo of a shared folder can be restored but not deleted for good', async ({ page, playwright, baseURL, folders }) => {
        const alice = await folders.as('alice');
        const folder = await folders.create('alice', folders.name('Shared trash'));
        expect((await alice.post('/api/sharing', {
            data: { folder_id: folder, grantee_id: 'erin', can_upload: false, can_delete: true },
        })).ok()).toBe(true);
        const id = await folders.upload('alice', folder);
        expect((await alice.post('/api/trash', { data: [id] })).ok()).toBe(true);

        await loginAs(page, 'erin');
        const erin = await apiAs(playwright, baseURL, 'erin');
        try {
            await gotoTrash(page);
            await expect(tile(page, id)).toHaveAttribute('data-owner', 'alice');
            await select(page, id);
            await expect(page.getByTestId('selection-restore')).not.toHaveAttribute('aria-disabled', 'true');
            await expect(page.getByTestId('selection-delete-forever')).toHaveAttribute('aria-disabled', 'true');

            await page.getByTestId('selection-delete-forever').click({ force: true });
            await expect(toast(page, "You can't permanently delete some of the selected photos")).toBeVisible();
            await expect(page.getByTestId('delete-dialog')).toBeHidden();
            // "Empty trash" doesn't offer it either
            expect(await (await page.request.get('/ui/trash/deletable-ids')).json()).not.toContain(id);

            await page.getByTestId('selection-restore').click();
            await expect(toast(page, '1 photo restored')).toBeVisible();
            expect((await photoInfo(alice, id)).photo.trashed_on).toBeNull();
        } finally {
            await alice.post('/api/trash', { data: [id] });
            await alice.delete(`/api/delete/${id}`);
            await erin.dispose();
        }
    });
});
