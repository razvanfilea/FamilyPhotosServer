// Unit tests of the pure viewer helpers, run without a browser.
import { expect, test } from '@playwright/test';
import {
    SWIPE_CLOSE_DISTANCE,
    clampTranslation,
    fitContain,
    formatTime,
    hasExtension,
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

test.describe('swipeOutcome', () => {
    const swipe = { dx: 0, dy: 0, duration: 500, width: 400, axis: 'x' as const, hasPrevious: true, hasNext: true };

    test('navigates past a quarter of the width or when fast', () => {
        expect(swipeOutcome({ ...swipe, dx: -120 })).toBe('next');
        expect(swipeOutcome({ ...swipe, dx: 120 })).toBe('previous');
        expect(swipeOutcome({ ...swipe, dx: -90 })).toBe('cancel');
        expect(swipeOutcome({ ...swipe, dx: -90, duration: 100 })).toBe('next');
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
