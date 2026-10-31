import type { Locator, Page } from '@playwright/test';
import { expect, gotoReady, loginAs, test, type UserId } from '../fixtures/auth';
import { photoInfo as libraryPhotoInfo, uploadPhoto } from '../fixtures/library';
import { folderId, getSeed, type SeedPhoto } from '../harness/seed';

// Read-only tests use alice's and bob's seeded photos. Tests that change photos run as heidi, on
// uploads with far-future dates (each upload unique), on desktop only. The selection on the trash,
// favorites and large files pages is tested in trash.spec.ts and utilities.spec.ts.

/** alice's or bob's timeline (filter all), in order */
function timeline(user: UserId): SeedPhoto[] {
    const holiday = folderId('Holiday');
    return Object.values(getSeed().photos)
        .filter((p) => !p.trashed
            && (p.userId === user || p.userId === null || (user === 'bob' && p.folderId === holiday)))
        .sort((a, b) => b.createdAt - a.createdAt || b.id - a.id);
}

const monthOf = (photo: SeedPhoto) => new Date(photo.createdAt * 1000).toISOString().slice(0, 7);

const tiles = (page: Page) => page.getByTestId('photo-tile');
const tile = (page: Page, id: number) => page.locator(`[data-testid="photo-tile"][data-photo-id="${id}"]`);
const tileAt = (page: Page, index: number) => page.locator(`[data-testid="photo-tile"][data-index="${index}"]`);
const check = (target: Locator) => target.locator('.photo-tile__check');
const bar = (page: Page) => page.getByTestId('selection-bar');
const count = (page: Page) => page.getByTestId('selection-count');
const toast = (page: Page, text: string | RegExp) => page.getByTestId('toast').filter({ hasText: text });

async function gotoGrid(page: Page, url = '/') {
    await gotoReady(page, url);
    await expect(page.getByTestId('photo-grid')).toHaveAttribute('data-ready', '');
}

async function selectedCount(page: Page) {
    return Number.parseInt((await count(page).textContent()) ?? '0', 10);
}

/** Selects with the check circle, which also works in selection mode */
async function select(page: Page, ...ids: number[]) {
    for (const id of ids) await check(tile(page, id)).click();
}

const center = async (target: Locator) => {
    const box = (await target.boundingBox())!;
    return { x: box.x + box.width / 2, y: box.y + box.height / 2 };
};

/** Three of heidi's photos, newest first, `year` far in the future so they top the timeline */
async function heidiPhotos(page: Page, year: number, names = ['a', 'b', 'c']) {
    await loginAs(page, 'heidi');
    const ids = [];
    for (const [i, name] of names.entries()) {
        ids.push(await uploadPhoto(page.request, `sel-${year}-${name}.jpg`, Date.UTC(year, 0, 10) / 1000 - i * 86400));
    }
    return ids;
}

const photoInfo = async (page: Page, id: number) => (await libraryPhotoInfo(page.request, id)).photo;

test.describe('entering and leaving selection mode', () => {
    test('the check circle selects, a click toggles, Escape and the close button clear', async ({ alicePage: page, isMobile }) => {
        test.skip(isMobile, 'Pointer and keyboard');
        const [first, second, third] = timeline('alice');
        await gotoGrid(page);
        await expect(bar(page)).toBeHidden();

        await check(tile(page, first.id)).click();
        await expect(bar(page)).toBeVisible();
        await expect(count(page)).toHaveText('1 selected');
        await expect(tile(page, first.id)).toHaveAttribute('aria-pressed', 'true');
        await expect(tile(page, first.id)).toHaveClass(/is-selected/);
        await expect(page.getByTestId('photo-grid')).toHaveClass(/selecting/);
        await expect(tile(page, second.id)).toHaveAttribute('aria-pressed', 'false');

        // In selection mode a plain click toggles and doesn't open the viewer
        await tile(page, second.id).click();
        await expect(count(page)).toHaveText('2 selected');
        await expect(page.getByTestId('viewer')).toBeHidden();
        await expect(page).toHaveURL(/\/$/);
        await tile(page, second.id).click();
        await expect(count(page)).toHaveText('1 selected');

        await page.keyboard.press('Escape');
        await expect(bar(page)).toBeHidden();
        await expect(tile(page, first.id)).not.toHaveClass(/is-selected/);
        await expect(tile(page, first.id)).not.toHaveAttribute('aria-pressed');

        // Ctrl+click enters the mode too; the close button leaves it
        await tile(page, third.id).click({ modifiers: ['Control'] });
        await expect(count(page)).toHaveText('1 selected');
        await page.getByTestId('selection-clear').click();
        await expect(bar(page)).toBeHidden();
        // A click outside the mode opens the viewer again
        await tile(page, third.id).click();
        await expect(page.getByTestId('viewer')).toBeVisible();
    });

    test('x selects the focused tile', async ({ alicePage: page, isMobile }) => {
        test.skip(isMobile, 'Keyboard');
        const [first] = timeline('alice');
        await gotoGrid(page);
        await tile(page, first.id).focus();
        await page.keyboard.press('x');
        await expect(count(page)).toHaveText('1 selected');
        await page.keyboard.press('x');
        await expect(bar(page)).toBeHidden();
    });

    test('a long-press selects (touch)', async ({ alicePage: page, isMobile }) => {
        test.skip(!isMobile, 'Touch');
        const [first, second] = timeline('alice');
        await gotoGrid(page);
        const cdp = await page.context().newCDPSession(page);
        const touch = (type: 'touchStart' | 'touchEnd', x?: number, y?: number) => cdp.send('Input.dispatchTouchEvent', {
            type, touchPoints: x === undefined ? [] : [{ x, y: y!, id: 1 }],
        });
        const { x, y } = await center(tile(page, first.id));

        await touch('touchStart', x, y);
        await page.waitForTimeout(600);
        await touch('touchEnd');
        await expect(count(page)).toHaveText('1 selected');
        await expect(tile(page, first.id)).toHaveClass(/is-selected/);
        // Releasing the long-press doesn't toggle it back or open the viewer
        await page.waitForTimeout(600);
        await expect(page.getByTestId('viewer')).toBeHidden();
        await expect(count(page)).toHaveText('1 selected');

        // A tap toggles from then on
        await tile(page, second.id).tap();
        await expect(count(page)).toHaveText('2 selected');
        await tile(page, first.id).tap();
        await tile(page, second.id).tap();
        await expect(bar(page)).toBeHidden();
    });

    test('the fast scroller is hidden while selecting', async ({ alicePage: page, isMobile }) => {
        test.skip(isMobile, 'Pointer');
        const [first] = timeline('alice');
        await gotoGrid(page);
        const scroller = page.locator('.fast-scroller');
        await page.mouse.move(600, 400);
        await page.mouse.wheel(0, 300);
        await expect(scroller).toHaveAttribute('data-visible', 'true');
        await page.evaluate(() => window.scrollTo(0, 0));

        await check(tile(page, first.id)).click();
        await expect(scroller).toHaveAttribute('data-visible', 'false');
        await page.mouse.wheel(0, 300);
        await page.waitForTimeout(200);
        await expect(scroller).toHaveAttribute('data-visible', 'false');
    });

    test('the bar shows icons on a phone and labels on a desktop', async ({ alicePage: page, isMobile }) => {
        const [first] = timeline('alice');
        await gotoGrid(page);
        if (isMobile) {
            await tile(page, first.id).dispatchEvent('contextmenu');
            await check(tile(page, first.id)).tap();
        } else {
            await check(tile(page, first.id)).click();
        }
        await expect(bar(page)).toBeVisible();
        const labels = page.getByTestId('selection-delete').locator('[data-slot="label"]');
        if (isMobile) await expect(labels).toBeHidden();
        else await expect(labels).toBeVisible();
        for (const action of ['favorite', 'download', 'move', 'delete']) {
            await expect(page.getByTestId(`selection-${action}`)).toBeVisible();
        }
        await expect(page.getByTestId('selection-restore')).toBeHidden();
        await expect(page.getByTestId('selection-delete-forever')).toBeHidden();
        // The bar fits the screen
        const box = (await bar(page).boundingBox())!;
        expect(box.x).toBeGreaterThanOrEqual(0);
        expect(box.x + box.width).toBeLessThanOrEqual(page.viewportSize()!.width);
    });
});

test.describe('ranges, all and months', () => {
    test('Shift+click selects the range across a month boundary, Ctrl+A all loaded tiles', async ({ alicePage: page, isMobile }) => {
        test.skip(isMobile, 'Keyboard');
        const photos = timeline('alice');
        const firstMonth = photos.filter((p) => monthOf(p) === monthOf(photos[0])).length;
        const last = firstMonth + 1;
        await gotoGrid(page);

        await check(tile(page, photos[0].id)).click();
        await tile(page, photos[last].id).click({ modifiers: ['Shift'] });
        await expect(count(page)).toHaveText(`${last + 1} selected`);
        for (const photo of photos.slice(0, last + 1)) {
            await expect(tile(page, photo.id)).toHaveClass(/is-selected/);
        }
        await expect(tile(page, photos[last + 1].id)).not.toHaveClass(/is-selected/);
        // The viewer didn't open
        await expect(page.getByTestId('viewer')).toBeHidden();

        // The month header's circle reflects a fully selected month
        const header = page.locator(`#m-${monthOf(photos[0])}`).getByTestId('month-select');
        await expect(header).toHaveAttribute('aria-pressed', 'true');
        await expect(page.locator(`#m-${monthOf(photos[last])}`).getByTestId('month-select'))
            .toHaveAttribute('aria-pressed', 'false');

        await page.keyboard.press('Escape');
        await page.keyboard.press('Control+a');
        const loaded = await tiles(page).count();
        await expect(count(page)).toHaveText(`${loaded} selected`);
        await expect(header).toHaveAttribute('aria-pressed', 'true');
    });

    test('a range reaching unloaded tiles is resolved with the server', async ({ alicePage: page, isMobile }) => {
        test.skip(isMobile, 'Pointer');
        const photos = timeline('alice');
        await gotoGrid(page);
        const last = photos.length - 1;
        await check(tile(page, photos[0].id)).click();

        // The last photo's section is far below: scroll there, shift+click, and come back
        const resolved = page.waitForRequest((request) => request.url().includes('/api/context/neighbors')
            && request.url().includes('start='));
        await page.evaluate(() => window.scrollTo(0, document.body.scrollHeight));
        await expect(tileAt(page, last)).toBeVisible();
        await tileAt(page, last).click({ modifiers: ['Shift'] });
        await resolved;
        await expect(count(page)).toHaveText(`${last + 1} selected`);
        await expect(tileAt(page, last)).toHaveClass(/is-selected/);

        // Sections that were selected and unloaded again show the selection when they come back
        await page.evaluate(() => window.scrollTo(0, 0));
        await expect(tile(page, photos[0].id)).toHaveClass(/is-selected/);
        await expect(count(page)).toHaveText(`${last + 1} selected`);
    });

    test('the month circle selects the whole 250 photo month, even when only part of it is loaded', async ({ alicePage: page, isMobile }) => {
        test.skip(isMobile, 'Pointer');
        await gotoGrid(page);
        await page.evaluate(() => (document.querySelector('.photo-grid') as any).fpGrid.scrollToMonth('2022-03'));
        const section = page.locator('#m-2022-03');
        await expect(section).toHaveAttribute('data-loaded', 'true');
        const loadedTiles = await section.locator('.photo-tile').count();
        expect(loadedTiles).toBeLessThan(250);

        const ids = page.waitForRequest((request) => request.url().includes('/api/context/ids')
            && request.url().includes('month=2022-03'));
        await section.getByTestId('month-select').click();
        await ids;
        await expect(count(page)).toHaveText('250 selected');
        await expect(section.getByTestId('month-select')).toHaveAttribute('aria-pressed', 'true');

        // The selection doesn't depend on which tiles are loaded
        await page.evaluate(() => window.scrollTo(0, 0));
        await expect(count(page)).toHaveText('250 selected');
        await page.evaluate(() => (document.querySelector('.photo-grid') as any).fpGrid.scrollToMonth('2022-03'));
        await expect(section.getByTestId('month-select')).toHaveAttribute('aria-pressed', 'true');

        // Selecting again deselects the month
        await section.getByTestId('month-select').click();
        await expect(bar(page)).toBeHidden();
        await expect(section.getByTestId('month-select')).toHaveAttribute('aria-pressed', 'false');
    });
});

test.describe('drag-select', () => {
    test('dragging from a check circle selects the range, dragging back shrinks it', async ({ alicePage: page, isMobile }) => {
        test.skip(isMobile, 'Mouse');
        // Tall enough that the first rows are away from the auto-scroll zones
        await page.setViewportSize({ width: 1280, height: 1100 });
        await gotoGrid(page);
        const start = await center(check(tileAt(page, 2)));
        await page.mouse.move(start.x, start.y);
        await page.mouse.down();

        const end = await center(tileAt(page, 9));
        await page.mouse.move(end.x, end.y, { steps: 8 });
        await expect(count(page)).toHaveText('8 selected');
        for (let index = 2; index <= 9; index++) await expect(tileAt(page, index)).toHaveClass(/is-selected/);
        await expect(tileAt(page, 1)).not.toHaveClass(/is-selected/);
        await expect(tileAt(page, 10)).not.toHaveClass(/is-selected/);

        const back = await center(tileAt(page, 5));
        await page.mouse.move(back.x, back.y, { steps: 8 });
        await expect(count(page)).toHaveText('4 selected');
        await expect(tileAt(page, 6)).not.toHaveClass(/is-selected/);
        await page.mouse.up();
        await expect(count(page)).toHaveText('4 selected');
        // Releasing doesn't count as a click on the tile
        await expect(page.getByTestId('viewer')).toBeHidden();

        // A second drag adds to what was selected before it
        const next = await center(check(tileAt(page, 12)));
        await page.mouse.move(next.x, next.y);
        await page.mouse.down();
        const after = await center(tileAt(page, 14));
        await page.mouse.move(after.x, after.y, { steps: 5 });
        await page.mouse.up();
        await expect(count(page)).toHaveText('7 selected');
    });

    test('dragging near the bottom scrolls and selects into the next months', async ({ alicePage: page, isMobile }) => {
        test.skip(isMobile, 'Mouse');
        await gotoGrid(page);
        const first = tileAt(page, 0);
        const firstMonth = await first.evaluate((element) => element.closest('.month')!.getAttribute('data-month'));
        const start = await center(check(first));
        await page.mouse.move(start.x, start.y);
        await page.mouse.down();
        const { height } = page.viewportSize()!;
        await page.mouse.move(start.x + 20, height - 6, { steps: 6 });

        await expect.poll(() => selectedCount(page), { timeout: 15_000 }).toBeGreaterThan(60);
        expect(await page.evaluate(() => window.scrollY)).toBeGreaterThan(100);
        await page.mouse.up();
        // The last selected tile that is loaded belongs to a later month
        const lastMonth = await page.evaluate(() => {
            const selected = [...document.querySelectorAll('.photo-tile.is-selected')] as HTMLElement[];
            const last = selected.reduce((a, b) => Number(a.dataset.index) > Number(b.dataset.index) ? a : b);
            return last.closest('.month')!.getAttribute('data-month');
        });
        expect(lastMonth).not.toBe(firstMonth);
        const total = await selectedCount(page);
        // The scrolling stopped with the mouse
        await page.waitForTimeout(300);
        expect(await selectedCount(page)).toBe(total);
    });
});

test('keys of the grid behind a deep-linked viewer don\'t reach the grid', async ({ alicePage: page, isMobile }) => {
    test.skip(isMobile, 'Keyboard');
    const [first] = timeline('alice');
    await gotoReady(page, `/?photo=${first.id}`);
    await expect(page.getByTestId('viewer')).toBeVisible();
    await expect(page.getByTestId('photo-grid')).toHaveAttribute('data-ready', '');
    const zoom = await page.getByTestId('photo-grid').getAttribute('data-zoom');

    for (const key of ['+', 'Control+a', 'x', 'm', 'Delete']) await page.keyboard.press(key);
    await expect(bar(page)).toBeHidden();
    await expect(page.getByTestId('photo-grid')).toHaveAttribute('data-zoom', zoom!);
    await expect(page.getByTestId('photo-grid')).not.toHaveClass(/selecting/);
});

test.describe('permissions and the bar', () => {
    test('bob can neither delete nor move photos of a view-only shared folder', async ({ bobPage: page, isMobile }) => {
        test.skip(isMobile, 'Pointer');
        await gotoGrid(page, `/folders/${folderId('Holiday')}`);
        await check(tiles(page).first()).click();
        await expect(count(page)).toHaveText('1 selected');

        for (const action of ['delete', 'move']) {
            const button = page.getByTestId(`selection-${action}`);
            await expect(button).toHaveAttribute('aria-disabled', 'true');
            await expect(button).toBeDisabled();
            await expect(button).toHaveAttribute('title', `You can't ${action} some of the selected photos`);
        }
        // Pressing a disabled action says why and does nothing
        await page.getByTestId('selection-delete').click({ force: true });
        await expect(toast(page, "You can't delete some of the selected photos")).toBeVisible();
        await expect(page.getByTestId('delete-dialog')).toHaveCount(0);
        await page.keyboard.press('Delete');
        await expect(page.getByTestId('delete-dialog')).toHaveCount(0);
        await expect(page.getByTestId('selection-download')).not.toHaveAttribute('aria-disabled', 'true');
        await expect(page.getByTestId('selection-favorite')).not.toHaveAttribute('aria-disabled', 'true');
    });

    test('a mix of deletable and view-only photos disables the actions', async ({ bobPage: page, isMobile }) => {
        test.skip(isMobile, 'Pointer');
        const photos = timeline('bob');
        const own = photos.find((p) => p.userId === 'bob')!;
        const shared = photos.find((p) => p.folderId === folderId('Holiday'))!;
        await gotoGrid(page);
        await select(page, own.id);
        await expect(page.getByTestId('selection-delete')).not.toHaveAttribute('aria-disabled', 'true');
        await select(page, shared.id);
        await expect(page.getByTestId('selection-delete')).toHaveAttribute('aria-disabled', 'true');
        await select(page, shared.id);
        await expect(page.getByTestId('selection-delete')).not.toHaveAttribute('aria-disabled', 'true');
    });

    test('Download asks first above 20 photos, otherwise downloads each file', async ({ alicePage: page, isMobile }) => {
        test.skip(isMobile, 'Downloads');
        const [first, second] = timeline('alice');
        await gotoGrid(page);

        await page.keyboard.press('Control+a');
        expect(await selectedCount(page)).toBeGreaterThan(20);
        await page.getByTestId('selection-download').click();
        const dialog = page.getByTestId('app-dialog');
        await expect(dialog.getByRole('heading')).toHaveText(`Download ${await selectedCount(page)} files?`);
        await dialog.getByTestId('confirm-cancel').click();
        await expect(dialog).toBeHidden();
        await expect(bar(page)).toBeVisible();

        await page.keyboard.press('Escape');
        await select(page, first.id, second.id);
        const names: string[] = [];
        page.on('download', (download) => names.push(download.suggestedFilename()));
        await page.getByTestId('selection-download').click();
        await expect.poll(() => names.length).toBe(2);
        expect([...names].sort()).toEqual([first.name, second.name].sort());
        // Downloading keeps the selection
        await expect(count(page)).toHaveText('2 selected');
    });

    test('Share uses the Web Share API with the files, up to 10', async ({ alicePage: page, isMobile }) => {
        test.skip(isMobile, 'Stubbed API');
        await page.addInitScript(() => {
            (navigator as any).canShare = () => true;
            (navigator as any).share = async (data: { files: File[] }) => {
                (window as any).shared = data.files.map((file) => file.name);
            };
        });
        const photos = timeline('alice');
        await gotoGrid(page);
        await select(page, photos[0].id, photos[1].id);
        await page.getByTestId('selection-share').click();
        await expect.poll(() => page.evaluate(() => (window as any).shared)).toEqual([photos[0].name, photos[1].name]);

        await select(page, ...photos.slice(2, 11).map((p) => p.id));
        await expect(count(page)).toHaveText('11 selected');
        await page.getByTestId('selection-share').click();
        await expect(toast(page, 'Select up to 10 photos to share')).toBeVisible();
    });

    test('Share is hidden when the browser can not share files', async ({ alicePage: page, isMobile }) => {
        test.skip(isMobile, 'Stubbed API');
        await page.addInitScript(() => {
            (navigator as any).canShare = () => false;
        });
        await gotoGrid(page);
        await check(tiles(page).first()).click();
        await expect(page.getByTestId('selection-download')).toBeVisible();
        await expect(page.getByTestId('selection-share')).toBeHidden();
    });
});

test.describe('changing photos', () => {
    test.describe.configure({ mode: 'serial' });
    test.beforeEach(({ isMobile }) => {
        test.skip(isMobile, 'Mutates heidi\'s photos');
    });

    test('Delete moves the photos to the trash, Undo brings them back', async ({ page }) => {
        const [a, b, c] = await heidiPhotos(page, 2040);
        await gotoGrid(page);
        await select(page, a, b);

        await page.getByTestId('selection-delete').click();
        const dialog = page.getByTestId('delete-dialog');
        await expect(dialog.getByRole('heading')).toHaveText('Delete photos');
        await expect(dialog.locator('img')).toHaveCount(2);
        await dialog.getByTestId('delete-confirm').click();
        await expect(dialog).toBeHidden();

        const moved = toast(page, '2 photos moved to trash');
        await expect(moved).toBeVisible();
        await expect(tile(page, a)).toHaveCount(0);
        await expect(tile(page, b)).toHaveCount(0);
        await expect(tile(page, c)).toHaveCount(1);
        await expect(bar(page)).toBeHidden();
        for (const id of [a, b]) expect((await photoInfo(page, id)).trashed_on).not.toBeNull();
        expect((await photoInfo(page, c)).trashed_on).toBeNull();

        const restored = page.waitForResponse((response) => response.url().endsWith('/api/trash/restore'));
        await moved.getByRole('button', { name: 'Undo' }).click();
        expect((await restored).ok()).toBe(true);
        await expect(tile(page, a)).toHaveCount(1);
        await expect(tile(page, b)).toHaveCount(1);
        await expect(toast(page, '2 photos restored')).toBeVisible();
        for (const id of [a, b]) expect((await photoInfo(page, id)).trashed_on).toBeNull();
        // The restored tiles are not selected
        await expect(bar(page)).toBeHidden();
        await expect(tile(page, a)).not.toHaveClass(/is-selected/);
    });

    test('the Delete key asks, Cancel keeps everything', async ({ page }) => {
        const [a] = await heidiPhotos(page, 2041);
        await gotoGrid(page);
        await select(page, a);
        await page.keyboard.press('Delete');
        const dialog = page.getByTestId('delete-dialog');
        await expect(dialog.getByRole('heading')).toHaveText('Delete photo');
        await dialog.getByTestId('delete-cancel').click();
        await expect(dialog).toBeHidden();
        await expect(tile(page, a)).toHaveClass(/is-selected/);
        expect((await photoInfo(page, a)).trashed_on).toBeNull();
    });

    test('Move to a new folder creates it and the photos are there', async ({ page }) => {
        const [a, b, c] = await heidiPhotos(page, 2042);
        await gotoGrid(page);
        await select(page, a, b);
        await page.getByTestId('selection-move').click();

        const dialog = page.getByTestId('move-dialog');
        await expect(dialog).toBeVisible();
        await expect(dialog.locator('.move-showcase__thumbs img')).toHaveCount(2);
        await expect(dialog.getByText('2 photos', { exact: true })).toBeVisible();
        await expect(page.getByTestId('move-note')).toBeHidden();
        await page.getByTestId('dest-new').click();
        await page.getByTestId('folder-name-input').fill('E2E Moved');
        await page.getByTestId('folder-name-confirm').click();
        await expect(page.getByTestId('dest-selected-name')).toHaveText('E2E Moved');
        await page.getByTestId('move-confirm').click();
        await expect(dialog).toBeHidden();

        // One toast, from the action (the dialog shows none)
        await expect(toast(page, '2 photos moved')).toHaveCount(1);
        await expect(bar(page)).toBeHidden();
        const infoA = await photoInfo(page, a);
        const infoB = await photoInfo(page, b);
        expect(infoA.folder_id).not.toBeNull();
        expect(infoB.folder_id).toBe(infoA.folder_id);
        expect((await photoInfo(page, c)).folder_id).toBeNull();

        await gotoGrid(page, `/folders/${infoA.folder_id}`);
        await expect(tile(page, a)).toHaveCount(1);
        await expect(tile(page, b)).toHaveCount(1);
        await expect(tiles(page)).toHaveCount(2);
    });

    test('Move to the Family gallery makes the photos public and removes them from the personal timeline', async ({ page, bobPage }) => {
        const [a, b, c] = await heidiPhotos(page, 2043);
        const bobInfo = (id: number) => bobPage.request.get(`/api/photos/${id}/info`);
        expect((await bobInfo(a)).status()).toBe(404);

        await gotoGrid(page, '/?filter=personal');
        await select(page, a, b);
        await page.getByTestId('selection-move').click();
        await page.getByTestId('dest-family').click();
        await expect(page.getByTestId('move-note')).toHaveText('These photos will become visible to the whole family');
        await page.getByTestId('move-confirm').click();
        await expect(page.getByTestId('move-dialog')).toBeHidden();

        // Public photos show up in everybody's timeline, and other specs run at the same time:
        // trash them again as soon as bob's access is checked
        const bobSees = [(await bobInfo(a)).status(), (await bobInfo(b)).status(), (await bobInfo(c)).status()];
        expect((await page.request.post('/api/trash', { data: [a, b] })).ok()).toBe(true);
        // A public trashed photo is in everybody's trash: delete them for good
        for (const id of [a, b]) expect((await page.request.delete(`/api/delete/${id}`)).ok()).toBe(true);
        expect(bobSees).toEqual([200, 200, 404]);

        await expect(toast(page, '2 photos moved')).toBeVisible();
        await expect(tile(page, a)).toHaveCount(0);
        await expect(tile(page, b)).toHaveCount(0);
        await expect(tile(page, c)).toHaveCount(1);
    });

    test('the move dialog warns when public photos would become private', async ({ alicePage: page }) => {
        // Nothing is moved: the dialog is cancelled
        const publicPhoto = timeline('alice').find((p) => p.userId === null)!;
        const own = timeline('alice').find((p) => p.userId === 'alice')!;
        await gotoGrid(page);
        await select(page, publicPhoto.id);
        await page.getByTestId('selection-move').click();
        await page.getByTestId('dest-personal').click();
        await expect(page.getByTestId('move-note')).toHaveText('These photos will become private');
        await page.getByTestId('dest-family').click();
        await expect(page.getByTestId('move-note')).toBeHidden();
        await page.getByTestId('move-cancel').click();
        await expect(page.getByTestId('move-dialog')).toBeHidden();

        await page.keyboard.press('Escape');
        await select(page, own.id);
        await page.getByTestId('selection-move').click();
        await page.getByTestId('dest-personal').click();
        await expect(page.getByTestId('move-note')).toBeHidden();
        await page.getByTestId('move-cancel').click();
    });

    test('the move dialog shows errors and keeps the selection when it is cancelled', async ({ page }) => {
        const [a] = await heidiPhotos(page, 2044, ['a']);
        await gotoGrid(page);
        await select(page, a);
        await page.getByTestId('selection-move').click();
        // Moving to where the photo already is moves nothing
        await page.getByTestId('dest-personal').click();
        await page.getByTestId('move-confirm').click();
        await expect(page.getByTestId('move-error')).toHaveText('None of the photos could be moved');
        await expect(page.getByTestId('move-dialog')).toBeVisible();
        await page.getByTestId('move-cancel').click();
        await expect(page.getByTestId('move-dialog')).toBeHidden();
        await expect(toast(page, /moved/)).toHaveCount(0);
        await expect(tile(page, a)).toHaveClass(/is-selected/);
    });

    test('a failed favorite is reported and stays selected', async ({ page }) => {
        const [a, b] = await heidiPhotos(page, 2046);
        await gotoGrid(page);
        await select(page, a, b);
        await page.route(`**/api/favorite/${b}`, (route) => route.fulfill({ status: 500 }));
        await page.getByTestId('selection-favorite').click();
        await expect(toast(page, 'Added 1 of 2 photos to favorites')).toBeVisible();
        await expect(toast(page, 'Something went wrong')).toBeVisible();
        await expect(count(page)).toHaveText('2 selected');
    });

    test('Add to favorites favorites every selected photo', async ({ page }) => {
        const [a, b] = await heidiPhotos(page, 2045);
        await gotoGrid(page);
        await select(page, a, b);
        const requests: string[] = [];
        page.on('request', (request) => {
            if (request.url().includes('/api/favorite/') && request.method() === 'POST') requests.push(request.url());
        });
        await page.getByTestId('selection-favorite').click();
        await expect(toast(page, 'Added 2 photos to favorites')).toBeVisible();
        await expect(bar(page)).toBeHidden();
        expect(requests).toHaveLength(2);
        const favorites = await (await page.request.get('/api/favorite')).json();
        expect(favorites).toEqual(expect.arrayContaining([a, b]));
    });
});
