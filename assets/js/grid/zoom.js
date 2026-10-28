// Zoom of the photo grid: pinch (touch), ctrl+wheel (mouse wheel and trackpad pinch), the +/-
// keys and the zoom buttons of the top bar. The grid applies the level; this module turns input
// into levels. Zoom controls and the grid talk through `fp:grid-zoom` events with `{level}`.

import { register } from '../core/registry.js';
import { EVENTS, emit, on } from '../core/events.js';
import { register as registerShortcut, popScope, pushScope } from '../core/keyboard.js';
import { MAX_ZOOM, clampZoom, pinchStep, wheelStep } from './geometry.js';

/** Time without input after which a pinch or wheel gesture is considered over. */
const GESTURE_IDLE = 150;

const SCOPE = 'grid';

/** Open modal UI on top of the grid, which takes the ctrl+wheel itself (or leaves it to the browser) */
const MODAL = 'dialog[open], [aria-modal="true"]:not([hidden])';

/**
 * Connects zoom input to a grid.
 * @param {HTMLElement} root the grid element, which receives the touch gestures
 * @param {{getLevel: () => number, setLevel: (level: number, point?: {x: number, y: number}) => void}} grid
 * @returns {() => void} cleanup
 */
export function initZoomInput(root, { getLevel, setLevel }) {
    const cleanups = [];

    // Pinch: two touch pointers. The level is applied after a short pause or when the fingers lift
    // so the layout doesn't change under the gesture on every event.
    const touches = new Map();
    let pinch = null;
    let pinchTimer;

    const centroid = () => {
        const points = [...touches.values()];
        return {
            x: points.reduce((sum, point) => sum + point.x, 0) / points.length,
            y: points.reduce((sum, point) => sum + point.y, 0) / points.length,
        };
    };
    const distance = () => {
        const [a, b] = [...touches.values()];
        return Math.hypot(a.x - b.x, a.y - b.y);
    };
    const applyPinch = () => {
        clearTimeout(pinchTimer);
        if (pinch && pinch.level !== getLevel()) setLevel(pinch.level, pinch.point);
    };
    const endPinch = () => {
        applyPinch();
        pinch = null;
    };

    const onPointerDown = (event) => {
        if (event.pointerType !== 'touch') return;
        touches.set(event.pointerId, { x: event.clientX, y: event.clientY });
        if (touches.size === 2) {
            pinch = { scale: 1, level: getLevel(), distance: distance(), point: centroid() };
        }
    };
    const onPointerMove = (event) => {
        if (!touches.has(event.pointerId)) return;
        touches.set(event.pointerId, { x: event.clientX, y: event.clientY });
        if (!pinch || touches.size < 2) return;

        const next = distance();
        if (pinch.distance > 0 && next > 0) {
            Object.assign(pinch, pinchStep(pinch, next / pinch.distance));
        }
        pinch.distance = next;
        pinch.point = centroid();
        clearTimeout(pinchTimer);
        pinchTimer = setTimeout(applyPinch, GESTURE_IDLE);
    };
    const onPointerEnd = (event) => {
        if (!touches.delete(event.pointerId)) return;
        if (touches.size < 2) endPinch();
    };
    root.addEventListener('pointerdown', onPointerDown);
    root.addEventListener('pointermove', onPointerMove);
    root.addEventListener('pointerup', onPointerEnd);
    root.addEventListener('pointercancel', onPointerEnd);
    cleanups.push(() => {
        clearTimeout(pinchTimer);
        root.removeEventListener('pointerdown', onPointerDown);
        root.removeEventListener('pointermove', onPointerMove);
        root.removeEventListener('pointerup', onPointerEnd);
        root.removeEventListener('pointercancel', onPointerEnd);
    });

    // Ctrl/Cmd + wheel, which is also how browsers report a trackpad pinch. Not passive, so the
    // browser's own page zoom can be prevented. Only over the grid and while nothing modal (a
    // dialog, the viewer with its own zoom) covers it.
    let wheelAccumulated = 0;
    let wheelTimer;
    const onWheel = (event) => {
        if (!event.ctrlKey && !event.metaKey) return;
        if (root.closest('[inert]') || document.querySelector(MODAL)) return;
        event.preventDefault();
        const result = wheelStep(wheelAccumulated, event.deltaY);
        wheelAccumulated = result.accumulated;
        clearTimeout(wheelTimer);
        wheelTimer = setTimeout(() => { wheelAccumulated = 0; }, GESTURE_IDLE * 2);
        if (result.step !== 0) {
            setLevel(clampZoom(getLevel() + result.step), { x: event.clientX, y: event.clientY });
        }
    };
    root.addEventListener('wheel', onWheel, { passive: false });
    cleanups.push(() => {
        clearTimeout(wheelTimer);
        root.removeEventListener('wheel', onWheel);
    });

    // Keys: "+" zooms in (fewer, larger tiles), "-" out
    pushScope(SCOPE);
    cleanups.push(
        registerShortcut(SCOPE, '+', () => setLevel(clampZoom(getLevel() - 1))),
        registerShortcut(SCOPE, '=', () => setLevel(clampZoom(getLevel() - 1))),
        registerShortcut(SCOPE, '-', () => setLevel(clampZoom(getLevel() + 1))),
        () => popScope(SCOPE),
    );

    return () => cleanups.forEach((cleanup) => cleanup());
}

/** The − / + buttons: ask for a level with `fp:grid-zoom` and follow the level the grid reports. */
register('zoom-controls', (element) => {
    const zoomOut = element.querySelector('[data-action="zoom-out"]');
    const zoomIn = element.querySelector('[data-action="zoom-in"]');
    let level = Number(element.dataset.zoom);

    const update = () => {
        zoomIn.disabled = level <= 0;
        zoomOut.disabled = level >= MAX_ZOOM;
    };
    const request = (newLevel) => emit(document, EVENTS.gridZoom, { level: clampZoom(newLevel) });

    zoomIn.addEventListener('click', () => request(level - 1));
    zoomOut.addEventListener('click', () => request(level + 1));
    update();

    return on(document, EVENTS.gridZoom, (event) => {
        level = event.detail.level;
        update();
    });
});
