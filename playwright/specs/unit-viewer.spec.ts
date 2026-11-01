// Unit tests of the pure viewer helpers, run without a browser.
import { expect, test } from '@playwright/test';
import {
    SWIPE_CLOSE_DISTANCE,
    clampTranslation,
    fitContain,
    formatTime,
    hasExtension,
    releaseVelocity,
    swipeOutcome,
    zoomAbout,
} from '../../assets/js/viewer/math.js';

test('fitContain fits the image inside the box, keeping its aspect ratio', () => {
    expect(fitContain(4000, 3000, 800, 800)).toEqual({ width: 800, height: 600 });
    expect(fitContain(3000, 4000, 1600, 900)).toEqual({ width: 675, height: 900 });
    expect(fitContain(100, 50, 1000, 1000)).toEqual({ width: 1000, height: 500 });
    // Unknown sizes fill the box
    expect(fitContain(0, 0, 300, 200)).toEqual({ width: 300, height: 200 });
});

test('zoomAbout keeps the point under the cursor in place', () => {
    const state = { x: 0, y: 0, scale: 1 };
    const point = { x: 100, y: -50 };
    const zoomed = zoomAbout(state, 2, point);
    expect(zoomed).toEqual({ x: -100, y: 50, scale: 2 });
    // The content point under the cursor: (point - translation) / scale is the same before and after
    expect((point.x - zoomed.x) / zoomed.scale).toBe((point.x - state.x) / state.scale);
    expect(zoomAbout(zoomed, 1, point)).toEqual({ x: 0, y: 0, scale: 1 });
});

test('clampTranslation keeps the edges outside the box', () => {
    const content = { width: 800, height: 600 };
    const box = { width: 800, height: 800 };
    // Not zoomed: centred
    expect(clampTranslation({ x: 50, y: 50, scale: 1 }, content, box)).toEqual({ x: 0, y: 0, scale: 1 });
    // 2×: 1600 × 1200 in 800 × 800 → ±400 horizontally, ±200 vertically
    expect(clampTranslation({ x: 1000, y: -1000, scale: 2 }, content, box)).toEqual({ x: 400, y: -200, scale: 2 });
    expect(clampTranslation({ x: 10, y: 20, scale: 2 }, content, box)).toEqual({ x: 10, y: 20, scale: 2 });
});

test.describe('releaseVelocity', () => {
    const at = (...points: [number, number][]) => points.map(([x, time]) => ({ x, time }));

    test('is the speed over the last 100 ms, not the whole gesture', () => {
        // A slow drag (200 px in 1 s), then 60 px in the last 60 ms
        const samples = at([0, 0], [100, 500], [200, 940], [230, 970], [260, 1000]);
        expect(releaseVelocity(samples, 1000)).toBeCloseTo(60 / 60);
        expect(releaseVelocity(at([0, 0], [260, 1000]), 1000)).toBe(0);
    });

    test('is signed by the direction', () => {
        expect(releaseVelocity(at([300, 920], [200, 1000]), 1000)).toBeCloseTo(-100 / 80);
    });

    test('a finger that rested before lifting is at rest', () => {
        const samples = at([0, 0], [100, 50], [101, 60]);
        expect(releaseVelocity(samples, 400)).toBe(0);
    });

    test('one sample, or none, is no speed; noise within a few ms is damped', () => {
        expect(releaseVelocity([], 100)).toBe(0);
        expect(releaseVelocity(at([5, 90]), 100)).toBe(0);
        expect(releaseVelocity(at([0, 90], [3, 91]), 100)).toBeCloseTo(3 / 10);
    });
});

test.describe('swipeOutcome', () => {
    const swipe = { dx: 0, dy: 0, vx: 0, width: 400, axis: 'x' as const, hasPrevious: true, hasNext: true };

    test('navigates past a quarter of the width or when fast', () => {
        expect(swipeOutcome({ ...swipe, dx: -120 })).toBe('next');
        expect(swipeOutcome({ ...swipe, dx: 120 })).toBe('previous');
        expect(swipeOutcome({ ...swipe, dx: -90 })).toBe('cancel');
        expect(swipeOutcome({ ...swipe, dx: -90, vx: -0.9 })).toBe('next');
        expect(swipeOutcome({ ...swipe, dx: 40, vx: 0.4 })).toBe('previous');
        expect(swipeOutcome({ ...swipe, dx: -90, vx: -0.2 })).toBe('cancel');
    });

    test('a flick back against the drag cancels, whatever the distance', () => {
        expect(swipeOutcome({ ...swipe, dx: -200, vx: 0.8 })).toBe('cancel');
        expect(swipeOutcome({ ...swipe, dx: 200, vx: -0.8 })).toBe('cancel');
        // A slow drift back doesn't
        expect(swipeOutcome({ ...swipe, dx: -200, vx: 0.1 })).toBe('next');
    });

    test('does nothing past the ends', () => {
        expect(swipeOutcome({ ...swipe, dx: -200, hasNext: false })).toBe('cancel');
        expect(swipeOutcome({ ...swipe, dx: 200, hasPrevious: false })).toBe('cancel');
    });

    test('a long enough swipe down closes', () => {
        expect(swipeOutcome({ ...swipe, axis: 'y', dy: SWIPE_CLOSE_DISTANCE + 1 })).toBe('close');
        expect(swipeOutcome({ ...swipe, axis: 'y', dy: SWIPE_CLOSE_DISTANCE - 1 })).toBe('cancel');
        expect(swipeOutcome({ ...swipe, axis: 'y', dy: -300 })).toBe('cancel');
    });
});

test('formatTime uses mm:ss, with hours from one hour', () => {
    const cases: [number, string][] = [[0, '00:00'], [2.7, '00:02'], [65, '01:05'], [3725, '1:02:05'], [NaN, '00:00']];
    for (const [seconds, expected] of cases) expect(formatTime(seconds), String(seconds)).toBe(expected);
});

test('hasExtension is case-insensitive and needs a real extension', () => {
    const extensions = ['jpg', 'png'];
    expect(hasExtension('a.JPG', extensions)).toBe(true);
    expect(hasExtension('archive.tar.png', extensions)).toBe(true);
    expect(hasExtension('a.heic', extensions)).toBe(false);
    expect(hasExtension('jpg', extensions)).toBe(false);
    expect(hasExtension('.png', extensions)).toBe(false);
});
