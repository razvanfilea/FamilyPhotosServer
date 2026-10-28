import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import type { APIRequestContext, Page } from '@playwright/test';
import { expect, gotoReady, loginAs, test, type UserId } from '../fixtures/auth';
import { folderId, getSeed, photo, type SeedPhoto } from '../harness/seed';
import { formatDate } from '../../assets/js/core/format.js';
import { EXTEND_THRESHOLD, WINDOW_AFTER, WINDOW_BEFORE } from '../../assets/js/viewer/source.js';

/** alice's or bob's timeline (filter all), in order */
function timeline(user: UserId): SeedPhoto[] {
    const holiday = folderId('Holiday');
    return Object.values(getSeed().photos)
        .filter((p) => !p.trashed
            && (p.userId === user || p.userId === null || (user === 'bob' && p.folderId === holiday)))
        .sort((a, b) => b.createdAt - a.createdAt || b.id - a.id);
}

const viewer = (page: Page) => page.getByTestId('viewer');
const tile = (page: Page, id: number) => page.locator(`[data-testid="photo-tile"][data-photo-id="${id}"]`);
const photoUrl = (id: number) => new RegExp(`[?&]photo=${id}(&|$)`);

async function gotoGrid(page: Page, url = '/') {
    await gotoReady(page, url);
    await expect(page.getByTestId('photo-grid')).toHaveAttribute('data-ready', '');
}

/** Opens a deep link and waits until the viewer shows the photo */
async function gotoViewer(page: Page, url: string, id: number) {
    await gotoReady(page, url);
    await expectShowing(page, id);
}

async function expectShowing(page: Page, id: number) {
    await expect(viewer(page)).toBeVisible();
    await expect(viewer(page)).toHaveAttribute('data-photo-id', String(id));
}

async function expectPhoto(page: Page, expected: SeedPhoto) {
    await expectShowing(page, expected.id);
    await expect(page.getByTestId('viewer-title')).toHaveText(formatDate(expected.createdAt, 'viewer'));
    await expect(page).toHaveURL(photoUrl(expected.id));
}

/** The photos of one of the seed folders, in order */
function folderPhotos(name: string): SeedPhoto[] {
    return Object.values(getSeed().photos)
        .filter((p) => !p.trashed && p.folderId === folderId(name))
        .sort((a, b) => b.createdAt - a.createdAt || b.id - a.id);
}

/** Uploads fixture files as carol with the given creation times, returns their ids in order */
async function uploadAsCarol(
    request: APIRequestContext,
    files: { fixture: string, name: string, time: number }[],
    folder?: number,
) {
    const ids = [];
    for (const { fixture, name, time } of files) {
        // The server reuses a photo with the same content, so make each upload unique
        const buffer = Buffer.concat([
            readFileSync(join(__dirname, '../fixtures/photos', fixture)),
            Buffer.from(`${name} ${Date.now()}`),
        ]);
        const target = folder === undefined ? '' : `&folder_id=${folder}`;
        const response = await request.post(`/api/upload?time_created=${time}${target}`, {
            multipart: { file: { name, mimeType: 'image/jpeg', buffer } },
        });
        expect(response.ok(), await response.text()).toBe(true);
        ids.push((await response.json()).id as number);
    }
    return ids;
}

test.describe('opening and closing', () => {
    test('a tile click opens the photo, Back closes it, a reload opens it directly', async ({ alicePage: page }) => {
        const [first] = timeline('alice');
        await gotoGrid(page);

        await tile(page, first.id).click();
        await expectPhoto(page, first);
        await expect(viewer(page)).toHaveAttribute('role', 'dialog');
        await expect(viewer(page)).toHaveAttribute('aria-modal', 'true');

        await page.goBack();
        await expect(viewer(page)).toBeHidden();
        await expect(page).toHaveURL(/\/$/);
        await expect(tile(page, first.id)).toBeFocused();

        await gotoViewer(page, `/?photo=${first.id}`, first.id);
        await expect(page.getByTestId('viewer-title')).toHaveText(formatDate(first.createdAt, 'viewer'));
        // Opened by the URL: closing replaces it
        await page.getByTestId('viewer-close').click();
        await expect(viewer(page)).toBeHidden();
        await expect(page).toHaveURL(/\/$/);
    });

    test('Escape closes and focus returns to the tile', async ({ alicePage: page, isMobile }) => {
        test.skip(isMobile, 'Keyboard');
        const photos = timeline('alice');
        await gotoGrid(page);
        await tile(page, photos[2].id).focus();
        await page.keyboard.press('Enter');
        await expectPhoto(page, photos[2]);
        await expect(viewer(page)).toBeFocused();

        // The page underneath is inert, and the grid doesn't zoom under the viewer
        await expect(page.getByTestId('nav-rail')).toHaveAttribute('inert', '');
        const zoom = await page.getByTestId('photo-grid').getAttribute('data-zoom');
        await page.keyboard.press('Equal');
        await expect(page.getByTestId('photo-grid')).toHaveAttribute('data-zoom', zoom!);

        await page.keyboard.press('Escape');
        await expect(viewer(page)).toBeHidden();
        await expect(page).toHaveURL(/\/$/);
        await expect(tile(page, photos[2].id)).toBeFocused();
        await expect(page.getByTestId('nav-rail')).not.toHaveAttribute('inert', '');
    });

    test('an unknown photo shows a toast and closes', async ({ alicePage: page }) => {
        await gotoReady(page, '/?photo=999999');
        await expect(page.getByTestId('toast').filter({ hasText: 'Photo not found' })).toBeVisible();
        await expect(viewer(page)).toBeHidden();
        await expect(page).toHaveURL(/\/$/);
    });

    test('a memory opens in its own context, kept in the URL', async ({ alicePage: page }) => {
        const memory = photo('public/memory-2y.jpg');
        await gotoGrid(page);
        await page.getByTestId('memory-card').first().click();
        await expectShowing(page, memory.id);
        await expect(page).toHaveURL(/ctx=memories%3A2/);
        // A single photo: no neighbours
        await expect(page.getByTestId('viewer-next')).toBeHidden();
        await expect(page.getByTestId('viewer-prev')).toBeHidden();
    });
});

test.describe('navigation', () => {
    test('keys and buttons walk across a month boundary', async ({ alicePage: page, isMobile }) => {
        test.skip(isMobile, 'Keyboard and arrow buttons');
        const photos = timeline('alice');
        const month = (p: SeedPhoto) => new Date(p.createdAt * 1000).toISOString().slice(0, 7);
        const last = photos.findIndex((p, i) => month(p) !== month(photos[i + 1]));
        await gotoGrid(page);

        await tile(page, photos[0].id).click();
        await expectPhoto(page, photos[0]);
        await expect(page.getByTestId('viewer-prev')).toBeHidden();

        for (let i = 1; i <= last + 1; i++) {
            await page.keyboard.press('ArrowRight');
            await expectPhoto(page, photos[i]);
        }
        expect(month(photos[last + 1])).not.toBe(month(photos[last]));
        await page.keyboard.press('ArrowLeft');
        await expectPhoto(page, photos[last]);

        await page.getByTestId('viewer-next').click();
        await expectPhoto(page, photos[last + 1]);
        await page.getByTestId('viewer-prev').click();
        await expectPhoto(page, photos[last]);

        // Back still closes: navigating replaces the URL
        await page.goBack();
        await expect(viewer(page)).toBeHidden();
        await expect(page).toHaveURL(/\/$/);
    });

    test('a folder tile walks only the folder, Back restores the folder page', async ({ alicePage: page, isMobile }) => {
        test.skip(isMobile, 'Keyboard');
        const folder = folderId('Holiday');
        const photos = folderPhotos('Holiday');
        expect(photos.length).toBeGreaterThan(2);
        const contexts = new Set<string | null>();
        page.on('request', (request) => {
            const url = new URL(request.url());
            if (url.pathname === '/api/context/neighbors') contexts.add(url.searchParams.get('ctx'));
        });
        await gotoGrid(page, `/folders/${folder}`);

        await tile(page, photos[0].id).click();
        await expectPhoto(page, photos[0]);
        await expect(page).not.toHaveURL(/ctx=/);
        await expect(page.getByTestId('viewer-prev')).toBeHidden();
        for (const expected of photos.slice(1)) {
            await page.keyboard.press('ArrowRight');
            await expectPhoto(page, expected);
        }
        await expect(page.getByTestId('viewer-next')).toBeHidden();
        await page.keyboard.press('ArrowRight');
        await expectShowing(page, photos.at(-1)!.id);
        expect([...contexts]).toEqual([`folder:${folder}`]);

        await page.goBack();
        await expect(viewer(page)).toBeHidden();
        await expect(page).toHaveURL(new RegExp(`/folders/${folder}$`));
        await expect(tile(page, photos.at(-1)!.id)).toBeFocused();
    });

    test('Escape closes the menu, then the info sheet, then the viewer', async ({ alicePage: page, isMobile }) => {
        test.skip(isMobile, 'Keyboard');
        // Narrow: the info panel is a bottom sheet
        await page.setViewportSize({ width: 800, height: 720 });
        const photos = timeline('alice');
        await gotoViewer(page, `/?photo=${photos[0].id}`, photos[0].id);
        const panel = page.getByTestId('viewer-info-panel');
        const menu = page.getByTestId('viewer-more');
        await page.keyboard.press('i');
        await expect(panel).toBeVisible();
        await menu.locator('summary').click();
        await expect(menu).toHaveAttribute('open', '');

        // The arrows don't navigate under the open menu
        await page.keyboard.press('ArrowRight');
        await expectShowing(page, photos[0].id);

        await page.keyboard.press('Escape');
        await expect(menu).not.toHaveAttribute('open', '');
        await expect(panel).toBeVisible();
        await page.keyboard.press('Escape');
        await expect(panel).toBeHidden();
        await expect(viewer(page)).toBeVisible();
        await page.keyboard.press('Escape');
        await expect(viewer(page)).toBeHidden();
    });

    test('walks beyond the loaded window to the end of the context', async ({ alicePage: page, isMobile }) => {
        test.skip(isMobile, 'Keyboard');
        const photos = timeline('alice');
        // Far enough from the end that the first window doesn't reach it
        const start = photos.length - (WINDOW_AFTER + EXTEND_THRESHOLD + 6);
        expect(start).toBeGreaterThan(WINDOW_BEFORE);
        await gotoViewer(page, `/?photo=${photos[start].id}`, photos[start].id);

        const extended = page.waitForRequest(/\/api\/context\/neighbors\?.*start=/);
        for (let i = start + 1; i < photos.length; i++) {
            await page.keyboard.press('ArrowRight');
            await expectShowing(page, photos[i].id);
        }
        await extended;
        await expectPhoto(page, photos.at(-1)!);
        await expect(page.getByTestId('viewer-next')).toBeHidden();
        await page.keyboard.press('ArrowRight');
        await expectShowing(page, photos.at(-1)!.id);
    });

    test('swipe left shows the next photo, swipe down closes', async ({ alicePage: page, isMobile }) => {
        test.skip(!isMobile, 'Touch');
        const photos = timeline('alice');
        await gotoViewer(page, `/?photo=${photos[0].id}`, photos[0].id);
        const cdp = await page.context().newCDPSession(page);
        const touch = (type: string, x?: number, y?: number) => cdp.send('Input.dispatchTouchEvent', {
            type, touchPoints: x === undefined ? [] : [{ x, y: y!, id: 1 }],
        });
        const { width, height } = page.viewportSize()!;

        await touch('touchStart', width * 0.8, height / 2);
        for (let step = 1; step <= 6; step++) await touch('touchMove', width * (0.8 - step * 0.1), height / 2);
        await touch('touchEnd');
        await expectPhoto(page, photos[1]);

        await touch('touchStart', width / 2, height * 0.3);
        for (let step = 1; step <= 6; step++) await touch('touchMove', width / 2, height * 0.3 + step * 40);
        await touch('touchEnd');
        await expect(viewer(page)).toBeHidden();
        await expect(page).toHaveURL(/\/$/);
    });
});

test.describe('actions', () => {
    test.describe.configure({ mode: 'serial' });

    test('favourite toggles the star and persists', async ({ page, isMobile }) => {
        test.skip(isMobile, 'Mutates carol\'s photos');
        await loginAs(page, 'carol');
        const [id] = await uploadAsCarol(page.request, [
            { fixture: 'public/kite.jpg', name: 'carol-favorite.jpg', time: Date.UTC(2031, 0, 1) / 1000 },
        ]);
        await gotoViewer(page, `/?photo=${id}`, id);
        const star = page.getByTestId('viewer-favorite');
        await expect(star).toHaveAttribute('aria-pressed', 'false');

        const favorited = page.waitForResponse((r) => r.url().endsWith(`/api/favorite/${id}`) && r.request().method() === 'POST');
        await star.click();
        expect((await favorited).ok()).toBe(true);
        await expect(star).toHaveAttribute('aria-pressed', 'true');
        await expect(star).toHaveAccessibleName('Remove from favorites');

        await page.reload();
        await expectShowing(page, id);
        await expect(star).toHaveAttribute('aria-pressed', 'true');

        await page.keyboard.press('f');
        await expect(star).toHaveAttribute('aria-pressed', 'false');
        await page.reload();
        await expectShowing(page, id);
        await expect(star).toHaveAttribute('aria-pressed', 'false');
    });

    test('shortcuts don\'t fire with Ctrl held', async ({ page, isMobile }) => {
        test.skip(isMobile, 'Keyboard');
        await loginAs(page, 'carol');
        const [id] = await uploadAsCarol(page.request, [
            { fixture: 'public/snow.jpg', name: 'carol-ctrl.jpg', time: Date.UTC(2033, 0, 1) / 1000 },
        ]);
        let favoriteRequests = 0;
        page.on('request', (request) => {
            if (request.url().includes('/api/favorite/')) favoriteRequests += 1;
        });
        await gotoViewer(page, `/?photo=${id}`, id);

        await page.keyboard.press('Control+f');
        await page.keyboard.press('Control+i');
        await page.keyboard.press('Control+d');
        await expect(page.getByTestId('viewer-favorite')).toHaveAttribute('aria-pressed', 'false');
        await expect(page.getByTestId('viewer-info-panel')).toBeHidden();
        // Without Ctrl the same key works
        await page.keyboard.press('i');
        await expect(page.getByTestId('viewer-info-panel')).toBeVisible();
        expect(favoriteRequests).toBe(0);
    });

    test('trash moves to the next photo and removes the tile, Undo restores it', async ({ page, isMobile }) => {
        test.skip(isMobile, 'Mutates carol\'s photos');
        await loginAs(page, 'carol');
        const base = Date.UTC(2032, 0, 10) / 1000;
        const [a, b, c] = await uploadAsCarol(page.request, [
            { fixture: 'public/garden.jpg', name: 'carol-a.jpg', time: base },
            { fixture: 'public/picnic.jpg', name: 'carol-b.jpg', time: base - 86400 },
            { fixture: 'public/bikes.jpg', name: 'carol-c.jpg', time: base - 2 * 86400 },
        ]);
        await gotoGrid(page);
        await tile(page, b).click();
        await expectShowing(page, b);

        await page.getByTestId('viewer-delete').click();
        const dialog = page.getByTestId('delete-dialog');
        await expect(dialog).toBeVisible();
        await expect(dialog.getByRole('heading')).toHaveText('Delete photo');
        await expect(dialog.locator('img')).toHaveCount(1);
        await dialog.getByTestId('delete-confirm').click();
        await expect(dialog).toBeHidden();

        const toast = page.getByTestId('toast').filter({ hasText: 'Photo moved to trash' });
        await expect(toast).toBeVisible();
        await expectShowing(page, c);
        await expect(tile(page, b)).toHaveCount(0);
        await expect(tile(page, a)).toHaveCount(1);

        const restored = page.waitForResponse((r) => r.url().endsWith('/api/trash/restore'));
        // After the restore, the viewer reloads its window around the current photo
        const reloaded = page.waitForResponse((r) => r.url().includes('/api/context/neighbors')
            && new URL(r.url()).searchParams.get('photo_id') === String(c));
        await toast.getByRole('button', { name: 'Undo' }).click();
        expect((await restored).ok()).toBe(true);
        expect((await reloaded).ok()).toBe(true);
        const info = await (await page.request.get(`/api/photos/${b}/info`)).json();
        expect(info.photo.trashed_on).toBeNull();
        // Back in the grid and in the viewer's context
        await expect(tile(page, b)).toHaveCount(1);
        await page.keyboard.press('ArrowLeft');
        await expectShowing(page, b);
    });

    test('Undo after trashing the only photo of a folder brings it back', async ({ page, isMobile }) => {
        test.skip(isMobile, 'Mutates carol\'s photos');
        await loginAs(page, 'carol');
        const created = await page.request.post('/api/folders', {
            data: { name: `carol-undo-${Date.now()}`, is_public: false },
        });
        expect(created.ok()).toBe(true);
        const folder = (await created.json()).id as number;
        const [id] = await uploadAsCarol(page.request, [
            { fixture: 'public/lake.jpg', name: 'carol-only.jpg', time: Date.UTC(2034, 0, 1) / 1000 },
        ], folder);
        await gotoGrid(page, `/folders/${folder}`);
        const count = page.getByTestId('folder-count');
        await expect(count).toHaveText('1 photo');

        await tile(page, id).click();
        await expectShowing(page, id);
        await page.getByTestId('viewer-delete').click();
        await page.getByTestId('delete-dialog').getByTestId('delete-confirm').click();
        // The last photo: the viewer closes on the empty folder
        await expect(viewer(page)).toBeHidden();
        await expect(page.getByTestId('grid-empty')).toBeVisible();
        await expect(count).toHaveText('0 photos');

        await page.getByTestId('toast').filter({ hasText: 'Photo moved to trash' })
            .getByRole('button', { name: 'Undo' }).click();
        await expect(tile(page, id)).toHaveCount(1);
        await expect(page.getByTestId('grid-empty')).toBeHidden();
        await expect(count).toHaveText('1 photo');
        // The rebuilt grid works
        await tile(page, id).click();
        await expectShowing(page, id);
    });

    test('Delete and Move are hidden on a view-only shared folder', async ({ bobPage: page }) => {
        const shared = photo('alice/Holiday/holiday-a.jpg');
        await gotoViewer(page, `/?photo=${shared.id}`, shared.id);
        await expect(page.getByTestId('viewer-delete')).toBeHidden();
        await expect(page.getByTestId('viewer-move')).toBeHidden();
        await expect(page.getByTestId('viewer-share')).toBeVisible();
        await expect(page.getByTestId('viewer-info')).toBeVisible();

        const own = photo('bob/bob-a.jpg');
        await gotoViewer(page, `/?photo=${own.id}`, own.id);
        await expect(page.getByTestId('viewer-delete')).toBeVisible();
        await expect(page.getByTestId('viewer-share')).toBeVisible();
    });

    test('the trash context offers Restore and Delete forever', async ({ alicePage: page }) => {
        const trashed = photo('alice/alice-f.jpg');
        await gotoViewer(page, `/trash?photo=${trashed.id}&ctx=trash`, trashed.id);
        await expect(page.getByTestId('viewer-restore')).toBeVisible();
        await expect(page.getByTestId('viewer-delete-forever')).toBeVisible();
        await expect(page.getByTestId('viewer-info')).toBeVisible();
        for (const hidden of ['viewer-delete', 'viewer-share', 'viewer-move', 'viewer-favorite']) {
            await expect(page.getByTestId(hidden)).toBeHidden();
        }

        await page.getByTestId('viewer-delete-forever').click();
        const dialog = page.getByTestId('delete-dialog');
        await expect(dialog.getByRole('heading')).toHaveText('Delete permanently');
        await expect(dialog).toContainText('cannot be recovered');
        await dialog.getByTestId('delete-cancel').click();
        await expect(dialog).toBeHidden();
        await expectShowing(page, trashed.id);
    });

    test('share copies the link when files can\'t be shared', async ({ alicePage: page, isMobile }) => {
        test.skip(isMobile, 'Clipboard permissions');
        await page.context().grantPermissions(['clipboard-read', 'clipboard-write']);
        const [first] = timeline('alice');
        await gotoViewer(page, `/?photo=${first.id}`, first.id);
        await page.getByTestId('viewer-share').click();
        await expect(page.getByTestId('toast').filter({ hasText: 'Link copied' })).toBeVisible();
        const copied = await page.evaluate(() => navigator.clipboard.readText());
        expect(new URL(copied).search).toBe(`?photo=${first.id}`);
    });
});

test.describe('info panel', () => {
    test('shows the name, resolution and folder, and follows the navigation', async ({ alicePage: page }) => {
        const trip = folderPhotos('Family Trip');
        // A plain deep link: the folder page names its own context
        await gotoViewer(page, `/folders/${folderId('Family Trip')}?photo=${trip[0].id}`, trip[0].id);

        await page.getByTestId('viewer-info').click();
        const panel = page.getByTestId('viewer-info-panel');
        await expect(panel).toBeVisible();
        for (const [index, expected] of [[0, trip[0]], [1, trip[1]]] as const) {
            const { details } = await (await page.request.get(`/api/photos/${expected.id}/info`)).json();
            await expect(panel.getByTestId('viewer-info-name')).toHaveText(expected.name);
            await expect(panel.getByTestId('viewer-info-date')).toHaveText(formatDate(expected.createdAt, 'long'));
            await expect(panel.getByTestId('viewer-info-type')).toContainText('Image (JPG)');
            await expect(panel.getByTestId('viewer-info-type')).toContainText(`${details.width} × ${details.height}`);
            await expect(panel.getByTestId('viewer-info-folder').getByRole('link', { name: 'Family Trip' }))
                .toHaveAttribute('href', `/folders/${folderId('Family Trip')}`);
            await expect(panel.getByTestId('viewer-info-visibility')).toContainText('Family');
            if (index === 0) {
                // Navigating keeps the panel open on the new photo
                await page.keyboard.press('ArrowRight');
                await expectShowing(page, trip[1].id);
            }
        }

        await page.getByTestId('viewer-info-close').click();
        await expect(panel).toBeHidden();
    });
});

test.describe('video', () => {
    test('plays, and the ±5 s buttons seek', async ({ bobPage: page }) => {
        const video = photo('bob/VID_20240610_120000.mp4');
        await gotoViewer(page, `/?photo=${video.id}`, video.id);
        const element = page.getByTestId('viewer-video');
        await expect(element).toHaveAttribute('src', `/api/download/${video.id}?inline=1`);
        const time = () => element.evaluate((v: HTMLVideoElement) => v.currentTime);

        await page.getByTestId('video-play').click();
        await expect.poll(time).toBeGreaterThan(0);
        await page.getByTestId('video-play').click();
        await expect.poll(() => element.evaluate((v: HTMLVideoElement) => v.paused)).toBe(true);
        await expect(page.getByTestId('video-time')).toHaveText(/^00:0\d \/ 00:02$/);

        await page.getByTestId('video-forward').click();
        await expect.poll(time).toBeCloseTo(2, 0);
        await page.getByTestId('video-back').click();
        await expect.poll(time).toBe(0);

        await page.getByTestId('video-mute').click();
        await expect.poll(() => element.evaluate((v: HTMLVideoElement) => v.muted)).toBe(true);
    });
});
