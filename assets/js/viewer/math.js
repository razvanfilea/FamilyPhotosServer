// Pure helpers of the viewer (geometry, gestures, formatting), unit-tested without a browser.

/** Release distance, as a fraction of the stage width, past which a swipe navigates */
export const SWIPE_DISTANCE = 0.25;
/** Release speed (px/ms) past which a swipe navigates whatever its distance */
export const SWIPE_VELOCITY = 0.3;
/** Downward drag (px) past which the viewer closes */
export const SWIPE_CLOSE_DISTANCE = 120;

/** The size of a `naturalWidth × naturalHeight` image fitted inside a box (object-fit: contain). */
export function fitContain(naturalWidth, naturalHeight, boxWidth, boxHeight) {
    if (!(naturalWidth > 0 && naturalHeight > 0 && boxWidth > 0 && boxHeight > 0)) {
        return { width: boxWidth, height: boxHeight };
    }
    const scale = Math.min(boxWidth / naturalWidth, boxHeight / naturalHeight);
    return { width: naturalWidth * scale, height: naturalHeight * scale };
}

/**
 * Clamps a translation so the edges of the scaled content never move inside the box: content
 * smaller than the box stays centred.
 * @param {{x: number, y: number, scale: number}} state translation from the centre, and scale
 * @param {{width: number, height: number}} content unscaled content size, centred in the box
 * @param {{width: number, height: number}} box
 */
export function clampTranslation({ x, y, scale }, content, box) {
    const maxX = Math.max(0, (content.width * scale - box.width) / 2);
    const maxY = Math.max(0, (content.height * scale - box.height) / 2);
    return {
        x: Math.min(maxX, Math.max(-maxX, x)) || 0,
        y: Math.min(maxY, Math.max(-maxY, y)) || 0,
        scale,
    };
}

/**
 * Scales about a point, keeping the content under it in place.
 * @param {{x: number, y: number, scale: number}} state
 * @param {number} scale the new scale
 * @param {{x: number, y: number}} point relative to the centre of the box
 */
export function zoomAbout(state, scale, point) {
    const ratio = scale / state.scale;
    return {
        x: point.x - (point.x - state.x) * ratio,
        y: point.y - (point.y - state.y) * ratio,
        scale,
    };
}

/**
 * What a finished touch swipe does.
 * @param {{dx: number, dy: number, duration: number, width: number, axis: 'x'|'y', hasPrevious: boolean, hasNext: boolean}} swipe
 *   `duration` in ms; `axis` is the direction the swipe was locked to. The velocity is the
 *   average over the whole gesture, not the one at release: a slow drag ending in a flick only
 *   navigates past the distance threshold.
 * @returns {'next'|'previous'|'close'|'cancel'}
 */
export function swipeOutcome({ dx, dy, duration, width, axis, hasPrevious, hasNext }) {
    if (axis === 'y') {
        return dy > SWIPE_CLOSE_DISTANCE ? 'close' : 'cancel';
    }
    const velocity = Math.abs(dx) / Math.max(duration, 1);
    const far = Math.abs(dx) > width * SWIPE_DISTANCE || velocity > SWIPE_VELOCITY;
    if (!far || dx === 0) return 'cancel';
    if (dx < 0) return hasNext ? 'next' : 'cancel';
    return hasPrevious ? 'previous' : 'cancel';
}

/** "mm:ss", or "h:mm:ss" from one hour (Android's video time format). */
export function formatTime(seconds) {
    const total = Number.isFinite(seconds) && seconds > 0 ? Math.floor(seconds) : 0;
    const hours = Math.floor(total / 3600);
    const minutes = Math.floor(total / 60) % 60;
    const pad = (n) => String(n).padStart(2, '0');
    return hours > 0 ? `${hours}:${pad(minutes)}:${pad(total % 60)}` : `${pad(minutes)}:${pad(total % 60)}`;
}

/** Whether the file can be shown by the browser, judging by its extension (`extensions` in lowercase). */
export function hasExtension(name, extensions) {
    const dot = name.lastIndexOf('.');
    return dot > 0 && extensions.includes(name.slice(dot + 1).toLowerCase());
}
