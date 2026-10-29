import type { Locator, Page } from '@playwright/test';
import { expect, gotoReady, test, type UserId } from '../fixtures/auth';
import { folderId, getSeed, type SeedPhoto } from '../harness/seed';

type Filter = 'all' | 'personal' | 'family' | 'shared';

const COLUMNS = {
    375: [3, 4, 5, 7],
    900: [4, 6, 8, 10],
    1440: [5, 8, 10, 13],
};

/** The photos the timeline of `user` shows, in order */
function timeline(user: UserId, filter: Filter): SeedPhoto[] {
    const sharedFolder = folderId('Holiday');
    const matches = (photo: SeedPhoto) => {
        const personal = photo.userId === user;
        const family = photo.userId === null;
        // Alice shares Holiday with bob (view only)
        const shared = user === 'bob' && photo.folderId === sharedFolder;
        switch (filter) {
            case 'personal': return personal;
            case 'family': return family;
            case 'shared': return shared;
            default: return personal || family || shared;
        }
    };
    return Object.values(getSeed().photos)
        .filter((photo) => !photo.trashed && matches(photo))
        .sort((a, b) => b.createdAt - a.createdAt || b.id - a.id);
}

const monthOf = (photo: SeedPhoto) => new Date(photo.createdAt * 1000).toISOString().slice(0, 7);

/** Months newest first with their photos */
function months(photos: SeedPhoto[]) {
    const byMonth = new Map<string, SeedPhoto[]>();
    for (const photo of photos) {
        byMonth.set(monthOf(photo), [...(byMonth.get(monthOf(photo)) ?? []), photo]);
    }
    return [...byMonth.entries()].map(([month, list]) => ({ month, photos: list }));
}

const MONTH_NAMES = ['January', 'February', 'March', 'April', 'May', 'June', 'July', 'August', 'September',
    'October', 'November', 'December'];

function monthLabel(month: string, withYear: boolean) {
    const [year, number] = month.split('-').map(Number);
    const name = MONTH_NAMES[number - 1];
    return withYear || year !== new Date().getUTCFullYear() ? `${name} ${year}` : name;
}

/** Waits for the page's modules and the grid, which load after app.js */
async function gotoGrid(page: Page, url = '/') {
    await gotoReady(page, url);
    await expect(page.getByTestId('photo-grid')).toHaveAttribute('data-ready', '');
}

const grid = (page: Page) => page.getByTestId('photo-grid');
const sections = (page: Page) => page.getByTestId('month-section');
const tiles = (page: Page) => page.getByTestId('photo-tile');

/** Waits until the part of the grid in the viewport is filled with tiles. */
async function expectViewportFilled(page: Page) {
    await expect.poll(() => page.evaluate(() => {
        const root = document.querySelector('.photo-grid')!;
        const cols = parseInt(getComputedStyle(root).getPropertyValue('--cols'), 10);
        let missing = 0;
        for (const section of root.querySelectorAll(':scope > .month')) {
            const grid = section.querySelector('.month__grid')!;
            const rect = grid.getBoundingClientRect();
            if (rect.bottom < 0 || rect.top > window.innerHeight) continue;
            const cell = (grid.clientWidth - (cols - 1) * 2) / cols;
            const lastVisibleY = Math.min(window.innerHeight, rect.bottom) - 1 - rect.top;
            const neededTiles = Math.min(Number((section as HTMLElement).dataset.count),
                (Math.floor(lastVisibleY / (cell + 2)) + 1) * cols);
            if (grid.querySelectorAll('.photo-tile').length < neededTiles) missing += 1;
        }
        return missing;
    })).toBe(0);
}

/** The bounding box of an element once it stopped moving (e.g. the fast scroller sliding in) */
async function settledBox(locator: Locator) {
    let previous = JSON.stringify(await locator.boundingBox());
    await expect.poll(async () => {
        const current = JSON.stringify(await locator.boundingBox());
        const settled = current === previous;
        previous = current;
        return settled;
    }).toBe(true);
    return (await locator.boundingBox())!;
}

async function scrollTo(page: Page, y: number) {
    await page.evaluate((top) => window.scrollTo({ top, behavior: 'instant' }), y);
}

/** Scrolls through the whole page in steps, collecting every tile that was in the DOM. */
async function collectTiles(page: Page) {
    const seen = new Map<number, number>();
    const height = () => page.evaluate(() => document.documentElement.scrollHeight);
    const viewport = page.viewportSize()!.height;

    for (let y = 0; ; y += viewport / 2) {
        await scrollTo(page, y);
        await expectViewportFilled(page);
        const found = await page.evaluate(() =>
            [...document.querySelectorAll<HTMLElement>('.photo-tile')]
                .map((tile) => [Number(tile.dataset.photoId), Number(tile.dataset.index)]));
        for (const [id, index] of found) {
            expect(seen.get(id) ?? index, `index of photo ${id}`).toBe(index);
            seen.set(id, index);
        }
        if (y > (await height()) - viewport) break;
    }
    return seen;
}

test.describe('sections', () => {
    test('render a section per month with its label and count', async ({ alicePage: page }) => {
        await gotoGrid(page);

        const expected = months(timeline('alice', 'all'));
        await expect(sections(page)).toHaveCount(expected.length);
        expect(expected.length).toBeGreaterThan(10);

        for (const [i, { month, photos }] of expected.entries()) {
            const section = sections(page).nth(i);
            await expect(section).toHaveAttribute('data-month', month);
            await expect(section).toHaveAttribute('data-count', String(photos.length));
            await expect(section.getByTestId('month-title')).toHaveText(monthLabel(month, false));
        }

        // Months of other years than the current one say the year, the current year doesn't
        await expect(page.locator(`[data-month="2024-06"] [data-testid="month-title"]`)).toHaveText('June 2024');
        expect(monthLabel(`${new Date().getUTCFullYear()}-03`, false)).toBe('March');
        await expect(grid(page)).toHaveAttribute('data-total', String(timeline('alice', 'all').length));
    });

    test('every photo loads once, in context order, including equal timestamps', async ({ alicePage: page }) => {
        await gotoGrid(page);
        const expected = timeline('alice', 'all');

        const seen = await collectTiles(page);

        expect(seen.size).toBe(expected.length);
        for (const [position, photo] of expected.entries()) {
            expect(seen.get(photo.id), photo.path).toBe(position);
        }
        // The two photos with the same timestamp are ordered by id
        const [tieA, tieB] = ['public/tie-a.jpg', 'public/tie-b.jpg'].map((path) => getSeed().photos[path]);
        expect(tieA.createdAt).toBe(tieB.createdAt);
        expect(seen.get(Math.max(tieA.id, tieB.id))).toBeLessThan(seen.get(Math.min(tieA.id, tieB.id))!);
    });

    for (const zoom of [0, 1, 2, 3]) {
        test(`reserved heights stay the same when tiles load (zoom ${zoom})`, async ({ alicePage: page, baseURL }) => {
            await page.context().addCookies([{ name: 'fp_prefs', value: `zoom=${zoom}`, url: baseURL! }]);
            await gotoGrid(page);

            const measure = () => page.evaluate(() => ({
                total: document.documentElement.scrollHeight,
                sections: [...document.querySelectorAll<HTMLElement>('.photo-grid > .month')].map((section) => ({
                    month: section.dataset.month,
                    loaded: section.dataset.loaded === 'true',
                    height: section.getBoundingClientRect().height,
                })),
            }));

            const before = await measure();
            expect(before.sections.some((section) => !section.loaded)).toBe(true);

            await collectTiles(page);
            const during = await measure();
            expect(Math.abs(during.total - before.total)).toBeLessThanOrEqual(1);
            for (const [i, section] of during.sections.entries()) {
                expect(Math.abs(section.height - before.sections[i].height), section.month).toBeLessThanOrEqual(1);
            }
        });
    }

    test('only the sections near the viewport keep tiles', async ({ alicePage: page }) => {
        await page.setViewportSize({ width: 1280, height: 500 });
        await gotoGrid(page);
        const total = timeline('alice', 'all').length;

        const loadedSections = () => page.locator('[data-testid="month-section"][data-loaded="true"]');
        await expect(loadedSections().first()).toHaveAttribute('data-month', months(timeline('alice', 'all'))[0].month);

        await scrollTo(page, 1e6);
        await expectViewportFilled(page);
        // The newest months were dropped again; the DOM stays small
        await expect(sections(page).first()).toHaveAttribute('data-loaded', 'false');
        await expect(sections(page).first().getByTestId('photo-tile')).toHaveCount(0);
        // Tiles are only kept within 6 viewports above and below the viewport (grid.js
        // UNLOAD_MARGIN), plus the rest of the partially covered sections at both ends
        const bound = await page.evaluate(() => {
            const root = document.querySelector<HTMLElement>('.photo-grid')!;
            const style = getComputedStyle(root);
            const cols = parseInt(style.getPropertyValue('--cols'), 10);
            const gap = parseFloat(style.getPropertyValue('--gap'));
            const cell = (root.clientWidth - (cols - 1) * gap) / cols;
            const rowsPerViewport = Math.ceil(window.innerHeight / (cell + gap));
            const loaded = root.querySelectorAll(':scope > .month[data-loaded="true"]').length;
            return { tiles: (6 + 1 + 6) * rowsPerViewport * cols + 2 * loaded * cols, rowsPerViewport, cols };
        });
        const inDom = await tiles(page).count();
        expect(inDom).toBeLessThan(total);
        expect(inDom, JSON.stringify(bound)).toBeLessThanOrEqual(bound.tiles);
        // Every section holding tiles overlaps that window
        const outside = await page.evaluate(() => [...document.querySelectorAll<HTMLElement>('.month[data-loaded="true"]')]
            .filter((section) => {
                const rect = section.getBoundingClientRect();
                return rect.bottom < -6 * window.innerHeight || rect.top > 7 * window.innerHeight;
            }).map((section) => section.dataset.month));
        expect(outside).toEqual([]);

        await scrollTo(page, 0);
        await expectViewportFilled(page);
        await expect(sections(page).first()).toHaveAttribute('data-loaded', 'true');
    });

    test('a photo is found through its index even if its section was never loaded', async ({ alicePage: page }) => {
        await gotoGrid(page);
        const expected = timeline('alice', 'all');
        const target = expected[expected.length - 3];

        const found = await page.evaluate(([photoId, index]) =>
            document.querySelector<any>('.photo-grid').fpGrid.scrollToPhoto(photoId, index), [target.id, expected.length - 3]);

        expect(found).toBe(true);
        const tile = page.locator(`[data-photo-id="${target.id}"]`);
        await expect(tile).toBeInViewport();
        await expect(tile).toBeFocused();
    });
});

test.describe('zoom', () => {
    const columns = (page: Page) => page.evaluate(() =>
        parseInt(getComputedStyle(document.querySelector('.photo-grid')!).getPropertyValue('--cols'), 10));

    for (const [width, levels] of Object.entries(COLUMNS)) {
        test(`columns follow the zoom levels at ${width} px`, async ({ alicePage: page, isMobile }) => {
            test.skip(isMobile, 'Viewport widths are set explicitly on the desktop project');
            await page.setViewportSize({ width: Number(width), height: 800 });
            await gotoGrid(page);

            // Default level 1, then through all the levels with the keyboard
            expect(await columns(page)).toBe(levels[1]);
            await expect(grid(page)).toHaveAttribute('data-zoom', '1');
            await page.keyboard.press('-');
            await expect(grid(page)).toHaveAttribute('data-zoom', '2');
            expect(await columns(page)).toBe(levels[2]);
            await page.keyboard.press('-');
            await page.keyboard.press('-');
            await expect(grid(page)).toHaveAttribute('data-zoom', '3');
            expect(await columns(page)).toBe(levels[3]);
            for (let i = 0; i < 3; i++) await page.keyboard.press('+');
            await expect(grid(page)).toHaveAttribute('data-zoom', '0');
            expect(await columns(page)).toBe(levels[0]);
        });
    }

    test('the top bar buttons change the level and stop at the limits', async ({ alicePage: page, isMobile }) => {
        test.skip(isMobile, 'The zoom buttons are shown from 768 px');
        await gotoGrid(page);
        const zoomIn = page.getByTestId('zoom-in');
        const zoomOut = page.getByTestId('zoom-out');

        await zoomIn.click();
        await expect(grid(page)).toHaveAttribute('data-zoom', '0');
        await expect(zoomIn).toBeDisabled();
        await expect(zoomOut).toBeEnabled();
        expect(await columns(page)).toBe(COLUMNS[1440][0]);

        for (let i = 0; i < 3; i++) await zoomOut.click();
        await expect(grid(page)).toHaveAttribute('data-zoom', '3');
        await expect(zoomOut).toBeDisabled();
        await expect(zoomIn).toBeEnabled();
    });

    test('the level is remembered across reloads', async ({ alicePage: page }) => {
        await gotoGrid(page);
        await page.keyboard.press('-');
        await expect(grid(page)).toHaveAttribute('data-zoom', '2');
        expect(await page.evaluate(() => document.cookie)).toContain('zoom=2');

        await page.reload();
        await expect(grid(page)).toHaveAttribute('data-zoom', '2');
        // The server rendered it, so there is no flash of the default level
        const html = await (await page.request.get('/')).text();
        expect(html).toContain('data-zoom="2"');
    });

    test('ctrl+wheel zooms the grid instead of the page', async ({ alicePage: page, isMobile }) => {
        test.skip(isMobile, 'Mouse wheel');
        await gotoGrid(page);
        await page.mouse.move(400, 400);

        await page.keyboard.down('Control');
        await page.mouse.wheel(0, 100);
        await expect(grid(page)).toHaveAttribute('data-zoom', '2');
        await page.mouse.wheel(0, -100);
        await page.mouse.wheel(0, -100);
        await page.keyboard.up('Control');
        await expect(grid(page)).toHaveAttribute('data-zoom', '0');
    });

    test('ctrl+wheel leaves the grid alone outside it and under a dialog', async ({ alicePage: page, isMobile }) => {
        test.skip(isMobile, 'Mouse wheel');
        await gotoGrid(page);
        const level = await grid(page).getAttribute('data-zoom');
        const zoomWithWheel = async () => {
            await page.keyboard.down('Control');
            await page.mouse.wheel(0, 100);
            await page.keyboard.up('Control');
        };

        // Over the top bar, outside the grid
        const topbar = (await page.locator('.fp-topbar').boundingBox())!;
        await page.mouse.move(topbar.x + topbar.width / 2, topbar.y + topbar.height / 2);
        await zoomWithWheel();

        // Over the grid while a dialog is open
        await page.evaluate(() => document.querySelector<HTMLDialogElement>('#app-dialog')!.showModal());
        await page.mouse.move(400, 400);
        await zoomWithWheel();
        await page.evaluate(() => document.querySelector<HTMLDialogElement>('#app-dialog')!.close());

        // Over the grid with nothing on top it zooms
        expect(await grid(page).getAttribute('data-zoom')).toBe(level);
        await zoomWithWheel();
        await expect(grid(page)).toHaveAttribute('data-zoom', String(Number(level) + 1));
    });

    test('the tile under the cursor stays in place while the columns change', async ({ alicePage: page, isMobile }) => {
        test.skip(isMobile, 'Keyboard zoom');
        await gotoGrid(page);
        await scrollTo(page, 1500);
        await expectViewportFilled(page);

        const topBefore = await page.evaluate(() => {
            const centre = window.innerHeight / 2;
            const tile = [...document.querySelectorAll('.photo-tile')]
                .filter((candidate) => {
                    const rect = candidate.getBoundingClientRect();
                    return rect.top <= centre && rect.bottom >= centre;
                })[0];
            tile.setAttribute('data-anchor', '');
            return tile.getBoundingClientRect().top;
        });
        await page.keyboard.press('-');
        await expect(grid(page)).toHaveAttribute('data-zoom', '2');

        const topAfter = await page.locator('[data-anchor]').evaluate((tile) => tile.getBoundingClientRect().top);
        expect(Math.abs(topAfter - topBefore)).toBeLessThanOrEqual(2);
    });
});

test.describe('fast scroller', () => {
    const scroller = (page: Page) => page.locator('.fast-scroller');
    const thumb = (page: Page) => page.getByTestId('fast-scroller-thumb');

    test('does not exist with a single month', async ({ bobPage: page }) => {
        await gotoGrid(page, '/?filter=shared');
        await expect(sections(page)).toHaveCount(1);
        await expect(scroller(page)).toBeHidden();
        await scrollTo(page, 100);
        await expect(scroller(page)).toBeHidden();
    });

    test('appears on scroll and hides 2.5 s after scrolling stops', async ({ alicePage: page }) => {
        await page.clock.install();
        await gotoGrid(page);
        await expect(scroller(page)).toBeHidden();
        expect(await scroller(page).getAttribute('data-visible')).toBe('false');

        await scrollTo(page, 600);
        await expect(thumb(page)).toBeVisible();
        await expect(thumb(page)).toHaveAttribute('role', 'scrollbar');

        await page.clock.fastForward(2000);
        await expect(thumb(page)).toBeVisible();
        await page.clock.fastForward(1000);
        await expect(thumb(page)).toBeHidden();
    });

    test('stays visible while hovered', async ({ alicePage: page, isMobile }) => {
        test.skip(isMobile, 'Hover');
        await page.clock.install();
        await gotoGrid(page);
        await scrollTo(page, 600);
        await expect(thumb(page)).toBeVisible();

        await thumb(page).hover();
        await page.clock.fastForward(5000);
        await expect(thumb(page)).toBeVisible();

        await page.mouse.move(300, 300);
        await page.clock.fastForward(3000);
        await expect(thumb(page)).toBeHidden();
    });

    test('follows the scroll position', async ({ alicePage: page }) => {
        await gotoGrid(page);
        await scrollTo(page, 1);
        await expect(thumb(page)).toBeVisible();

        const position = () => page.evaluate(() => {
            const track = document.querySelector('.fast-scroller')!.getBoundingClientRect();
            const thumb = document.querySelector('.fast-scroller__thumb')!.getBoundingClientRect();
            const max = document.documentElement.scrollHeight - window.innerHeight;
            return { thumb: (thumb.top - track.top) / (track.height - thumb.height), page: window.scrollY / max };
        });

        await scrollTo(page, 1e6);
        await expect.poll(async () => (await position()).thumb).toBeGreaterThan(0.99);
        await scrollTo(page, 0);
        await expect.poll(async () => (await position()).thumb).toBeLessThan(0.01);
        await scrollTo(page, await page.evaluate(() => (document.documentElement.scrollHeight - window.innerHeight) / 2));
        await expect.poll(async () => {
            const { thumb, page: pageProgress } = await position();
            return Math.abs(thumb - pageProgress);
        }).toBeLessThan(0.02);
    });

    test('dragging to the bottom lands on the last month, even though it never loaded', async ({ alicePage: page, isMobile }) => {
        test.skip(isMobile, 'Mouse drag');
        await gotoGrid(page);
        const expected = months(timeline('alice', 'all'));
        const last = expected[expected.length - 1];
        await expect(page.locator(`[data-month="${last.month}"]`)).toHaveAttribute('data-loaded', 'false');

        await scrollTo(page, 1);
        await expect(thumb(page)).toBeVisible();
        // Hovering keeps the thumb from hiding while the test works, however slow the machine is
        await thumb(page).hover();
        const box = await settledBox(thumb(page));
        const track = await settledBox(scroller(page));
        await page.mouse.down();
        await page.mouse.move(box.x + box.width / 2, track.y + track.height, { steps: 8 });

        await expect(page.getByTestId('fast-scroller-label')).toHaveText(monthLabel(last.month, true));
        await page.mouse.up();

        const section = page.locator(`[data-month="${last.month}"]`);
        await expect(section).toBeInViewport();
        await expect(section).toHaveAttribute('data-loaded', 'true');
        await expectViewportFilled(page);
    });

    test('dragging to the middle puts a month header at 20% of the viewport', async ({ alicePage: page, isMobile }) => {
        test.skip(isMobile, 'Mouse drag');
        await gotoGrid(page);
        const expected = months(timeline('alice', 'all'));

        await scrollTo(page, 1);
        await expect(thumb(page)).toBeVisible();
        // Hovering keeps the thumb from hiding while the test works, however slow the machine is
        await thumb(page).hover();
        const box = await settledBox(thumb(page));
        const track = await settledBox(scroller(page));
        await page.mouse.down();
        await page.mouse.move(box.x + box.width / 2, track.y + track.height / 2, { steps: 8 });
        await page.mouse.up();

        const label = await page.getByTestId('fast-scroller-label').textContent();
        const section = page.locator('.month', { has: page.locator('[data-testid="month-title"]') })
            .filter({ hasText: label! }).first();
        await expect(section).toHaveAttribute('data-loaded', 'true');
        const { top, viewport } = await section.evaluate((element) => ({
            top: element.getBoundingClientRect().top,
            viewport: window.innerHeight,
        }));
        expect(Math.abs(top - viewport * 0.2)).toBeLessThanOrEqual(2);
        expect(expected.map(({ month }) => monthLabel(month, true))).toContain(label);
    });

    test('clicking the track jumps there', async ({ alicePage: page, isMobile }) => {
        test.skip(isMobile, 'Mouse');
        await gotoGrid(page);
        await scrollTo(page, 1);
        await expect(thumb(page)).toBeVisible();

        await scroller(page).hover({ position: { x: 4, y: 300 } });
        const track = await settledBox(scroller(page));
        await page.mouse.click(track.x + 4, track.y + track.height - 10);

        const last = months(timeline('alice', 'all')).at(-1)!;
        await expect(page.locator(`[data-month="${last.month}"]`)).toBeInViewport();
    });

    /** Waits until the scroller has slid in and the thumb stopped moving */
    const slidIn = (page: Page) => expect.poll(() => scroller(page).evaluate((element) =>
        element.getAnimations({ subtree: true }).length)).toBe(0);

    test('a tap on the track does not jump: it reaches the tile under it', async ({ alicePage: page, isMobile }) => {
        test.skip(!isMobile, 'Touch');
        await gotoGrid(page);
        await scrollTo(page, 1);
        await expect(thumb(page)).toBeVisible();
        await slidIn(page);
        const track = (await scroller(page).boundingBox())!;
        const x = track.x + track.width / 2;
        const y = track.y + track.height - 40;
        await page.evaluate(() => {
            (window as any).activated = [];
            document.addEventListener('fp:photo-activate', (event: any) =>
                (window as any).activated.push(event.detail.photoId));
        });
        const tile = await page.evaluate(([px, py]) =>
            document.elementFromPoint(px, py)?.closest<HTMLElement>('.photo-tile')?.dataset.photoId ?? null, [x, y]);
        expect(tile, 'the track lets touches through').not.toBeNull();

        await page.touchscreen.tap(x, y);

        await expect.poll(() => page.evaluate(() => (window as any).activated)).toEqual([Number(tile)]);
        expect(await page.evaluate(() => window.scrollY)).toBeLessThanOrEqual(1);
    });

    test('the thumb can be dragged with a finger', async ({ alicePage: page, isMobile }) => {
        test.skip(!isMobile, 'Touch');
        await gotoGrid(page);
        const cdp = await page.context().newCDPSession(page);
        let x = 0;
        const touch = (type: 'touchStart' | 'touchMove' | 'touchEnd' | 'touchCancel', y?: number) => cdp.send('Input.dispatchTouchEvent', {
            type, touchPoints: y === undefined ? [] : [{ x, y, id: 1 }],
        });

        // Touch the thumb right after it slid in, before it hides again
        await scrollTo(page, 1);
        await expect(thumb(page)).toBeVisible();
        await slidIn(page);
        const box = (await thumb(page).boundingBox())!;
        const track = (await scroller(page).boundingBox())!;
        x = box.x + box.width / 2;
        await touch('touchStart', box.y + box.height / 2);
        await expect(scroller(page)).toHaveAttribute('data-dragging', 'true');
        for (let step = 1; step <= 8; step++) {
            await touch('touchMove', box.y + box.height / 2 + (track.y + track.height - box.y) * step / 8);
        }
        const last = months(timeline('alice', 'all')).at(-1)!;
        await expect(page.getByTestId('fast-scroller-label')).toHaveText(monthLabel(last.month, true));
        await touch('touchEnd');

        await expect(page.locator(`[data-month="${last.month}"]`)).toBeInViewport();
    });

    test('hides while photos are selected', async ({ alicePage: page }) => {
        await gotoGrid(page);
        await scrollTo(page, 600);
        await expect(thumb(page)).toBeVisible();

        await page.evaluate(() => document.dispatchEvent(
            new CustomEvent('fp:selection-change', { detail: { count: 2, ids: new Set([1, 2]) }, bubbles: true })));
        await expect(thumb(page)).toBeHidden();
        await scrollTo(page, 900);
        await expect(thumb(page)).toBeHidden();

        await page.evaluate(() => document.dispatchEvent(
            new CustomEvent('fp:selection-change', { detail: { count: 0, ids: new Set() }, bubbles: true })));
        await scrollTo(page, 1200);
        await expect(thumb(page)).toBeVisible();
    });

    test('the native scrollbar is hidden on pages with it', async ({ alicePage: page }) => {
        await gotoGrid(page);
        const scrollbarWidth = await page.evaluate(() => window.innerWidth - document.documentElement.clientWidth);
        expect(scrollbarWidth).toBe(0);
    });
});

test.describe('month picker', () => {
    test('is not offered with only one month', async ({ bobPage: page }) => {
        await gotoGrid(page, '/?filter=shared');
        await expect(page.getByTestId('month-picker-open')).toHaveCount(0);
    });

    test('goes away when removals leave two months', async ({ alicePage: page }) => {
        await gotoGrid(page, '/?filter=personal');
        const list = months(timeline('alice', 'personal'));
        expect(list.length).toBeGreaterThan(2);
        await expect(page.getByTestId('month-picker-open').first()).toBeVisible();

        // Removals only touch loaded tiles: load every month first
        await page.evaluate(() => {
            const grid = document.querySelector<any>('.photo-grid').fpGrid;
            return Promise.all(grid.sections().map((section: HTMLElement) =>
                grid.ensureLoaded(section, Number(section.dataset.count))));
        });
        await page.evaluate((removed) => document.body.dispatchEvent(
            new CustomEvent('fp:photos-removed', { detail: { ids: removed }, bubbles: true })),
        list.slice(2).flatMap((month) => month.photos.map((photo) => photo.id)));

        await expect(sections(page)).toHaveCount(2);
        await expect(page.getByTestId('month-picker-open')).toHaveCount(2);
        for (const button of await page.getByTestId('month-picker-open').all()) await expect(button).toBeHidden();
    });

    test('lists the months and jumps to the chosen one', async ({ alicePage: page }) => {
        await gotoGrid(page);
        const expected = months(timeline('alice', 'all'));
        await expect(page.getByTestId('month-picker-open')).toHaveCount(expected.length);

        await page.getByTestId('month-picker-open').first().click();
        const dialog = page.getByTestId('app-dialog');
        await expect(dialog).toBeVisible();
        await expect(page.getByRole('heading', { name: 'Select a Month' })).toBeVisible();
        const rows = page.getByTestId('month-picker-row');
        await expect(rows).toHaveCount(expected.length);
        await expect(rows.first()).toContainText(monthLabel(expected[0].month, true));
        await expect(rows.first()).toContainText(`${expected[0].photos.length} photo`);
        await expect(rows.first().locator('img')).toHaveAttribute('src', `/api/preview/${expected[0].photos[0].id}`);

        const target = expected[Math.floor(expected.length / 2)];
        await rows.nth(Math.floor(expected.length / 2)).click();
        await expect(dialog).toBeHidden();

        const section = page.locator(`[data-month="${target.month}"]`);
        await expect(section).toHaveAttribute('data-loaded', 'true');
        await expect.poll(() => section.evaluate((element) =>
            Math.abs(element.getBoundingClientRect().top - window.innerHeight * 0.2))).toBeLessThanOrEqual(2);
    });
});

test.describe('ensureLoaded', () => {
    test('settles when the load is aborted because the section went far away', async ({ alicePage: page }) => {
        await page.setViewportSize({ width: 1280, height: 500 });
        await gotoGrid(page);
        const last = months(timeline('alice', 'all')).at(-1)!;
        // The request never gets an answer
        await page.route((url) => url.pathname === '/ui/grid/month' && url.searchParams.get('month') === last.month, () => {});
        const requested = page.waitForRequest((request) => request.url().includes(`month=${last.month}`));

        await page.evaluate((month) => {
            const section = document.querySelector(`[data-month="${month}"]`);
            (window as any).settled = false;
            document.querySelector<any>('.photo-grid').fpGrid.ensureLoaded(section, 1)
                .then(() => { (window as any).settled = true; });
        }, last.month);
        await requested;
        expect(await page.evaluate(() => (window as any).settled)).toBe(false);

        // The section is far from the viewport: the next scroll aborts its load
        await scrollTo(page, 10);
        await expect.poll(() => page.evaluate(() => (window as any).settled)).toBe(true);
    });

    test('settles when the section is removed while it loads', async ({ alicePage: page }) => {
        await gotoGrid(page, '/?filter=personal');
        const first = months(timeline('alice', 'personal'))[0];
        await page.route((url) => url.pathname === '/ui/grid/month' && url.searchParams.get('month') === first.month, () => {});
        const requested = page.waitForRequest((request) => request.url().includes(`month=${first.month}`));

        // Drop the last tile so the section has one left to load, and wait for it
        await page.evaluate((month) => {
            const section = document.querySelector<HTMLElement>(`[data-month="${month}"]`)!;
            [...section.querySelectorAll('.photo-tile')].at(-1)!.remove();
            (window as any).settled = false;
            document.querySelector<any>('.photo-grid').fpGrid.ensureLoaded(section, Number(section.dataset.count))
                .then(() => { (window as any).settled = true; });
        }, first.month);
        await requested;

        // As if the missing photo had gone elsewhere, then every loaded photo is removed: the section goes
        const ids = await page.evaluate((month) => {
            const section = document.querySelector<HTMLElement>(`[data-month="${month}"]`)!;
            const loaded = [...section.querySelectorAll<HTMLElement>('.photo-tile')].map((tile) => Number(tile.dataset.photoId));
            section.dataset.count = String(loaded.length);
            return loaded;
        }, first.month);
        await page.evaluate((removed) => document.body.dispatchEvent(
            new CustomEvent('fp:photos-removed', { detail: { ids: removed }, bubbles: true })), ids);

        await expect(page.locator(`[data-month="${first.month}"]`)).toHaveCount(0);
        await expect.poll(() => page.evaluate(() => (window as any).settled)).toBe(true);
    });
});

test.describe('filters', () => {
    const chip = (page: Page, name: Filter) => page.getByTestId(`filter-chip-${name}`);

    test('Personal shows only the own photos, Family the public ones, All the union', async ({ alicePage: page }) => {
        await gotoGrid(page);
        await expect(chip(page, 'all')).toHaveAttribute('aria-pressed', 'true');

        await chip(page, 'personal').click();
        await expect(grid(page)).toHaveAttribute('data-context', 'timeline:personal');
        await expect(page).toHaveURL(/filter=personal/);
        await expect(chip(page, 'personal')).toHaveAttribute('aria-pressed', 'true');
        await expect(chip(page, 'all')).toHaveAttribute('aria-pressed', 'false');
        const personal = timeline('alice', 'personal');
        await expect(grid(page)).toHaveAttribute('data-total', String(personal.length));
        await expect(tiles(page)).toHaveCount(personal.length);
        const ids = await tiles(page).evaluateAll((all) => all.map((tile) => Number(tile.dataset.photoId)));
        expect(ids).toEqual(personal.map((photo) => photo.id));

        await chip(page, 'family').click();
        await expect(grid(page)).toHaveAttribute('data-context', 'timeline:family');
        const family = timeline('alice', 'family');
        await expect(grid(page)).toHaveAttribute('data-total', String(family.length));
        const familyIds = new Set(family.map((photo) => photo.id));
        await expectViewportFilled(page);
        const shown = await tiles(page).evaluateAll((all) => all.map((tile) => Number(tile.dataset.photoId)));
        expect(shown.length).toBeGreaterThan(0);
        expect(shown.every((id) => familyIds.has(id))).toBe(true);

        await chip(page, 'all').click();
        await expect(grid(page)).toHaveAttribute('data-total', String(timeline('alice', 'all').length));
        expect(timeline('alice', 'all').length).toBe(new Set([...personal, ...family].map((p) => p.id)).size);
    });

    test('Shared shows the photos of folders shared with the user', async ({ bobPage: page }) => {
        await gotoGrid(page);
        await chip(page, 'shared').click();
        await expect(grid(page)).toHaveAttribute('data-context', 'timeline:shared');

        const holiday = timeline('bob', 'shared');
        expect(holiday.length).toBe(8);
        await expect(tiles(page)).toHaveCount(holiday.length);
        const ids = await tiles(page).evaluateAll((all) => all.map((tile) => Number(tile.dataset.photoId)));
        expect(ids).toEqual(holiday.map((photo) => photo.id));
    });

    test('Shared is empty for the owner of the folder', async ({ alicePage: page }) => {
        await gotoGrid(page, '/?filter=shared');
        await expect(page.getByTestId('grid-empty')).toBeVisible();
        await expect(page.getByTestId('grid-empty')).toContainText('No photos yet');
        await expect(page.getByTestId('grid-empty-upload')).toHaveAttribute('href', '/upload');
        await expect(tiles(page)).toHaveCount(0);
    });

    test('the choice is remembered', async ({ alicePage: page }) => {
        await gotoGrid(page);
        await chip(page, 'family').click();
        await expect(chip(page, 'family')).toHaveAttribute('aria-pressed', 'true');

        await gotoGrid(page, '/');
        await expect(chip(page, 'family')).toHaveAttribute('aria-pressed', 'true');
        await expect(grid(page)).toHaveAttribute('data-context', 'timeline:family');

        // ?filter= wins over the saved choice for that visit
        await gotoGrid(page, '/?filter=personal');
        await expect(grid(page)).toHaveAttribute('data-context', 'timeline:personal');
        // and an invalid one is ignored
        await gotoGrid(page, '/?filter=nonsense');
        await expect(grid(page)).toHaveAttribute('data-context', 'timeline:family');
    });

    test('the swapped content is wired up again', async ({ alicePage: page }) => {
        await gotoGrid(page);
        await chip(page, 'personal').click();
        await expect(grid(page)).toHaveAttribute('data-context', 'timeline:personal');
        await expect(grid(page)).toHaveAttribute('data-ready', '');

        await page.keyboard.press('-');
        await expect(grid(page)).toHaveAttribute('data-zoom', '2');
        await chip(page, 'all').click();
        await expect(grid(page)).toHaveAttribute('data-context', 'timeline:all');
        await expect(grid(page)).toHaveAttribute('data-zoom', '2');
        await expect(grid(page)).toHaveAttribute('data-ready', '');

        await page.goBack();
        await expect(grid(page)).toHaveAttribute('data-context', 'timeline:personal');
    });
});

test.describe('memories', () => {
    test('shows the "On this day" cards of the filter', async ({ alicePage: page }) => {
        await gotoGrid(page);
        const cards = page.getByTestId('memory-card');
        const memory2y = getSeed().photos['public/memory-2y.jpg'];
        const memory5y = getSeed().photos['public/memory-5y.jpg'];
        const yearsAgo = (photo: SeedPhoto) => new Date().getUTCFullYear() - new Date(photo.createdAt * 1000).getUTCFullYear();

        await expect(cards.first()).toHaveAttribute('data-photo-id', String(memory2y.id));
        await expect(cards.first()).toContainText('2 years ago');
        await expect(cards.first().locator('img')).toHaveAttribute('src', `/api/preview/${memory2y.id}`);
        await expect(cards.first()).toHaveAttribute('data-context', 'memories:2');
        // Oldest last, labelled with the years since
        await expect(cards.last()).toHaveAttribute('data-photo-id', String(memory5y.id));
        await expect(cards.last()).toContainText(`${yearsAgo(memory5y)} years ago`);

        const box = (await cards.first().boundingBox())!;
        expect(box.width).toBe(140);
        expect(Math.round(box.height)).toBe(187);

        // They are public photos, so personal has none
        await gotoGrid(page, '/?filter=personal');
        await expect(page.getByTestId('memory-card')).toHaveCount(0);
        await expect(page.getByTestId('memories')).toHaveCount(0);
    });

    test('activating a card asks to open the group', async ({ alicePage: page }) => {
        await gotoGrid(page);
        await page.evaluate(() => {
            (window as any).activations = [];
            document.addEventListener('fp:photo-activate', (event) => {
                const { photoId, index, contextKey, tile } = (event as CustomEvent).detail;
                (window as any).activations.push({ photoId, index, contextKey, isCard: tile.dataset.testid === 'memory-card' });
            });
        });

        await page.getByTestId('memory-card').first().click();

        const memory2y = getSeed().photos['public/memory-2y.jpg'];
        expect(await page.evaluate(() => (window as any).activations))
            .toEqual([{ photoId: memory2y.id, index: 0, contextKey: 'memories:2', isCard: true }]);
    });
});

test.describe('tiles', () => {
    test('activating a tile with click, Enter or Space announces it with its context', async ({ alicePage: page }) => {
        await gotoGrid(page);
        await page.evaluate(() => {
            (window as any).activations = [];
            document.addEventListener('fp:photo-activate', (event) => {
                const { photoId, index, contextKey } = (event as CustomEvent).detail;
                (window as any).activations.push({ photoId, index, contextKey, cancelable: event.cancelable });
                // Keep the viewer closed (capturing runs before its listener), so the tile can be activated again
                event.preventDefault();
            }, true);
        });
        const expected = timeline('alice', 'all');

        await tiles(page).nth(3).click();
        await tiles(page).nth(3).press('Enter');
        await tiles(page).nth(3).press('Space');

        const activation = { photoId: expected[3].id, index: 3, contextKey: 'timeline:all', cancelable: true };
        expect(await page.evaluate(() => (window as any).activations)).toEqual([activation, activation, activation]);
    });

    test('a prevented activation does nothing else', async ({ alicePage: page }) => {
        await gotoGrid(page);
        const result = await page.evaluate(() => {
            let seen = 0;
            document.addEventListener('fp:photo-activate', (event) => { seen += 1; event.preventDefault(); });
            document.querySelector<HTMLElement>('.photo-tile')!.click();
            return seen;
        });
        expect(result).toBe(1);
    });

    test('has the contract attributes and a thumbhash placeholder until the preview loads', async ({ alicePage: page }) => {
        await gotoGrid(page);
        const first = tiles(page).first();
        const expected = timeline('alice', 'all')[0];

        await expect(first).toHaveAttribute('data-photo-id', String(expected.id));
        await expect(first).toHaveAttribute('data-index', '0');
        await expect(first).toHaveAttribute('data-deletable', 'true');
        await expect(first).toHaveAttribute('data-video', 'false');
        await expect(first).toHaveAttribute('aria-label', /^Photo, \d{1,2} \w{3} \d{4} \d{2}:\d{2}$/);
        await expect(first.locator('canvas')).toHaveAttribute('data-thumbhash', /.+/);
        await expect(first).toHaveClass(/is-loaded/);
    });

    test('videos are flagged and show the play icon', async ({ bobPage: page }) => {
        await gotoGrid(page, '/?filter=personal');
        const video = getSeed().photos['bob/VID_20240610_120000.mp4'];

        const tile = page.locator(`[data-photo-id="${video.id}"]`);
        await expect(tile).toHaveAttribute('data-video', 'true');
        await expect(tile).toHaveAttribute('aria-label', /^Video, /);
        await expect(tile.locator('.photo-tile__video')).toBeVisible();
        await expect(tiles(page).and(page.locator('[data-video="false"]'))).toHaveCount(3);
    });

    test('keeps the thumbhash when the image fails to load', async ({ alicePage: page }) => {
        await page.route('**/api/preview/*', (route) => route.fulfill({ status: 503, body: 'nope' }));
        await gotoGrid(page);

        const first = tiles(page).first();
        await expect(first.locator('img')).toHaveJSProperty('complete', true);
        await expect(first).toHaveClass(/is-error/);
        await expect(first).not.toHaveClass(/is-loaded/);
        await expect.poll(() => first.locator('canvas').evaluate((canvas: HTMLCanvasElement) => canvas.width)).toBeGreaterThan(0);
        await expect(first.locator('canvas')).toBeVisible();
        expect(await first.locator('canvas').evaluate((canvas) => getComputedStyle(canvas).opacity)).toBe('1');
    });

    test('the selection classes are styled', async ({ alicePage: page }) => {
        await gotoGrid(page);
        const tile = tiles(page).first();

        const check = tile.locator('.photo-tile__check');
        await page.mouse.move(0, 0);
        await expect(check).toHaveCSS('opacity', '0');

        await tile.evaluate((element) => element.classList.add('is-selected'));
        await expect(check).toHaveCSS('opacity', '1');
        const scrim = await tile.evaluate((element) => getComputedStyle(element, '::after').backgroundColor);
        expect(scrim).toBe('rgba(0, 0, 0, 0.4)');
        await tile.evaluate((element) => element.classList.remove('is-selected'));

        await grid(page).evaluate((element) => element.classList.add('selecting'));
        await expect(check).toHaveCSS('opacity', '1');
    });
});

test.describe('keyboard', () => {
    const focusedIndex = (page: Page) => page.evaluate(() =>
        Number((document.activeElement as HTMLElement).dataset.index ?? -1));

    test('one tab stop, arrows move between tiles, Enter activates', async ({ alicePage: page, isMobile }) => {
        test.skip(isMobile, 'Keyboard');
        await gotoGrid(page);
        const total = timeline('alice', 'all').length;
        const cols = COLUMNS[1440][1];

        // Only one tile can be tabbed to
        expect(await tiles(page).evaluateAll((all) => all.filter((tile) => tile.tabIndex === 0).length)).toBe(1);

        // The month header's buttons come before the tiles
        await sections(page).first().getByTestId('month-picker-open').focus();
        await page.keyboard.press('Tab');
        await expect(tiles(page).first()).toBeFocused();

        await page.keyboard.press('ArrowRight');
        expect(await focusedIndex(page)).toBe(1);
        await page.keyboard.press('ArrowDown');
        expect(await focusedIndex(page)).toBe(1 + cols);
        await page.keyboard.press('ArrowLeft');
        await page.keyboard.press('ArrowUp');
        expect(await focusedIndex(page)).toBe(0 + 0);
        await page.keyboard.press('ArrowLeft');
        expect(await focusedIndex(page)).toBe(0);

        // Moving focus moves the tab stop
        expect(await tiles(page).evaluateAll((all) => all.filter((tile) => tile.tabIndex === 0).length)).toBe(1);
        await page.keyboard.press('ArrowRight');
        await page.keyboard.press('Tab');
        await page.keyboard.press('Shift+Tab');
        expect(await focusedIndex(page)).toBe(1);

        // End goes to the very last photo, loading its section first
        await page.keyboard.press('End');
        await expect.poll(() => focusedIndex(page)).toBe(total - 1);
        await expect(page.locator(':focus')).toBeInViewport();
        await page.keyboard.press('Home');
        await expect.poll(() => focusedIndex(page)).toBe(0);
        await expect(page.locator(':focus')).toBeInViewport();

        await page.evaluate(() => {
            (window as any).activated = [];
            document.addEventListener('fp:photo-activate', (event) => (window as any).activated.push((event as CustomEvent).detail.index));
        });
        await page.keyboard.press('Enter');
        expect(await page.evaluate(() => (window as any).activated)).toEqual([0]);
    });

    test('arrow keys cross from one month to the next', async ({ alicePage: page, isMobile }) => {
        test.skip(isMobile, 'Keyboard');
        await gotoGrid(page);
        const first = months(timeline('alice', 'all'))[0];
        const next = first.photos.length;

        await tiles(page).nth(next - 1).focus();
        await page.keyboard.press('ArrowRight');
        await expect.poll(() => focusedIndex(page)).toBe(next);
        await page.keyboard.press('ArrowLeft');
        await expect.poll(() => focusedIndex(page)).toBe(next - 1);
    });

    test('Page Down moves focus by a screen', async ({ alicePage: page, isMobile }) => {
        test.skip(isMobile, 'Keyboard');
        await gotoGrid(page);
        await tiles(page).first().focus();
        await page.keyboard.press('PageDown');
        await expect.poll(() => focusedIndex(page)).toBeGreaterThan(5);
        await expect(page.locator(':focus')).toBeInViewport();
    });
});

test.describe('removal', () => {
    const remove = (page: Page, ids: number[]) => page.evaluate((removed) => document.body.dispatchEvent(
        new CustomEvent('fp:photos-removed', { detail: { ids: removed }, bubbles: true })), ids);

    test('removing photos updates counts, indices and sections', async ({ alicePage: page }) => {
        await gotoGrid(page, '/?filter=personal');
        const expected = timeline('alice', 'personal');
        const monthList = months(expected);
        const firstMonth = monthList[0];

        // One photo of the first month (July 2024: alice's Holiday folder)
        expect(firstMonth.photos.length).toBe(8);
        await remove(page, [firstMonth.photos[0].id]);
        await expect(grid(page)).toHaveAttribute('data-total', String(expected.length - 1));
        await expect(sections(page).first()).toHaveAttribute('data-count', '7');
        await expect(sections(page).first().locator('.month__grid')).toHaveCSS('--count', '7');

        // Indices are contiguous again and the later sections start in the right place
        const indices = await tiles(page).evaluateAll((all) => all.map((tile) => Number(tile.dataset.index)));
        expect(indices).toEqual(indices.map((_, i) => i));
        const starts = await sections(page).evaluateAll((all) => all.map((s) => Number(s.dataset.startIndex)));
        let start = 0;
        for (const [i, s] of (await sections(page).evaluateAll((all) => all.map((el) => Number(el.dataset.count)))).entries()) {
            expect(starts[i]).toBe(start);
            start += s;
        }

        // Removing every photo of a month removes the section
        const remaining = months(expected.filter((photo) => photo.id !== firstMonth.photos[0].id));
        const doomed = remaining[0];
        await remove(page, doomed.photos.map((photo) => photo.id));
        await expect(page.locator(`[data-month="${doomed.month}"]`)).toHaveCount(0);
    });

    test('removing the last photo shows the empty state', async ({ bobPage: page }) => {
        await gotoGrid(page, '/?filter=shared');
        await expect(page.getByTestId('grid-empty')).toBeHidden();
        await page.evaluate(() => {
            const counter = document.createElement('span');
            counter.dataset.bind = 'photo-count';
            counter.dataset.testid = 'count';
            document.body.append(counter);
        });
        const ids = timeline('bob', 'shared').map((photo) => photo.id);

        await page.evaluate((removed) => document.body.dispatchEvent(
            new CustomEvent('fp:photos-removed', { detail: { ids: removed.slice(0, 3) }, bubbles: true })), ids);
        await expect(page.getByTestId('count')).toHaveText('5 photos');
        await page.evaluate((removed) => document.body.dispatchEvent(
            new CustomEvent('fp:photos-removed', { detail: { ids: removed }, bubbles: true })), ids);

        await expect(tiles(page)).toHaveCount(0);
        await expect(sections(page)).toHaveCount(0);
        await expect(page.getByTestId('grid-empty')).toBeVisible();
        await expect(page.getByTestId('count')).toHaveText('0 photos');
    });

    test('removals from another context are ignored', async ({ alicePage: page }) => {
        await gotoGrid(page);
        const before = await tiles(page).count();
        const id = timeline('alice', 'all')[0].id;

        await page.evaluate((photoId) => document.body.dispatchEvent(
            new CustomEvent('fp:photos-removed', { detail: { ids: [photoId], contextKey: 'trash' }, bubbles: true })), id);

        await expect(tiles(page)).toHaveCount(before);
    });
});

test.describe('fragments', () => {
    test('a bad month is rejected and a missing session answers with HX-Redirect', async ({ alicePage: page, request }) => {
        const bad = await page.request.get('/ui/grid/month?ctx=timeline:all&month=2024-6');
        expect(bad.status()).toBe(400);

        const anonymous = await request.get('/ui/grid/month?ctx=timeline:all&month=2024-06', {
            headers: { 'hx-current-url': 'http://localhost/?filter=family' },
            maxRedirects: 0,
        });
        expect(anonymous.status()).toBe(401);
        expect(anonymous.headers()['hx-redirect']).toBe('/login?next=%2F%3Ffilter%3Dfamily');
    });

    test('a folder without access is a 404', async ({ bobPage: page }) => {
        const response = await page.request.get(`/ui/grid/month?ctx=folder:${folderId('Private')}&month=2024-01`);
        expect(response.status()).toBe(404);
    });
});
