// Drag-select (Android's `photoGridDrag`): long-press a tile with a finger or pen, or press the
// mouse on a check circle and move. The selection is the selection from before the drag plus the
// contiguous range from the tile where it started to the one under the pointer, so dragging back
// shrinks it. Near the top and bottom of the screen the page scrolls.

import { autoScrollSpeed } from '../selection/math.js';

const TILE = '.photo-tile';
const LONG_PRESS = 400;
/** Movement that cancels a long-press (it is a scroll or a swipe) */
const PRESS_SLOP = 10;
/** Movement of the mouse from a check circle that starts the drag */
const MOUSE_SLOP = 4;
/** The auto-scroll speed is `depth` px per this many ms */
const SCROLL_STEP = 10;
/** Clicks right after a drag or long-press belong to it, for this many ms */
const CLICK_GRACE = 400;

/** The part of the viewport where the pointer doesn't scroll the page: between the top bar and the bottom nav */
function visibleBand() {
    const topbar = document.querySelector('.fp-topbar');
    const nav = document.querySelector('.fp-bottom-nav');
    const top = topbar ? Math.max(0, topbar.getBoundingClientRect().bottom) : 0;
    const bottom = nav && getComputedStyle(nav).display !== 'none'
        ? nav.getBoundingClientRect().top
        : window.innerHeight;
    return { top, bottom };
}

/**
 * @param {HTMLElement} root the photo grid
 * @param {{begin: (tile: Element) => {base: Map, anchor: number}, extend: (base: Map, anchor: number, current: number) => void, swallowClicks: (ms: number) => void}} host
 * @returns {() => void} cleanup
 */
export function initDrag(root, host) {
    const abort = new AbortController();
    const listen = { signal: abort.signal };

    /** A press that may become a drag: {pointerId, x, y, tile, timer, kind} */
    let pending = null;
    /** @type {{pointerId: number, base: Map, anchor: number, current: number, x: number, y: number, frame: number, last: number} | null} */
    let drag = null;

    function cancelPending() {
        clearTimeout(pending?.timer);
        pending = null;
    }

    function hit() {
        const tile = document.elementFromPoint(drag.x, drag.y)?.closest?.(TILE);
        if (!tile || !root.contains(tile)) return;
        const index = Number(tile.dataset.index);
        if (index === drag.current) return;
        drag.current = index;
        host.extend(drag.base, drag.anchor, index);
    }

    function tick(now) {
        if (!drag) return;
        const { top, bottom } = visibleBand();
        const speed = autoScrollSpeed(drag.y, top, bottom);
        if (speed !== 0) {
            window.scrollBy({ top: speed * (now - drag.last) / SCROLL_STEP, behavior: 'instant' });
            // The tile under a still pointer changes while the page scrolls
            hit();
        }
        drag.last = now;
        drag.frame = requestAnimationFrame(tick);
    }

    function startDrag() {
        const { pointerId, x, y, tile } = pending;
        cancelPending();
        const { base, anchor } = host.begin(tile);
        drag = { pointerId, base, anchor, current: anchor, x, y, frame: 0, last: performance.now() };
        root.classList.add('is-dragging');
        navigator.vibrate?.(10);
        host.swallowClicks(Infinity);
        drag.frame = requestAnimationFrame(tick);
    }

    function endDrag() {
        if (!drag) return;
        cancelAnimationFrame(drag.frame);
        drag = null;
        root.classList.remove('is-dragging');
        host.swallowClicks(CLICK_GRACE);
    }

    root.addEventListener('pointerdown', (event) => {
        const target = /** @type {Element} */ (event.target);
        const tile = target.closest(TILE);
        // A second finger is a pinch
        if (pending && pending.pointerId !== event.pointerId) {
            cancelPending();
            return;
        }
        if (!tile || drag || (event.pointerType === 'mouse' && event.button !== 0)) return;

        const mouse = event.pointerType === 'mouse';
        // The mouse drags from the check circle (the rest of the tile is a click); fingers long-press anywhere
        if (mouse && !target.closest('.photo-tile__check')) return;
        cancelPending();
        pending = { pointerId: event.pointerId, x: event.clientX, y: event.clientY, tile, kind: mouse ? 'mouse' : 'press' };
        if (!mouse) pending.timer = setTimeout(startDrag, LONG_PRESS);
    }, listen);

    document.addEventListener('pointermove', (event) => {
        if (drag?.pointerId === event.pointerId) {
            drag.x = event.clientX;
            drag.y = event.clientY;
            hit();
        } else if (pending?.pointerId === event.pointerId) {
            const moved = Math.hypot(event.clientX - pending.x, event.clientY - pending.y);
            if (pending.kind === 'mouse' && moved > MOUSE_SLOP) {
                pending.x = event.clientX;
                pending.y = event.clientY;
                startDrag();
            } else if (pending.kind === 'press' && moved > PRESS_SLOP) {
                cancelPending();
            }
        }
    }, listen);

    const release = (event) => {
        if (drag?.pointerId === event.pointerId) endDrag();
        if (pending?.pointerId === event.pointerId) cancelPending();
    };
    document.addEventListener('pointerup', release, listen);
    document.addEventListener('pointercancel', release, listen);

    // A finger that drags must not scroll the page (touch-action is only read when the touch starts)
    root.addEventListener('touchmove', (event) => {
        if (drag && event.cancelable) event.preventDefault();
    }, { passive: false, ...listen });

    return () => {
        abort.abort();
        cancelPending();
        endDrag();
    };
}
