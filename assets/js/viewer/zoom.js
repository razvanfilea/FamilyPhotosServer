// Zoom and pan of an image: pinch, double-tap / double-click, ctrl+wheel (and plain wheel when
// zoomed or over the image), drag to pan when zoomed. Transform based (translate3d + scale).
// Used by the viewer, and reusable on any image (e.g. the duplicates page):
//
//     const zoom = createZoom(container, { target: img });
//
// The target must fill the container (centred); its content is fitted inside it (object-fit:
// contain for an <img>, or the size given by `contentSize`). Single taps are reported through
// `onTap` once they can't be a double tap anymore.

import { clampTranslation, fitContain, zoomAbout } from './math.js';

const DOUBLE_TAP_SCALE = 2.5;
const MIN_MAX_SCALE = 4;
const DOUBLE_TAP_DELAY = 300;
const DOUBLE_TAP_DISTANCE = 30;
const TAP_SLOP = 10;
const TAP_DURATION = 500;
const ANIMATION_MS = 200;

/** Elements whose pointer events are theirs, not gestures */
const CONTROLS = 'button, a, input, select, textarea, summary, details, [data-no-gesture]';

const reducedMotion = () => matchMedia('(prefers-reduced-motion: reduce)').matches;

/**
 * @param {HTMLElement} container receives the pointer and wheel events
 * @param {{target?: HTMLElement|null, contentSize?: () => ({width: number, height: number}|null), naturalWidth?: () => number, onTap?: (event: PointerEvent) => void, onChange?: (scale: number) => void}} options
 *   `contentSize`: the unscaled size of the content inside the target (default: the target's
 *   <img> fitted with contain, else the target's size); `naturalWidth`: the image's full
 *   resolution, for the maximum scale (default: the <img>'s naturalWidth).
 */
export function createZoom(container, options = {}) {
    let { target = null, contentSize, naturalWidth } = options;
    let state = { x: 0, y: 0, scale: 1 };
    /** @type {Map<number, {x: number, y: number, startX: number, startY: number, time: number}>} */
    const pointers = new Map();
    let gesture = null;
    let tapCandidate = false;
    let lastTap = null;
    let tapTimer;
    let animationTimer;

    const box = () => ({ width: container.clientWidth, height: container.clientHeight });

    function imageOf(element) {
        if (!element) return null;
        return element instanceof HTMLImageElement ? element : element.querySelector('img');
    }

    function content() {
        const size = contentSize?.();
        if (size) return size;
        const image = imageOf(target);
        const { width, height } = box();
        return image ? fitContain(image.naturalWidth, image.naturalHeight, width, height) : { width, height };
    }

    function maxScale() {
        const natural = naturalWidth?.() ?? imageOf(target)?.naturalWidth ?? 0;
        const displayed = content().width;
        return Math.max(MIN_MAX_SCALE, displayed > 0 ? (natural / displayed) * 2 : 0);
    }

    /** A client point relative to the centre of the container */
    function fromCenter(clientX, clientY) {
        const rect = container.getBoundingClientRect();
        return { x: clientX - rect.left - rect.width / 2, y: clientY - rect.top - rect.height / 2 };
    }

    function isOverContent(clientX, clientY) {
        const point = fromCenter(clientX, clientY);
        const size = content();
        return Math.abs(point.x - state.x) <= (size.width * state.scale) / 2
            && Math.abs(point.y - state.y) <= (size.height * state.scale) / 2;
    }

    function apply(next, { animate = false, clamp = true } = {}) {
        state = clamp ? clampTranslation(next, content(), box()) : next;
        if (!target) return;
        clearTimeout(animationTimer);
        if (animate && !reducedMotion()) {
            target.style.transition = `transform ${ANIMATION_MS}ms ease-out`;
            animationTimer = setTimeout(() => { target.style.transition = ''; }, ANIMATION_MS);
        } else {
            target.style.transition = '';
        }
        target.style.transform = state.scale === 1 && state.x === 0 && state.y === 0
            ? ''
            : `translate3d(${state.x}px, ${state.y}px, 0) scale(${state.scale})`;
        options.onChange?.(state.scale);
    }

    /** Zooms to `scale` about a client point (default: the centre). */
    function zoomTo(scale, clientPoint, { animate = false } = {}) {
        if (!target) return;
        const clamped = Math.min(maxScale(), Math.max(1, scale));
        const point = clientPoint ? fromCenter(clientPoint.x, clientPoint.y) : { x: 0, y: 0 };
        apply(zoomAbout(state, clamped, point), { animate });
    }

    function reset({ animate = false } = {}) {
        apply({ x: 0, y: 0, scale: 1 }, { animate });
    }

    // ---------- Pointers ----------

    function pinchInfo() {
        const [a, b] = [...pointers.values()];
        return {
            distance: Math.hypot(a.x - b.x, a.y - b.y),
            mid: { x: (a.x + b.x) / 2, y: (a.y + b.y) / 2 },
        };
    }

    function startGesture() {
        if (!target) {
            gesture = null;
            return;
        }
        if (pointers.size >= 2) {
            const { distance, mid } = pinchInfo();
            gesture = { type: 'pinch', distance, mid, state: { ...state } };
        } else if (pointers.size === 1 && state.scale > 1) {
            const [pointer] = pointers.values();
            gesture = { type: 'pan', x: pointer.x, y: pointer.y, state: { ...state } };
        } else {
            gesture = null;
        }
    }

    function onPointerDown(event) {
        if (event.button > 0 || /** @type {Element} */ (event.target).closest(CONTROLS)) return;
        pointers.set(event.pointerId, {
            x: event.clientX, y: event.clientY, startX: event.clientX, startY: event.clientY, time: event.timeStamp,
        });
        tapCandidate = pointers.size === 1;
        startGesture();
        if (gesture) container.setPointerCapture?.(event.pointerId);
    }

    function onPointerMove(event) {
        const pointer = pointers.get(event.pointerId);
        if (!pointer) return;
        pointer.x = event.clientX;
        pointer.y = event.clientY;
        if (Math.hypot(pointer.x - pointer.startX, pointer.y - pointer.startY) > TAP_SLOP) tapCandidate = false;

        if (gesture?.type === 'pinch' && pointers.size >= 2) {
            const { distance, mid } = pinchInfo();
            const scale = Math.min(maxScale(), Math.max(0.8, gesture.state.scale * (distance / gesture.distance)));
            const start = fromCenter(gesture.mid.x, gesture.mid.y);
            const zoomed = zoomAbout(gesture.state, scale, start);
            apply({ ...zoomed, x: zoomed.x + mid.x - gesture.mid.x, y: zoomed.y + mid.y - gesture.mid.y },
                { clamp: scale >= 1 });
        } else if (gesture?.type === 'pan') {
            apply({ ...gesture.state, x: gesture.state.x + pointer.x - gesture.x, y: gesture.state.y + pointer.y - gesture.y });
        }
    }

    function onPointerUp(event) {
        const pointer = pointers.get(event.pointerId);
        if (!pointer) return;
        pointers.delete(event.pointerId);
        const wasPinch = gesture?.type === 'pinch';

        if (event.type === 'pointerup' && tapCandidate && pointers.size === 0
            && event.timeStamp - pointer.time < TAP_DURATION) {
            handleTap(event);
        }
        if (pointers.size === 0) tapCandidate = false;

        if (wasPinch && state.scale < 1) {
            reset({ animate: true });
        }
        // The remaining finger keeps panning
        startGesture();
    }

    function handleTap(event) {
        clearTimeout(tapTimer);
        const point = { x: event.clientX, y: event.clientY };
        const isDouble = lastTap && event.timeStamp - lastTap.time < DOUBLE_TAP_DELAY
            && Math.hypot(point.x - lastTap.x, point.y - lastTap.y) < DOUBLE_TAP_DISTANCE;

        if (isDouble && target) {
            lastTap = null;
            if (state.scale > 1) {
                reset({ animate: true });
            } else {
                zoomTo(DOUBLE_TAP_SCALE, point, { animate: true });
            }
            return;
        }
        if (!target) {
            // Nothing to zoom: no need to wait for a second tap
            lastTap = null;
            options.onTap?.(event);
            return;
        }
        lastTap = { ...point, time: event.timeStamp };
        tapTimer = setTimeout(() => {
            lastTap = null;
            options.onTap?.(event);
        }, DOUBLE_TAP_DELAY);
    }

    function onWheel(event) {
        if (!target || /** @type {Element} */ (event.target).closest(CONTROLS)) return;
        const pinch = event.ctrlKey || event.metaKey;
        if (!pinch && state.scale <= 1 && !isOverContent(event.clientX, event.clientY)) return;
        event.preventDefault();
        const delta = event.deltaMode === WheelEvent.DOM_DELTA_LINE ? event.deltaY * 16 : event.deltaY;
        const factor = Math.exp(-delta * (pinch ? 0.01 : 0.002));
        zoomTo(state.scale * factor, { x: event.clientX, y: event.clientY });
    }

    const abort = new AbortController();
    const listen = { signal: abort.signal };
    container.addEventListener('pointerdown', onPointerDown, listen);
    container.addEventListener('pointermove', onPointerMove, listen);
    container.addEventListener('pointerup', onPointerUp, listen);
    container.addEventListener('pointercancel', onPointerUp, listen);
    container.addEventListener('wheel', onWheel, { passive: false, ...listen });

    return {
        get scale() {
            return state.scale;
        },
        isZoomed: () => state.scale > 1.01,
        /** Whether a pinch or a pan is in progress */
        isGesturing: () => gesture !== null,
        pointerCount: () => pointers.size,
        /** Switches to another target (or none), resetting the zoom */
        setTarget(element, sizes = {}) {
            if (target) {
                target.style.transform = '';
                target.style.transition = '';
            }
            target = element;
            contentSize = sizes.contentSize ?? options.contentSize;
            naturalWidth = sizes.naturalWidth ?? options.naturalWidth;
            state = { x: 0, y: 0, scale: 1 };
            gesture = null;
            lastTap = null;
            clearTimeout(tapTimer);
            options.onChange?.(1);
        },
        zoomTo,
        zoomBy: (factor, clientPoint) => zoomTo(state.scale * factor, clientPoint, { animate: true }),
        reset,
        /** Re-clamps after the container or the content changed size */
        refresh: () => apply(state),
        destroy() {
            abort.abort();
            clearTimeout(tapTimer);
            clearTimeout(animationTimer);
        },
    };
}
