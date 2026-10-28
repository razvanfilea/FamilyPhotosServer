// Zoom of the photo grid: the zoom buttons of the top bar, ctrl+wheel (mouse wheel and trackpad
// pinch) and the +/- keys. The grid applies the level; this module turns input into levels.
// Zoom controls and the grid talk through `fp:grid-zoom` events with `{level}`.

import { register } from '../core/registry.js';
import { EVENTS, emit, on } from '../core/events.js';
import { register as registerShortcut, popScope, pushScope } from '../core/keyboard.js';
import { MAX_ZOOM, clampZoom, wheelStep } from './geometry.js';

/** Time without input after which a wheel gesture is considered over. */
const WHEEL_IDLE = 300;

const SCOPE = 'grid';

/** Open modal UI on top of the grid, which takes the ctrl+wheel itself (or leaves it to the browser) */
const MODAL = 'dialog[open], [aria-modal="true"]:not([hidden])';

/**
 * Connects zoom input to a grid.
 * @param {HTMLElement} root the grid element, which receives the wheel events
 * @param {{getLevel: () => number, setLevel: (level: number, point?: {x: number, y: number}) => void}} grid
 * @returns {() => void} cleanup
 */
export function initZoomInput(root, { getLevel, setLevel }) {
    const cleanups = [];

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
        wheelTimer = setTimeout(() => { wheelAccumulated = 0; }, WHEEL_IDLE);
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
