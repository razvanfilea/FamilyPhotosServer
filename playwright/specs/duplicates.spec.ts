import type { Page, PlaywrightWorkerArgs } from '@playwright/test';
import { expect, gotoReady, loginAs, test } from '../fixtures/auth';
import { apiAs, photoInfo } from '../fixtures/library';

const position = (page: Page) => page.getByTestId('dup-position');
/** The copies of the group on screen */
const copiesOf = (page: Page) => page.locator('[data-group]:not([hidden])').getByTestId('dup-keep');
const image = (page: Page) => page.getByTestId('dup-stage').locator('img');
const summary = (page: Page) => page.getByTestId('dup-summary');
const SIZE = '\\d+(\\.\\d)? (B|KB|MB|GB)';

/** Shows the group that has a copy with the file name, groups being ordered by their first copy */
async function showGroupOf(page: Page, name: string) {
    for (let i = 0; i < 10; i++) {
        if (await copiesOf(page).filter({ hasText: name }).count() > 0) return;
        await page.getByTestId('dup-next').click();
    }
    throw new Error(`No group with ${name}`);
}

/** Answers the trash request instead of the server (nothing is trashed for real) and returns the ids it got */
async function mockTrash(page: Page) {
    const requests: number[][] = [];
    await page.route('**/api/trash', async (route) => {
        requests.push(route.request().postDataJSON());
        await route.fulfill({ json: [] });
    });
    return requests;
}

async function gotoDuplicates(page: Page) {
    await gotoReady(page, '/duplicates');
    await expect(position(page)).toBeVisible();
}

test.describe('the duplicate fixtures (read only)', () => {
    test('shows the group with its copies', async ({ alicePage: page }) => {
        await gotoDuplicates(page);

        await expect(page.getByTestId('duplicates-title')).toHaveText('Duplicates');
        await expect(position(page)).toHaveText(/^Group 1 of \d+$/);
        await expect(page.getByTestId('dup-prev')).toBeDisabled();
        await expect(image(page)).toHaveAttribute('src', /^\/api\/screen\/\d+$/);

        // duplicate-a.jpg and duplicate-b.jpg: public, no folder
        const copies = copiesOf(page);
        await expect(copies).toHaveCount(2);
        await expect(page.getByText('Which do you want to keep?')).toBeVisible();
        for (const copy of await copies.all()) {
            await expect(copy).toContainText('Family');
            await expect(copy).toContainText('No folder');
            await expect(copy).toContainText('14 Aug 2023');
            await expect(copy.locator('img')).toHaveAttribute('src', /^\/api\/preview\/\d+$/);
            // The size is the same for every copy: it's shown once, for the group
            await expect(copy).not.toContainText(new RegExp(SIZE));
        }
        expect((await copies.getByTestId('dup-name').allTextContents()).sort()).toEqual(['duplicate-a.jpg', 'duplicate-b.jpg']);
        await expect(page.locator('[data-group]:not([hidden])').getByTestId('dup-frees'))
            .toHaveText(new RegExp(`^Frees ${SIZE}$`));
    });

    test('the oldest copy is suggested when nothing else tells them apart', async ({ alicePage: page }) => {
        await gotoDuplicates(page);

        await expect(page.getByTestId('dup-suggested')).toHaveCount(1);
        await expect(copiesOf(page).first()).toHaveAttribute('data-suggested', '');
        await expect(copiesOf(page).first()).toContainText('Suggested');
        await expect(copiesOf(page).last()).not.toHaveAttribute('data-suggested', '');
    });

    test('shows how many groups there are and what can be freed', async ({ alicePage: page }) => {
        await gotoDuplicates(page);

        await expect(summary(page)).toHaveText(new RegExp(`^1 group · ${SIZE} can be freed$`));
        // alice sees the public pair: both are Family copies in no folder
        await expect(page.getByTestId('dup-bulk-extra')).toHaveText('Remove extra copies (1)');
        await expect(page.getByTestId('dup-bulk-family')).toBeHidden();
    });

    test('the large image shows a spinner until it has loaded', async ({ alicePage: page }) => {
        await page.route('**/api/screen/*', async (route) => {
            await new Promise((resolve) => setTimeout(resolve, 600));
            await route.continue();
        });
        // Not waiting for the load event: the image is part of it
        await page.goto('/duplicates', { waitUntil: 'commit' });
        await expect(page.getByTestId('dup-loading')).toBeVisible();
        await expect(page.getByTestId('dup-loading')).toBeHidden();
        await expect(image(page)).toHaveJSProperty('complete', true);
    });

    test('each copy has its own info button', async ({ alicePage: page }) => {
        await gotoDuplicates(page);
        const infos = page.locator('[data-group]:not([hidden])').getByTestId('dup-copy-info');
        await expect(infos).toHaveCount(2);

        const name = await copiesOf(page).last().getByTestId('dup-name').textContent();
        await infos.last().click();
        await expect(page.getByTestId('viewer-info-name')).toHaveText(name!);
    });

    test('removing the extra copies asks first, then trashes all but the suggested copy', async ({ alicePage: page }) => {
        const requests = await mockTrash(page);
        await gotoDuplicates(page);
        const suggested = Number(await copiesOf(page).first().getAttribute('data-photo-id'));
        const other = Number(await copiesOf(page).last().getAttribute('data-photo-id'));

        await page.getByTestId('dup-bulk-extra').click();
        const dialog = page.getByTestId('delete-dialog');
        await expect(dialog).toContainText('Remove extra copies');
        await expect(dialog).toContainText(new RegExp(`1 photo will be moved to the trash, freeing ${SIZE}`));
        await expect(dialog).toContainText('not deleted permanently');
        await expect(page.getByTestId('delete-confirm')).toHaveText('Move to trash');
        // Cancelling trashes nothing
        await page.getByTestId('delete-cancel').click();
        expect(requests).toEqual([]);
        await expect(copiesOf(page)).toHaveCount(2);

        await page.getByTestId('dup-bulk-extra').click();
        await page.getByTestId('delete-confirm').click();
        await expect(page.getByTestId('toast').filter({ hasText: '1 duplicate moved to trash' })).toBeVisible();
        expect(requests).toEqual([[other]]);
        expect(suggested).not.toEqual(other);
        await expect(page.getByTestId('duplicates-empty')).toBeVisible();
        await expect(page.getByTestId('dup-bulk-extra')).toBeHidden();
    });

    test('the info button shows the details of the photo', async ({ alicePage: page }) => {
        await gotoDuplicates(page);
        await page.getByTestId('dup-info').click();
        await expect(page.getByTestId('app-dialog')).toBeVisible();
        await expect(page.getByTestId('viewer-info-name')).toHaveText(/^duplicate-[ab]\.jpg$/);
    });

    test('double click zooms the image in place', async ({ alicePage: page, isMobile }) => {
        test.skip(isMobile, 'Mouse gesture');
        await gotoDuplicates(page);
        const stage = page.getByTestId('dup-stage');
        await expect(image(page)).toHaveJSProperty('complete', true);

        await stage.dblclick();
        await expect(image(page)).toHaveCSS('transform', /^matrix\(2\.5, 0, 0, 2\.5/);
        await stage.dblclick();
        await expect(image(page)).toHaveCSS('transform', 'none');
    });

    test('keeping a copy trashes the others, then the empty state shows', async ({ alicePage: page }) => {
        // Nothing is deleted for real: the trash request is answered by the test
        let trashed: number[] = [];
        await page.route('**/api/trash', async (route) => {
            trashed = route.request().postDataJSON();
            await route.fulfill({ json: [] });
        });
        await gotoDuplicates(page);
        const groups = Number((await position(page).textContent())!.match(/of (\d+)/)![1]);
        const kept = await copiesOf(page).first().getAttribute('data-photo-id');
        const other = await copiesOf(page).last().getAttribute('data-photo-id');

        await copiesOf(page).first().click();
        await expect(page.getByTestId('delete-dialog')).toBeVisible();
        await expect(page.getByTestId('delete-dialog').locator('img')).toHaveCount(1);
        await page.getByTestId('delete-confirm').click();

        await expect(page.getByTestId('toast').filter({ hasText: '1 duplicate moved to trash' })).toBeVisible();
        expect(trashed).toEqual([Number(other)]);
        expect(kept).not.toEqual(other);
        if (groups === 1) {
            await expect(page.getByTestId('duplicates-empty')).toBeVisible();
            await expect(page.getByTestId('duplicates-empty')).toContainText('No duplicates found');
            await expect(page.getByTestId('dup-stage')).toBeHidden();
            await expect(page.getByTestId('dup-info')).toBeHidden();
        } else {
            await expect(position(page)).toHaveText(`Group 1 of ${groups - 1}`);
        }
    });
});

test.describe('duplicates of a user', () => {
    test.describe.configure({ mode: 'serial' });
    test.skip(({ isMobile }) => isMobile, 'Mutates carol\'s photos');

    // carol sees three groups: her dup-a.jpg and Copies/dup-b.jpg (byte-identical, both Personal),
    // her snow-copy.jpg with the public snow.jpg, and the public duplicate-a.jpg and duplicate-b.jpg
    async function restoreDuplicates(playwright: PlaywrightWorkerArgs['playwright'], baseURL: string | undefined) {
        const carol = await apiAs(playwright, baseURL, 'carol');
        const photos = ((await (await carol.get('/api/sync/full')).json()).photos as { id: number, name: string }[]);
        const idOf = (name: string) => photos.find((p) => p.name === name)!.id;
        // Makes the tests repeatable (a retry): both copies are back
        expect((await carol.post('/api/trash/restore', { data: [idOf('dup-a.jpg'), idOf('dup-b.jpg')] })).ok()).toBe(true);
        return { carol, idOf };
    }

    test('navigate between groups and keep a copy', async ({ page, playwright, baseURL }) => {
        await loginAs(page, 'carol');
        const { carol, idOf } = await restoreDuplicates(playwright, baseURL);
        const rootCopy = idOf('dup-a.jpg');
        const folderCopy = idOf('dup-b.jpg');
        const first = Math.min(rootCopy, folderCopy);

        await gotoDuplicates(page);
        await expect(position(page)).toHaveText('Group 1 of 3');
        await showGroupOf(page, 'dup-a.jpg');
        const here = await position(page).textContent();
        await expect(image(page)).toHaveAttribute('src', `/api/screen/${first}`);

        const copies = copiesOf(page);
        await expect(copies).toHaveCount(2);
        await expect(copies.filter({ hasText: 'Copies' })).toContainText('Personal');
        await expect(copies.filter({ hasText: 'No folder' })).toContainText('Personal');
        // A copy in a folder is suggested over one that is in none
        await expect(copies.filter({ hasText: 'Copies' })).toHaveAttribute('data-suggested', '');
        await expect(copies.filter({ hasText: 'No folder' })).not.toHaveAttribute('data-suggested', '');
        // Different folders: no bulk action applies to this group
        await expect(page.locator('[data-group]:not([hidden])')).toHaveAttribute('data-bulk', '');

        // Arrow keys move between groups
        const last = here === 'Group 3 of 3';
        await page.keyboard.press(last ? 'ArrowLeft' : 'ArrowRight');
        await expect(position(page)).not.toHaveText(here!);
        await expect(image(page)).not.toHaveAttribute('src', `/api/screen/${first}`);
        await page.keyboard.press(last ? 'ArrowRight' : 'ArrowLeft');
        await expect(position(page)).toHaveText(here!);
        await expect(image(page)).toHaveAttribute('src', `/api/screen/${first}`);

        // Keep the copy in the folder: the other goes to the trash
        await copies.filter({ hasText: 'Copies' }).click();
        await page.getByTestId('delete-confirm').click();
        await expect(page.getByTestId('toast').filter({ hasText: '1 duplicate moved to trash' })).toBeVisible();

        await expect(position(page)).toHaveText(/ of 2$/);
        await expect(summary(page)).toContainText('2 groups');
        expect((await photoInfo(carol, rootCopy)).photo.trashed_on).not.toBeNull();
        expect((await photoInfo(carol, folderCopy)).photo.trashed_on).toBeNull();

        await page.reload();
        await expect(position(page)).toHaveText('Group 1 of 2');
        await carol.dispose();
    });

    test('keeping the Family copy of a Personal and Family pair in bulk', async ({ page, playwright, baseURL }) => {
        await loginAs(page, 'carol');
        const { carol, idOf } = await restoreDuplicates(playwright, baseURL);
        const requests = await mockTrash(page);

        await gotoDuplicates(page);
        await expect(summary(page)).toContainText('3 groups');
        await expect(page.getByTestId('dup-bulk-family')).toHaveText('Keep the Family copy (1)');
        await expect(page.getByTestId('dup-bulk-extra')).toHaveText('Remove extra copies (1)');

        // The Family copy is suggested, and the Personal one would go
        await showGroupOf(page, 'snow-copy.jpg');
        const copies = copiesOf(page);
        await expect(copies.filter({ hasText: 'Family' })).toHaveAttribute('data-suggested', '');
        await expect(copies.filter({ hasText: 'Personal' })).not.toHaveAttribute('data-suggested', '');

        await page.getByTestId('dup-bulk-family').click();
        const dialog = page.getByTestId('delete-dialog');
        await expect(dialog).toContainText('Keep the Family copy');
        await expect(dialog).toContainText(new RegExp(`1 photo will be moved to the trash, freeing ${SIZE}`));
        await page.getByTestId('delete-confirm').click();

        await expect(page.getByTestId('toast').filter({ hasText: '1 duplicate moved to trash' })).toBeVisible();
        expect(requests).toEqual([[idOf('snow-copy.jpg')]]);
        await expect(summary(page)).toContainText('2 groups');
        await expect(page.getByTestId('dup-bulk-family')).toBeHidden();
        await expect(page.getByTestId('dup-bulk-extra')).toBeVisible();
        await expect(position(page)).toHaveText(/ of 2$/);
        await carol.dispose();
    });
});
