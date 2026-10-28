// Unit tests of the pure grid helpers, run without a browser.
import { expect, test } from '@playwright/test';
import {
    JUMP_OFFSET,
    findItemAt,
    navigateIndex,
    pinchStep,
    scrollTopForSection,
    sectionForIndex,
    sectionForProgress,
    tilesNeeded,
    wheelStep,
} from '../../assets/js/grid/geometry.js';

test.describe('pinchStep', () => {
    test('a spreading pinch past 1.8 zooms in (fewer columns)', () => {
        expect(pinchStep({ scale: 1, level: 2 }, 1.5)).toEqual({ scale: 1.5, level: 2 });
        const result = pinchStep({ scale: 1.5, level: 2 }, 1.4);
        expect(result.level).toBe(1);
        expect(result.scale).toBeCloseTo(2.1 / 1.8);
    });

    test('a closing pinch past 0.6 zooms out (more columns)', () => {
        expect(pinchStep({ scale: 1, level: 1 }, 0.7)).toEqual({ scale: 0.7, level: 1 });
        const result = pinchStep({ scale: 0.7, level: 1 }, 0.8);
        expect(result.level).toBe(2);
        expect(result.scale).toBeCloseTo(0.56 / 0.6);
    });

    test('one long pinch steps several levels and keeps the leftover', () => {
        const result = pinchStep({ scale: 1, level: 3 }, 1.8 * 1.8 * 1.2);
        expect(result.level).toBe(1);
        expect(result.scale).toBeCloseTo(1.2);

        expect(pinchStep({ scale: 1, level: 0 }, 0.6 * 0.6 * 0.9).level).toBe(2);
    });

    test('stays within the levels', () => {
        expect(pinchStep({ scale: 1, level: 0 }, 10).level).toBe(0);
        expect(pinchStep({ scale: 1, level: 3 }, 0.01).level).toBe(3);
    });
});

test('wheelStep accumulates small trackpad deltas and steps once per mouse notch', () => {
    expect(wheelStep(0, 100)).toEqual({ accumulated: 0, step: 1 });
    expect(wheelStep(0, -100)).toEqual({ accumulated: 0, step: -1 });
    let result = wheelStep(0, -20);
    expect(result).toEqual({ accumulated: -20, step: 0 });
    result = wheelStep(result.accumulated, -20);
    result = wheelStep(result.accumulated, -20);
    expect(result).toEqual({ accumulated: 0, step: -1 });
});

const items = [
    { top: 0, end: 100 },
    { top: 100, end: 400 },
    { top: 400, end: 450 },
    { top: 450, end: 1000 },
];

test('findItemAt', () => {
    expect(findItemAt([], 5)).toBe(-1);
    expect(findItemAt(items, -50)).toBe(0);
    expect(findItemAt(items, 0)).toBe(0);
    expect(findItemAt(items, 99.9)).toBe(0);
    expect(findItemAt(items, 100)).toBe(1);
    expect(findItemAt(items, 449)).toBe(2);
    expect(findItemAt(items, 450)).toBe(3);
    expect(findItemAt(items, 5000)).toBe(3);
});

test.describe('sectionForProgress', () => {
    test('the ends of the track are the first and last month', () => {
        expect(sectionForProgress(items, 0, 500, 500, 0)).toBe(0);
        // Even if the last month is shorter than the part of the viewport that would pick it
        expect(sectionForProgress(items, 1, 500, 500, 0)).toBe(3);
        expect(sectionForProgress([], 0.5, 500, 500, 0)).toBe(-1);
    });

    test('in between, the month at 20% of the viewport after scrolling there', () => {
        // progress 0.3 → scrollY 150, probe 150 + 100 = 250
        expect(sectionForProgress(items, 0.3, 500, 500, 0)).toBe(1);
        // progress 0.5 → probe 350
        expect(sectionForProgress(items, 0.5, 500, 500, 0)).toBe(1);
        // progress 0.7 → probe 450
        expect(sectionForProgress(items, 0.7, 500, 500, 0)).toBe(3);
        // The grid starting below the page top shifts the probe
        expect(sectionForProgress(items, 0.5, 500, 500, 300)).toBe(0);
    });
});

test('scrollTopForSection puts the header at 20% from the top, never above the page', () => {
    expect(JUMP_OFFSET).toBe(0.2);
    expect(scrollTopForSection(1000, 300, 800)).toBe(1140);
    expect(scrollTopForSection(0, 0, 800)).toBe(0);
});

test('tilesNeeded covers the rows down to the bottom', () => {
    const metrics = { cell: 100, gap: 2, cols: 4 };
    const section = { gridTop: 50, count: 100 };
    expect(tilesNeeded(section, metrics, 40)).toBe(0);
    expect(tilesNeeded(section, metrics, 50)).toBe(0);
    expect(tilesNeeded(section, metrics, 51)).toBe(4);
    // 102 px per row: 2 rows end at 50 + 202
    expect(tilesNeeded(section, metrics, 50 + 202)).toBe(8);
    expect(tilesNeeded(section, metrics, 50 + 203)).toBe(12);
    expect(tilesNeeded(section, metrics, 1e9)).toBe(100);
});

const sections = [
    { start: 0, count: 5 },
    { start: 5, count: 1 },
    { start: 6, count: 9 },
];

test('sectionForIndex', () => {
    expect(sectionForIndex(sections, -1)).toBe(-1);
    expect(sectionForIndex(sections, 0)).toBe(0);
    expect(sectionForIndex(sections, 4)).toBe(0);
    expect(sectionForIndex(sections, 5)).toBe(1);
    expect(sectionForIndex(sections, 6)).toBe(2);
    expect(sectionForIndex(sections, 14)).toBe(2);
    expect(sectionForIndex(sections, 15)).toBe(-1);
    expect(sectionForIndex([], 0)).toBe(-1);
});

test.describe('navigateIndex with 4 columns', () => {
    const go = (index: number, key: string, pageRows = 2) => navigateIndex(sections, 4, index, key, pageRows);

    test('left and right walk through the whole context', () => {
        expect(go(0, 'ArrowLeft')).toBeNull();
        expect(go(4, 'ArrowRight')).toBe(5);
        expect(go(5, 'ArrowRight')).toBe(6);
        expect(go(6, 'ArrowLeft')).toBe(5);
        expect(go(14, 'ArrowRight')).toBeNull();
    });

    test('down and up stay in the column and cross sections', () => {
        // Section 0: row 0 is [0..3], row 1 only has index 4
        expect(go(0, 'ArrowDown')).toBe(4);
        expect(go(4, 'ArrowUp')).toBe(0);
        // Section 0 has rows [0..3] and [4]; from column 2 down there is no tile below: last tile
        expect(go(2, 'ArrowDown')).toBe(4);
        // From the last row of section 0 to the single photo of section 1
        expect(go(4, 'ArrowDown')).toBe(5);
        // Section 1 (one photo) to section 2: first row, same column clamped
        expect(go(5, 'ArrowDown')).toBe(6);
        // Up from section 2's first row goes to the last row of section 1
        expect(go(8, 'ArrowUp')).toBe(5);
        // Up from section 1 to the last row of section 0, column 0
        expect(go(5, 'ArrowUp')).toBe(4);
        // Within section 2: rows [6..9], [10..13], [14]
        expect(go(7, 'ArrowDown')).toBe(11);
        expect(go(11, 'ArrowDown')).toBe(14);
        expect(go(14, 'ArrowDown')).toBeNull();
        expect(go(0, 'ArrowUp')).toBeNull();
    });

    test('Home, End and pages', () => {
        expect(go(9, 'Home')).toBe(0);
        expect(go(0, 'Home')).toBeNull();
        expect(go(3, 'End')).toBe(14);
        expect(go(14, 'End')).toBeNull();
        expect(go(10, 'PageDown', 1)).toBe(14);
        expect(go(10, 'PageUp', 2)).toBe(2);
        expect(go(1, 'PageUp')).toBe(0);
    });

    test('ignores other keys and unknown positions', () => {
        expect(go(3, 'a')).toBeNull();
        expect(go(99, 'ArrowLeft')).toBeNull();
        expect(navigateIndex([], 4, 0, 'ArrowRight')).toBeNull();
    });
});
