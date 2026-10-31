import type { CDPSession, Page } from '@playwright/test';
import { expect, test } from '@playwright/test';

// Opt-in browser performance check on a large library:
//   scripts/seed-large-db.sh /tmp/fp-perf/perf.db 200000, give alice a password, start a release server on it, then
//   PERF=1 PERF_BASE_URL=http://localhost:8132 PERF_PASSWORD=... npm run test:perf
// It is skipped otherwise, so the normal suite and scripts/check.sh don't need a big database.
test.skip(!process.env.PERF || !process.env.PERF_BASE_URL, 'set PERF=1 and PERF_BASE_URL');
test.describe.configure({ mode: 'serial' });

// Long tasks: headless Chromium rasterizes in software, so scrolling gives 50-70 ms tasks and outliers of 165-340 ms (target 200)
// (JS is a small part: thumbHashToRGBA ~7 ms per step); the 500 ms ceiling avoids flaking
const BUDGET = { firstLoadMs: 1500, tiles: 2000, longTaskMs: 500, heapGrowth: 1.3 };
const tiles = (page: Page) => page.getByTestId('photo-tile');
const tileCount = (page: Page) => page.locator('[data-testid=photo-tile]').count();

type Probe = { tasks: number[]; maxTiles: number };
declare global {
    interface Window { __perf: Probe }
}

let page: Page;
let cdp: CDPSession;
const report: Record<string, unknown> = {};

test.beforeAll(async ({ browser }) => {
    const context = await browser.newContext({ viewport: { width: 1440, height: 900 } });
    const login = await context.request.post('/api/login', {
        form: { user_id: 'alice', password: process.env.PERF_PASSWORD ?? 'perf' },
    });
    expect(login.ok()).toBe(true);
    page = await context.newPage();
    cdp = await context.newCDPSession(page);
    await page.addInitScript(() => {
        window.__perf = { tasks: [], maxTiles: 0 };
        new PerformanceObserver((list) => {
            for (const entry of list.getEntries()) window.__perf.tasks.push(entry.duration);
        }).observe({ type: 'longtask', buffered: true });
    });
});

test.afterAll(() => {
    console.log(`PERF ${JSON.stringify(report)}`);
});

/** Waits until the grid stops changing (no section is loading and the tile count is steady) */
async function settle() {
    let last = -1;
    await expect.poll(async () => {
        const now = await tileCount(page);
        const stable = now === last && (await tiles(page).first().isVisible());
        last = now;
        return stable;
    }, { intervals: [150], timeout: 30_000 }).toBe(true);
}

async function heapMb() {
    await cdp.send('HeapProfiler.collectGarbage');
    const { usedSize } = await cdp.send('Runtime.getHeapUsage');
    return Math.round(usedSize / 1e4) / 100;
}

async function maxLongTask() {
    const tasks = await page.evaluate(() => window.__perf.tasks);
    return { count: tasks.length, max: Math.round(Math.max(0, ...tasks)) };
}

/** Drags the fast scroller thumb along the whole track in steps, sampling the tile count at each step */
async function dragThumb(down: boolean, samples: number[]) {
    const thumb = page.getByTestId('fast-scroller-thumb');
    const track = page.locator('.fast-scroller');
    await page.mouse.move(600, 450);
    await page.mouse.wheel(0, down ? 100 : -100); // the thumb appears on scroll
    await expect(track).toHaveAttribute('data-visible', 'true');
    const box = (await thumb.boundingBox())!;
    const trackBox = (await track.boundingBox())!;
    await page.mouse.move(box.x + box.width / 2, box.y + box.height / 2);
    await page.mouse.down();
    const steps = 40;
    for (let step = 1; step <= steps; step++) {
        const fraction = down ? step / steps : 1 - step / steps;
        await page.mouse.move(box.x + box.width / 2, trackBox.y + box.height / 2 + fraction * (trackBox.height - box.height));
        samples.push(await tileCount(page));
        if (step % 5 === 0) await settle();
    }
    await page.mouse.up();
    await settle();
    samples.push(await tileCount(page));
}

test('timeline first load', async () => {
    const start = Date.now();
    await page.goto('/');
    await expect(tiles(page).first()).toBeVisible();
    const firstTiles = Date.now() - start;
    const fcp = await page.evaluate(() => performance.getEntriesByName('first-contentful-paint')[0]?.startTime ?? -1);
    const nav = await page.evaluate(() => {
        const e = performance.getEntriesByType('navigation')[0] as PerformanceNavigationTiming;
        return { responseEnd: Math.round(e.responseEnd), domInteractive: Math.round(e.domInteractive), bytes: e.transferSize };
    });
    Object.assign(report, { fcpMs: Math.round(fcp), firstTilesMs: firstTiles, ...nav, tilesAtLoad: await tileCount(page) });
    expect(fcp).toBeGreaterThan(0);
    expect(fcp).toBeLessThan(BUDGET.firstLoadMs);
    expect(firstTiles).toBeLessThan(BUDGET.firstLoadMs);
    await expect(page.getByTestId('photo-grid')).toHaveAttribute('data-ready', '');
});

test('fast scroller passes keep the DOM and the heap bounded', async () => {
    await settle();
    const samples: number[] = [];
    const heap = [await heapMb()];
    for (let pass = 0; pass < 2; pass++) {
        await dragThumb(true, samples);
        await dragThumb(false, samples);
        heap.push(await heapMb());
    }
    Object.assign(report, { dragMaxTiles: Math.max(...samples), heapMb: heap });
    expect(Math.max(...samples)).toBeLessThan(BUDGET.tiles);
    // The second pass must not grow the heap much over the first
    expect(heap[2]).toBeLessThan(heap[1] * BUDGET.heapGrowth);
});

test('continuous wheel scroll', async () => {
    await page.evaluate(() => window.scrollTo(0, 0));
    await settle();
    const samples: number[] = [];
    await page.mouse.move(600, 450);
    for (let step = 0; step < 200; step++) {
        await page.mouse.wheel(0, 2000);
        samples.push(await tileCount(page));
        if (step % 20 === 19) await settle();
    }
    await settle();
    samples.push(await tileCount(page));
    report.wheelMaxTiles = Math.max(...samples);
    report.wheelScrolledPx = await page.evaluate(() => Math.round(scrollY));
    expect(Math.max(...samples)).toBeLessThan(BUDGET.tiles);
});

test('largest month', async () => {
    // A month with more than 200 tiles loads in several sequential pages, only as far as the viewport needs
    const biggest = await page.evaluate(() => [...document.querySelectorAll<HTMLElement>('.month')]
        .map((s) => ({ month: s.dataset.month!, count: Number(s.dataset.count) }))
        .sort((a, b) => b.count - a.count)[0]);
    const section = page.locator(`.month[data-month="${biggest.month}"]`);
    const start = Date.now();
    await section.evaluate((element) => window.scrollTo(0, element.getBoundingClientRect().top + scrollY - 60));
    await expect(section.locator('.photo-tile').first()).toBeInViewport();
    const firstScreenMs = Date.now() - start;
    const height = (await section.boundingBox())!.height;
    let maxTiles = 0;
    for (let offset = 0; offset < height; offset += 800) {
        await page.mouse.wheel(0, 800);
        await settle();
        maxTiles = Math.max(maxTiles, await tileCount(page));
    }
    const inSection = await section.locator('.photo-tile').count();
    Object.assign(report, { bigMonth: biggest.month, bigMonthPhotos: biggest.count, bigMonthHeightPx: Math.round(height),
        bigMonthFirstScreenMs: firstScreenMs, bigMonthTilesAfterScroll: inSection, tilesWhileInBigMonth: maxTiles });
    expect(maxTiles).toBeLessThan(BUDGET.tiles);
});

test('no long tasks while scrolling', async () => {
    const { count, max } = await maxLongTask();
    Object.assign(report, { longTasks: count, longTaskMaxMs: max });
    expect(max).toBeLessThan(BUDGET.longTaskMs);
});
