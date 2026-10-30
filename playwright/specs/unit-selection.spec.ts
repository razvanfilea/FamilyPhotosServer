// Unit tests of the pure selection helpers, run without a browser.
import { expect, test } from '@playwright/test';
import {
    autoScrollSpeed,
    chunk,
    missingRequests,
    monthOf,
    rangeIndices,
    runPool,
    staysInContext,
    summarize,
    visibilityChange,
} from '../../assets/js/selection/math.js';

test('rangeIndices is inclusive and works in both directions', () => {
    expect(rangeIndices(2, 5)).toEqual([2, 3, 4, 5]);
    expect(rangeIndices(5, 2)).toEqual([2, 3, 4, 5]);
    expect(rangeIndices(7, 7)).toEqual([7]);
});

test('missingRequests groups unknown runs and splits them at the chunk size', () => {
    const known = new Set([2, 3, 10]);
    const isKnown = (index: number) => known.has(index);
    const cases: [number[], { start: number, limit: number }[]][] = [
        [rangeIndices(0, 4), [{ start: 0, limit: 2 }, { start: 4, limit: 1 }]],
        [rangeIndices(2, 3), []],
        [rangeIndices(8, 12), [{ start: 8, limit: 2 }, { start: 11, limit: 2 }]],
    ];
    for (const [indices, expected] of cases) {
        expect(missingRequests(indices, isKnown), String(indices)).toEqual(expected);
    }
    expect(missingRequests(rangeIndices(100, 349), () => false)).toEqual([
        { start: 100, limit: 100 }, { start: 200, limit: 100 }, { start: 300, limit: 50 },
    ]);
    expect(missingRequests(rangeIndices(0, 4), () => false, 2)).toEqual([
        { start: 0, limit: 2 }, { start: 2, limit: 2 }, { start: 4, limit: 1 },
    ]);
});

test('autoScrollSpeed scrolls in the top and bottom zones, proportional to the depth', () => {
    // The visible area is 64..800 and the zone 40 px
    const cases: [number, number][] = [
        [400, 0],
        [104, 0],
        [103, -1],
        [84, -20],
        [64, -40],
        [10, -40],
        [760, 0],
        [761, 1],
        [780, 20],
        [800, 40],
        [900, 40],
    ];
    for (const [y, expected] of cases) {
        expect(autoScrollSpeed(y, 64, 800), `y=${y}`).toBe(expected);
    }
});

test('chunk splits in arrays of at most the size', () => {
    expect(chunk([1, 2, 3, 4, 5], 2)).toEqual([[1, 2], [3, 4], [5]]);
    expect(chunk([], 2)).toEqual([]);
});

test('runPool respects the limit, keeps the order and reports failures', async () => {
    let running = 0;
    let peak = 0;
    const outcomes = await runPool([1, 2, 3, 4, 5, 6], 2, async (item) => {
        running += 1;
        peak = Math.max(peak, running);
        await new Promise((resolve) => setTimeout(resolve, 5));
        running -= 1;
        if (item === 4) throw new Error('four');
        return item * 10;
    });
    expect(peak).toBe(2);
    expect(outcomes.map((o) => o.ok)).toEqual([true, true, true, false, true, true]);
    expect(outcomes[2]).toEqual({ ok: true, value: 30 });
    expect((outcomes[3] as { error: Error }).error.message).toBe('four');
    expect(await runPool([], 4, async () => 1)).toEqual([]);
});

test('monthOf is the UTC month of a timestamp', () => {
    expect(monthOf(Date.UTC(2024, 5, 8, 14) / 1000)).toBe('2024-06');
    expect(monthOf(Date.UTC(2023, 11, 31, 23, 59, 59) / 1000)).toBe('2023-12');
});

test('summarize counts photos the user can not delete, and photos only others own', () => {
    const cases: [Parameters<typeof summarize>[0], ReturnType<typeof summarize>][] = [
        [[], { count: 0, undeletable: 0, foreign: 0 }],
        // Unknown photos (never loaded) count for neither: the server decides
        [[{}, {}], { count: 2, undeletable: 0, foreign: 0 }],
        [[{ deletable: true, owner: 'alice' }, { deletable: true, owner: null }], { count: 2, undeletable: 0, foreign: 0 }],
        [[{ deletable: false, owner: 'alice' }, { deletable: true, owner: 'bob' }], { count: 2, undeletable: 1, foreign: 1 }],
    ];
    for (const [metas, expected] of cases) {
        expect(summarize(metas, 'alice'), JSON.stringify(metas)).toEqual(expected);
    }
});

test('staysInContext follows the filter of the context', () => {
    const own = { user_id: 'alice', folder_id: null };
    const family = { user_id: null, folder_id: null };
    const bobs = { user_id: 'bob', folder_id: 4 };
    const cases: [string, typeof own, boolean][] = [
        ['timeline:all', own, true],
        ['timeline:personal', own, true],
        ['timeline:personal', family, false],
        ['timeline:family', family, true],
        ['timeline:family', own, false],
        ['timeline:shared', bobs, true],
        ['timeline:shared', own, false],
        ['timeline:shared', family, false],
        ['folder:4', bobs, true],
        ['folder:4', own, false],
        ['folder:4', { user_id: 'alice', folder_id: 44 }, false],
        ['favorites', family, true],
        ['large-files', own, true],
        ['memories:2', own, true],
    ];
    for (const [context, photo, expected] of cases) {
        expect(staysInContext(context, photo, 'alice'), `${context} ${JSON.stringify(photo)}`).toBe(expected);
    }
});

test('visibilityChange tells when moving changes who sees the photos', () => {
    const cases: [{ public: boolean, private: boolean }, boolean, string | null][] = [
        [{ public: true, private: false }, false, 'private'],
        [{ public: true, private: false }, true, null],
        [{ public: false, private: true }, true, 'family'],
        [{ public: false, private: true }, false, null],
        [{ public: true, private: true }, true, 'family'],
        [{ public: true, private: true }, false, 'private'],
        [{ public: false, private: false }, true, null],
        [{ public: false, private: false }, false, null],
    ];
    for (const [sources, destinationPublic, expected] of cases) {
        expect(visibilityChange(sources, destinationPublic), JSON.stringify([sources, destinationPublic])).toBe(expected);
    }
});
