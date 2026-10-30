// Pure helpers of the selection (no DOM), unit-tested in playwright/specs/unit-selection.spec.ts.

/** Photos per `GET /api/context/neighbors?start&limit` request when resolving a range */
const RESOLVE_CHUNK = 100;
/** Ids per `POST /api/trash` / `/api/trash/restore` request */
export const BULK_CHUNK = 500;
/** The auto-scroll zone at the viewport's top and bottom while drag-selecting, in px */
const AUTO_SCROLL_ZONE = 40;
/** Files shared at once with the Web Share API */
export const MAX_SHARE = 10;
/** Files downloaded at once without asking */
export const MAX_QUIET_DOWNLOADS = 20;

/** The indices from `a` to `b`, whichever is larger */
export function rangeIndices(a, b) {
    const low = Math.min(a, b);
    const high = Math.max(a, b);
    return Array.from({ length: high - low + 1 }, (_, i) => low + i);
}

/**
 * The runs of consecutive indices that aren't known, split in `{start, limit}` requests of at most
 * `chunk` photos.
 * @param {number[]} indices sorted ascending
 * @param {(index: number) => boolean} isKnown
 */
export function missingRequests(indices, isKnown, chunk = RESOLVE_CHUNK) {
    const requests = [];
    let run = null;
    for (const index of indices) {
        if (isKnown(index)) {
            run = null;
        } else if (run && run.start + run.limit === index && run.limit < chunk) {
            run.limit += 1;
        } else {
            run = { start: index, limit: 1 };
            requests.push(run);
        }
    }
    return requests;
}

/**
 * How far to scroll per 10 ms while the pointer is inside the zone at the top or bottom of the
 * visible area: the depth in px, negative when scrolling up.
 * @param {number} y pointer position
 * @param {number} top where the visible area starts (below the top bar)
 * @param {number} bottom where it ends (above the bottom nav)
 */
export function autoScrollSpeed(y, top, bottom, zone = AUTO_SCROLL_ZONE) {
    if (y < top + zone) return -Math.min(zone, top + zone - y);
    if (y > bottom - zone) return Math.min(zone, y - (bottom - zone));
    return 0;
}

/** Splits `items` in arrays of at most `size` */
export function chunk(items, size) {
    const chunks = [];
    for (let i = 0; i < items.length; i += size) chunks.push(items.slice(i, i + size));
    return chunks;
}

/**
 * Runs `work(item, index)` for every item with at most `limit` running at once, never rejecting.
 * @returns {Promise<{ok: boolean, value?: any, error?: any}[]>} one outcome per item
 */
export async function runPool(items, limit, work) {
    const outcomes = new Array(items.length);
    let next = 0;
    async function worker() {
        while (next < items.length) {
            const index = next++;
            try {
                outcomes[index] = { ok: true, value: await work(items[index], index) };
            } catch (error) {
                outcomes[index] = { ok: false, error };
            }
        }
    }
    await Promise.all(Array.from({ length: Math.min(limit, items.length) }, worker));
    return outcomes;
}

/** `YYYY-MM` (UTC) of a unix timestamp, like the month sections of the grid */
export function monthOf(unixSeconds) {
    return new Date(unixSeconds * 1000).toISOString().slice(0, 7);
}

/**
 * What the bar needs to know about the selected photos. `meta` has `{deletable, owner}` per id;
 * `deletable` and `owner` are `undefined` for photos whose tile was never loaded (the server
 * decides for those). `owner` is null for public photos.
 * @param {Iterable<{deletable?: boolean, owner?: string | null}>} metas
 * @param {string} userId
 */
export function summarize(metas, userId) {
    let count = 0;
    let undeletable = 0;
    let foreign = 0;
    for (const { deletable, owner } of metas) {
        count += 1;
        if (deletable === false) undeletable += 1;
        // Permanent deletion is owner-only (/api/delete): own or public photos
        if (owner !== undefined && owner !== null && owner !== userId) foreign += 1;
    }
    return { count, undeletable, foreign };
}

/**
 * Whether a moved photo is still part of the context, so its tile stays.
 * @param {string} contextKey
 * @param {{user_id: string | null, folder_id: number | null}} photo the photo after the move
 * @param {string} userId
 */
export function staysInContext(contextKey, photo, userId) {
    if (contextKey.startsWith('folder:')) return String(photo.folder_id) === contextKey.slice('folder:'.length);
    switch (contextKey) {
        case 'timeline:personal':
            return photo.user_id === userId;
        case 'timeline:family':
            return photo.user_id === null;
        case 'timeline:shared':
            return photo.user_id !== null && photo.user_id !== userId;
        default:
            return true;
    }
}

/**
 * The note under the picker when moving changes who can see the photos, or null.
 * @param {{public: boolean, private: boolean}} sources visibility of the photos being moved (unknown ones count for neither)
 * @param {boolean} destinationPublic
 * @returns {'private' | 'family' | null}
 */
export function visibilityChange(sources, destinationPublic) {
    if (destinationPublic) return sources.private ? 'family' : null;
    return sources.public ? 'private' : null;
}
