// Custom events shared between modules.
// All events bubble and carry their data in `detail`.

export const EVENTS = Object.freeze({
    /** {photoId, index, contextKey, tile} — cancelable; selection mode calls preventDefault() */
    photoActivate: 'fp:photo-activate',
    /** {count, ids: Set} */
    selectionChange: 'fp:selection-change',
    /** {ids: number[], contextKey?} */
    photosRemoved: 'fp:photos-removed',
    /** {photos: PhotoJson[]} */
    photosChanged: 'fp:photos-changed',
    /** {ids: number[], contextKey?} — photos came (back) into contexts, e.g. Undo of a trash: grids reload */
    photosAdded: 'fp:photos-added',
    /** {level} */
    gridZoom: 'fp:grid-zoom',
    /** {section, month, tiles: Element[]} */
    sectionLoaded: 'fp:section-loaded',
    /** {photoId, contextKey, index?} — index only when the viewer walked the page grid's context */
    viewerClosed: 'fp:viewer-closed',
    /** {message, type} — also triggered by the server with the `HX-Trigger` header */
    toast: 'fp:toast',
});

/**
 * @returns {CustomEvent} the event, so callers can check `defaultPrevented`
 */
export function emit(target, name, detail = {}, { cancelable = false } = {}) {
    const event = new CustomEvent(name, { detail, bubbles: true, cancelable });
    target.dispatchEvent(event);
    return event;
}

/**
 * @returns {() => void} a function removing the listener
 */
export function on(target, name, handler, options) {
    target.addEventListener(name, handler, options);
    return () => target.removeEventListener(name, handler, options);
}
