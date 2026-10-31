import type { Page } from '@playwright/test';
import { expect, gotoReady, loginAs, test } from '../fixtures/auth';
import { apiAs, uploadPhoto } from '../fixtures/library';
import { getSeed, photo } from '../harness/seed';
import { formatBytes, plural } from '../../assets/js/core/format.js';

const tile = (page: Page, id: number) => page.locator(`[data-testid="photo-tile"][data-photo-id="${id}"]`);

async function gotoGrid(page: Page, url: string) {
    await gotoReady(page, url);
    await expect(page.getByTestId('photo-grid')).toHaveAttribute('data-ready', '');
}

interface PhotoJson {
    user_id: string | null;
    name: string;
    file_size: number;
    trashed_on: number | null;
}

test.describe('utilities hub', () => {
    test('the statistics card matches the library', async ({ alicePage: page }) => {
        // Other specs trash and restore public photos at any time: compare the two reads again if they differ
        await expect(async () => {
            // What the card counts: alice's own and the public photos that aren't trashed
            const response = await page.request.get('/api/sync/full');
            const photos = ((await response.json()).photos as PhotoJson[]).filter((p) => p.trashed_on === null);
            const sum = (list: PhotoJson[]) => list.reduce((total, p) => total + p.file_size, 0);
            const label = (list: PhotoJson[]) => `${plural(list.length, 'photo')} • ${formatBytes(sum(list))}`;
            const isVideo = (p: PhotoJson) => p.name.endsWith('.mp4');

            const family = photos.filter((p) => p.user_id === null);
            const personal = photos.filter((p) => p.user_id === 'alice');
            const videos = photos.filter(isVideo);
            expect(family.length).toBeGreaterThanOrEqual(269);
            expect(personal.length).toBeGreaterThanOrEqual(16);

            await gotoReady(page, '/utilities');
            const card = page.getByTestId('stats-card');
            await expect(card).toContainText('Photo Statistics');
            await expect(page.getByTestId('stat-family').locator('dd')).toHaveText(label(family));
            await expect(page.getByTestId('stat-personal').locator('dd')).toHaveText(label(personal));
            await expect(page.getByTestId('stat-images').locator('dd')).toHaveText(label(photos.filter((p) => !isVideo(p))));
            await expect(page.getByTestId('stat-videos').locator('dd')).toHaveText(label(videos));
            await expect(page.getByTestId('stat-total')).toHaveText(`Total: ${label(photos)}`);
        }).toPass({ timeout: 20_000 });
    });

    test('each tool links to its page', async ({ alicePage: page }) => {
        const tools: [string, string][] = [
            ['favorites', '/favorites'],
            ['trash', '/trash'],
            ['duplicates', '/duplicates'],
            ['large-files', '/large-files'],
            ['upload', '/upload'],
            ['settings', '/settings'],
        ];
        for (const [name, path] of tools) {
            await gotoReady(page, '/utilities');
            await page.getByTestId(`tool-${name}`).click();
            await expect(page).toHaveURL(new RegExp(`${path}$`));
        }
    });

    test('the tool grid has 2 columns on phones and 4 on wide screens', async ({ alicePage: page, isMobile }) => {
        await gotoReady(page, '/utilities');
        const tops = await page.getByTestId(/^tool-/).evaluateAll((tools) =>
            tools.map((element) => Math.round(element.getBoundingClientRect().top)));
        const columns = tops.filter((top) => top === tops[0]).length;
        expect(columns).toBe(isMobile ? 2 : 4);
    });

    test('the nav highlights Utilities on its tool pages', async ({ alicePage: page, isMobile }) => {
        test.skip(!isMobile, 'The bottom nav is the mobile one');
        for (const path of ['/duplicates', '/settings', '/large-files']) {
            await gotoReady(page, path);
            await expect(page.getByTestId('bottom-nav-utilities')).toHaveAttribute('aria-current', 'page');
        }
    });
});

test.describe('favorites', () => {
    test('lists the favourites of the user', async ({ alicePage: page }) => {
        const favorites = getSeed().aliceFavorites.map(photo);
        expect(favorites.length).toBeGreaterThan(1);

        await gotoGrid(page, '/favorites');
        await expect(page.getByTestId('favorites-title')).toHaveText('Favorites');
        await expect(page.getByTestId('favorites-badge')).toContainText('Favorites');
        await expect(page.getByTestId('favorites-count')).toHaveText(plural(favorites.length, 'photo'));
        await expect(page.getByTestId('photo-tile')).toHaveCount(favorites.length);
        for (const favorite of favorites) {
            await expect(tile(page, favorite.id)).toBeVisible();
        }
        await expect(page.getByTestId('nav-favorites')).toHaveAttribute('aria-current', 'page');

        // The viewer walks the favorites
        await tile(page, favorites[0].id).click();
        await expect(page.getByTestId('viewer')).toHaveAttribute('data-photo-id', String(favorites[0].id));
        await expect(page).toHaveURL(new RegExp(`/favorites\\?photo=${favorites[0].id}$`));
    });

    test('shows an empty state without favourites', async ({ bobPage: page }) => {
        await gotoGrid(page, '/favorites');
        await expect(page.getByTestId('grid-empty')).toBeVisible();
        await expect(page.getByTestId('grid-empty')).toContainText('No favorites yet');
        await expect(page.getByTestId('grid-empty')).toContainText('Tap the star on a photo to add it here.');
        await expect(page.getByTestId('photo-tile')).toHaveCount(0);
    });

    test('unfavouriting in the viewer removes the tile', async ({ page, playwright, baseURL, isMobile }) => {
        test.skip(isMobile, 'Mutates grace\'s favourites');
        await loginAs(page, 'grace');
        const grace = await apiAs(playwright, baseURL, 'grace');
        const time = Date.UTC(2035, 0, 1) / 1000;
        const first = await uploadPhoto(grace, 'grace-fav-a.jpg', time);
        const second = await uploadPhoto(grace, 'grace-fav-b.jpg', time - 86400);
        for (const id of [first, second]) {
            expect((await grace.post(`/api/favorite/${id}`)).ok()).toBe(true);
        }

        await gotoGrid(page, '/favorites');
        await expect(tile(page, first)).toBeVisible();
        await expect(tile(page, second)).toBeVisible();

        await tile(page, first).click();
        await expect(page.getByTestId('viewer')).toHaveAttribute('data-photo-id', String(first));
        await page.getByTestId('viewer-favorite').click();
        await page.getByTestId('viewer-close').click();

        await expect(tile(page, first)).toHaveCount(0);
        await expect(tile(page, second)).toBeVisible();

        // It stays removed
        await gotoGrid(page, '/favorites');
        await expect(tile(page, first)).toHaveCount(0);
        await expect(tile(page, second)).toBeVisible();
        await grace.dispose();
    });
});

test.describe('large files', () => {
    test('shows an empty state when no file is over 50 MB', async ({ alicePage: page }) => {
        await gotoGrid(page, '/large-files');
        await expect(page.getByTestId('large-files-title')).toHaveText('Large Files');
        await expect(page.getByTestId('large-files-subtitle')).toHaveText('Files larger than 50 MB');
        await expect(page.getByTestId('grid-empty')).toBeVisible();
        await expect(page.getByTestId('grid-empty')).toContainText('No files larger than 50 MB');
        await expect(page.getByTestId('photo-tile')).toHaveCount(0);
    });
});

// The selection bar on the grid pages. hank is used by no other spec (Mobile would race with
// Desktop on her favourites, so these run on desktop only).
test.describe('selection on the grid pages', () => {
    test.skip(({ isMobile }) => isMobile, 'Mutates hank\'s photos');

    const toast = (page: Page, text: string) => page.getByTestId('toast').filter({ hasText: text });

    async function select(page: Page, ...ids: number[]) {
        for (const id of ids) {
            await tile(page, id).scrollIntoViewIfNeeded();
            await tile(page, id).locator('.photo-tile__check').click();
        }
    }

    test('favorites: the bar removes the selected photos from the favorites', async ({ page, playwright, baseURL }) => {
        await loginAs(page, 'hank');
        const hank = await apiAs(playwright, baseURL, 'hank');
        const time = Date.UTC(2036, 0, 1) / 1000;
        const ids = [await uploadPhoto(hank, 'hank-fav-a.jpg', time), await uploadPhoto(hank, 'hank-fav-b.jpg', time - 86400)];
        try {
            for (const id of ids) expect((await hank.post(`/api/favorite/${id}`)).ok()).toBe(true);
            await gotoGrid(page, '/favorites');
            await select(page, ...ids);

            await expect(page.getByTestId('selection-favorite')).toHaveAttribute('aria-label', 'Remove from favorites');
            await page.getByTestId('selection-favorite').click();
            await expect(toast(page, 'Removed 2 photos from favorites')).toBeVisible();
            for (const id of ids) await expect(tile(page, id)).toHaveCount(0);
            await expect(page.getByTestId('selection-bar')).toBeHidden();
            expect(await (await hank.get('/api/favorite')).json()).not.toEqual(expect.arrayContaining([ids[0]]));
        } finally {
            for (const id of ids) {
                await hank.post('/api/trash', { data: [id] });
                await hank.delete(`/api/delete/${id}`);
            }
            await hank.dispose();
        }
    });

    test('large files: the bar offers the usual actions and Delete can be undone', async ({ page, playwright, baseURL }) => {
        await loginAs(page, 'hank');
        const hank = await apiAs(playwright, baseURL, 'hank');
        // Over 50 MiB: the fixture plus padding
        const id = await uploadPhoto(hank, 'hank-large.jpg', Date.UTC(2036, 0, 1) / 1000, undefined, 51 * 1024 * 1024);
        try {
            await gotoGrid(page, '/large-files');
            await select(page, id);
            for (const action of ['favorite', 'download', 'move', 'delete']) {
                await expect(page.getByTestId(`selection-${action}`)).toBeVisible();
            }

            await page.getByTestId('selection-delete').click();
            await page.getByTestId('delete-confirm').click();
            const moved = toast(page, '1 photo moved to trash');
            await expect(moved).toBeVisible();
            await expect(tile(page, id)).toHaveCount(0);
            await expect(page.getByTestId('grid-empty')).toBeVisible();

            await moved.getByRole('button', { name: 'Undo' }).click();
            await expect(tile(page, id)).toBeVisible();
        } finally {
            await hank.post('/api/trash', { data: [id] });
            await hank.delete(`/api/delete/${id}`);
            await hank.dispose();
        }
    });
});
