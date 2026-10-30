import type { Page } from '@playwright/test';
import { expect, gotoReady, loginAs, test } from '../fixtures/auth';
import { apiAs, photoInfo } from '../fixtures/library';

const position = (page: Page) => page.getByTestId('dup-position');
/** The copies of the group on screen */
const copiesOf = (page: Page) => page.locator('[data-group]:not([hidden])').getByTestId('dup-keep');
const image = (page: Page) => page.getByTestId('dup-stage').locator('img');

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
        }
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

    // carol's fixtures dup-a.jpg and Copies/dup-b.jpg are byte-identical
    test('navigate between groups and keep a copy', async ({ page, playwright, baseURL }) => {
        await loginAs(page, 'carol');
        const carol = await apiAs(playwright, baseURL, 'carol');
        const photos = ((await (await carol.get('/api/sync/full')).json()).photos as { id: number, name: string }[]);
        const rootCopy = photos.find((p) => p.name === 'dup-a.jpg')!.id;
        const folderCopy = photos.find((p) => p.name === 'dup-b.jpg')!.id;
        // Makes the test repeatable (a retry): both copies are back
        expect((await carol.post('/api/trash/restore', { data: [rootCopy, folderCopy] })).ok()).toBe(true);

        // Groups are ordered by their first photo: carol's or the public fixture pair
        const publicPair = photos.filter((p) => p.name.startsWith('duplicate-')).map((p) => p.id);
        const first = Math.min(rootCopy, folderCopy);
        const groupNumber = first < Math.min(...publicPair) ? 1 : 2;

        await gotoDuplicates(page);
        await expect(position(page)).toHaveText('Group 1 of 2');
        if (groupNumber === 2) {
            await page.getByTestId('dup-next').click();
        }
        await expect(position(page)).toHaveText(`Group ${groupNumber} of 2`);
        await expect(image(page)).toHaveAttribute('src', `/api/screen/${first}`);

        const copies = copiesOf(page);
        await expect(copies).toHaveCount(2);
        await expect(copies.filter({ hasText: 'Copies' })).toContainText('Personal');
        await expect(copies.filter({ hasText: 'No folder' })).toContainText('Personal');

        // Arrow keys move between groups
        await page.keyboard.press(groupNumber === 1 ? 'ArrowRight' : 'ArrowLeft');
        await expect(position(page)).toHaveText(`Group ${3 - groupNumber} of 2`);
        await expect(image(page)).not.toHaveAttribute('src', `/api/screen/${first}`);
        await page.keyboard.press(groupNumber === 1 ? 'ArrowLeft' : 'ArrowRight');
        await expect(position(page)).toHaveText(`Group ${groupNumber} of 2`);
        await expect(image(page)).toHaveAttribute('src', `/api/screen/${first}`);

        // Keep the copy in the folder: the other goes to the trash
        await copies.filter({ hasText: 'Copies' }).click();
        await page.getByTestId('delete-confirm').click();
        await expect(page.getByTestId('toast').filter({ hasText: '1 duplicate moved to trash' })).toBeVisible();

        await expect(position(page)).toHaveText('Group 1 of 1');
        expect((await photoInfo(carol, rootCopy)).photo.trashed_on).not.toBeNull();
        expect((await photoInfo(carol, folderCopy)).photo.trashed_on).toBeNull();

        await page.reload();
        await expect(position(page)).toHaveText('Group 1 of 1');
        await carol.dispose();
    });
});
