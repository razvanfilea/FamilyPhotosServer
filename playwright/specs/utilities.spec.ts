import type { Page } from '@playwright/test';
import { expect, gotoReady, loginAs, test } from '../fixtures/auth';
import { apiAs, uploadPhoto } from '../fixtures/library';
import { getSeed, photo } from '../harness/seed';
import { plural } from '../../assets/js/core/format.js';

const tile = (page: Page, id: number) => page.locator(`[data-testid="photo-tile"][data-photo-id="${id}"]`);

async function gotoGrid(page: Page, url: string) {
    await gotoReady(page, url);
    await expect(page.getByTestId('photo-grid')).toHaveAttribute('data-ready', '');
}

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

    // Other specs favourite carol's photos too, so only this test's own tiles are checked
    test('unfavouriting in the viewer removes the tile', async ({ page, playwright, baseURL, isMobile }) => {
        test.skip(isMobile, 'Mutates carol\'s favourites');
        await loginAs(page, 'carol');
        const carol = await apiAs(playwright, baseURL, 'carol');
        const time = Date.UTC(2035, 0, 1) / 1000;
        const first = await uploadPhoto(carol, 'carol-fav-a.jpg', time);
        const second = await uploadPhoto(carol, 'carol-fav-b.jpg', time - 86400);
        for (const id of [first, second]) {
            expect((await carol.post(`/api/favorite/${id}`)).ok()).toBe(true);
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
        await carol.dispose();
    });
});

// The selection bar on the grid pages. erin is used by no other spec (Mobile would race with
// Desktop on her favourites, so these run on desktop only).
test.describe('selection on the grid pages', () => {
    test.skip(({ isMobile }) => isMobile, 'Mutates erin\'s photos');

    const toast = (page: Page, text: string) => page.getByTestId('toast').filter({ hasText: text });

    async function select(page: Page, ...ids: number[]) {
        for (const id of ids) {
            await tile(page, id).scrollIntoViewIfNeeded();
            await tile(page, id).locator('.photo-tile__check').click();
        }
    }

    test('favorites: the bar removes the selected photos from the favorites', async ({ page, playwright, baseURL }) => {
        await loginAs(page, 'erin');
        const erin = await apiAs(playwright, baseURL, 'erin');
        const time = Date.UTC(2036, 0, 1) / 1000;
        const ids = [await uploadPhoto(erin, 'erin-fav-a.jpg', time), await uploadPhoto(erin, 'erin-fav-b.jpg', time - 86400)];
        try {
            for (const id of ids) expect((await erin.post(`/api/favorite/${id}`)).ok()).toBe(true);
            await gotoGrid(page, '/favorites');
            await select(page, ...ids);

            await expect(page.getByTestId('selection-favorite')).toHaveAttribute('aria-label', 'Remove from favorites');
            await page.getByTestId('selection-favorite').click();
            await expect(toast(page, 'Removed 2 photos from favorites')).toBeVisible();
            for (const id of ids) await expect(tile(page, id)).toHaveCount(0);
            await expect(page.getByTestId('selection-bar')).toBeHidden();
            expect(await (await erin.get('/api/favorite')).json()).not.toEqual(expect.arrayContaining([ids[0]]));
        } finally {
            for (const id of ids) {
                await erin.post('/api/trash', { data: [id] });
                await erin.delete(`/api/delete/${id}`);
            }
            await erin.dispose();
        }
    });

    test('large files: the bar offers the usual actions and Delete can be undone', async ({ page, playwright, baseURL }) => {
        await loginAs(page, 'erin');
        const erin = await apiAs(playwright, baseURL, 'erin');
        // Over 50 MiB: the fixture plus padding
        const id = await uploadPhoto(erin, 'erin-large.jpg', Date.UTC(2036, 0, 1) / 1000, undefined, 51 * 1024 * 1024);
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
            await erin.post('/api/trash', { data: [id] });
            await erin.delete(`/api/delete/${id}`);
            await erin.dispose();
        }
    });
});
