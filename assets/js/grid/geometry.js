// Pure helpers of the photo grid: layout maths, zoom gestures and keyboard navigation. They have
// no DOM dependencies so they can be unit tested (playwright/specs/unit-grid.spec.ts).

export const MAX_ZOOM = 3;

/** Accumulated pinch scale beyond which the zoom level steps (Android's detectZoomIn). */
const PINCH_ZOOM_IN = 1.8;
const PINCH_ZOOM_OUT = 0.6;

/** Accumulated ctrl+wheel delta that makes one zoom step. A mouse wheel notch is 100. */
const WHEEL_STEP_DELTA = 50;

/** Where a month header lands when jumping to it, as a fraction of the viewport height. */
export const JUMP_OFFSET = 0.2;

export const clampZoom = (level) => Math.min(MAX_ZOOM, Math.max(0, level));

/**
 * One pinch event. A spreading pinch (scale > 1) zooms in, which means fewer columns, so a lower
 * level. The leftover scale is kept, so one long pinch can step several levels.
 * @param {{scale: number, level: number}} state
 * @param {number} ratio change of the distance between the fingers since the last event
 * @returns {{scale: number, level: number}}
 */
export function pinchStep({ scale, level }, ratio) {
    scale *= ratio;
    while (scale > PINCH_ZOOM_IN) {
        level = clampZoom(level - 1);
        scale /= PINCH_ZOOM_IN;
    }
    while (scale < PINCH_ZOOM_OUT) {
        level = clampZoom(level + 1);
        scale /= PINCH_ZOOM_OUT;
    }
    return { scale, level };
}

/**
 * One ctrl+wheel event (also what a trackpad pinch looks like). Negative deltas zoom in.
 * @returns {{accumulated: number, step: -1 | 0 | 1}} step is the change of the zoom level
 */
export function wheelStep(accumulated, deltaY) {
    accumulated += deltaY;
    if (accumulated <= -WHEEL_STEP_DELTA) return { accumulated: 0, step: -1 };
    if (accumulated >= WHEEL_STEP_DELTA) return { accumulated: 0, step: 1 };
    return { accumulated, step: 0 };
}

/**
 * Index of the item whose `[top, end)` range contains `y`; before the first and after the last
 * item it is the first and last. -1 for no items.
 * @param {{top: number, end: number}[]} items sorted by position
 */
export function findItemAt(items, y) {
    if (items.length === 0) return -1;
    let low = 0;
    let high = items.length - 1;
    while (low < high) {
        const middle = (low + high + 1) >> 1;
        if (items[middle].top <= y) low = middle;
        else high = middle - 1;
    }
    return low;
}

/**
 * The section the fast scroller jumps to for a drag position, like Android: the ends of the track
 * are the first and last month, in between the section at `JUMP_OFFSET` of the viewport after
 * scrolling to the proportional position.
 * @param {{top: number, end: number}[]} items sections, positions relative to the grid
 * @param {number} progress 0-1 position of the thumb on the track
 * @param {number} viewportHeight
 * @param {number} maxScroll document height minus viewport height
 * @param {number} gridTop document position of the grid
 */
export function sectionForProgress(items, progress, viewportHeight, maxScroll, gridTop) {
    if (items.length === 0) return -1;
    if (progress <= 0) return 0;
    if (progress >= 1) return items.length - 1;
    const probe = progress * maxScroll + JUMP_OFFSET * viewportHeight - gridTop;
    return findItemAt(items, probe);
}

/** The scroll position placing a section's top at `JUMP_OFFSET` of the viewport. */
export function scrollTopForSection(sectionTop, gridTop, viewportHeight) {
    return Math.max(0, Math.round(gridTop + sectionTop - JUMP_OFFSET * viewportHeight));
}

/**
 * How many tiles (from the start of a section) are needed to cover everything down to `bottom`.
 * @param {{gridTop: number, count: number}} section positions relative to the grid
 * @param {{cell: number, gap: number, cols: number}} metrics
 */
export function tilesNeeded(section, { cell, gap, cols }, bottom) {
    const visibleHeight = bottom - section.gridTop;
    if (visibleHeight <= 0) return 0;
    const rows = Math.ceil((visibleHeight + gap) / (cell + gap));
    return Math.min(section.count, rows * cols);
}

/** Index of the section containing the photo at context `index`, -1 if there is none. */
export function sectionForIndex(sections, index) {
    if (index < 0) return -1;
    let low = 0;
    let high = sections.length - 1;
    while (low <= high) {
        const middle = (low + high) >> 1;
        const { start, count } = sections[middle];
        if (index < start) high = middle - 1;
        else if (index >= start + count) low = middle + 1;
        else return middle;
    }
    return -1;
}

/**
 * The context index to focus after a navigation key, or null when the key doesn't move.
 * Vertical movement stays in the column of the section's rows; leaving a section goes to the last
 * or first row of the neighbouring one.
 * @param {{start: number, count: number}[]} sections
 * @param {number} cols
 * @param {number} index current context index
 * @param {string} key an arrow key, Home, End, PageUp or PageDown
 * @param {number} pageRows rows per screen, for PageUp/PageDown
 */
export function navigateIndex(sections, cols, index, key, pageRows = 1) {
    const total = sections.reduce((sum, section) => sum + section.count, 0);
    if (total === 0) return null;
    const current = sectionForIndex(sections, index);
    if (current < 0) return null;

    let target;
    switch (key) {
        case 'ArrowLeft': target = index - 1; break;
        case 'ArrowRight': target = index + 1; break;
        case 'Home': target = 0; break;
        case 'End': target = total - 1; break;
        case 'PageUp': target = index - pageRows * cols; break;
        case 'PageDown': target = index + pageRows * cols; break;
        case 'ArrowUp': target = verticalTarget(sections, cols, current, index, -1); break;
        case 'ArrowDown': target = verticalTarget(sections, cols, current, index, 1); break;
        default: return null;
    }

    target = Math.min(total - 1, Math.max(0, target));
    return target === index ? null : target;
}

function verticalTarget(sections, cols, currentSection, index, direction) {
    const { start, count } = sections[currentSection];
    const local = index - start;
    const column = local % cols;

    if (direction < 0) {
        if (local - cols >= 0) return index - cols;
        const previous = sections[currentSection - 1];
        if (!previous) return index;
        const lastRowStart = Math.floor((previous.count - 1) / cols) * cols;
        return previous.start + Math.min(lastRowStart + column, previous.count - 1);
    }

    if (local + cols < count) return index + cols;
    // Not in the last row, but the row below is shorter: go to its last tile
    if (Math.floor(local / cols) < Math.floor((count - 1) / cols)) return start + count - 1;
    const next = sections[currentSection + 1];
    if (!next) return index;
    return next.start + Math.min(column, next.count - 1);
}
