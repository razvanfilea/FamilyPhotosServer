// The state of the files on the upload page, as pure functions.
//
// An item is {size, hash, checks, phase, loaded, …}:
// - `hash`: undefined while hashing, null if hashing failed (the file is uploaded without one),
//   else the BLAKE3 hex digest;
// - `checks`: Map of destination key → whether the server already has the file there;
// - `phase`: 'pending' until the upload queue takes it, then 'queued' | 'uploading' | 'retrying'
//   | 'uploaded' | 'failed' | 'cancelled' (set by queue.js, or by Cancel).

/**
 * The scope the server deduplicates in for a destination: a folder of someone else is checked on
 * its own, everything else by owner (the user, or public). A folder that is still to be created
 * is checked like the gallery it will be in.
 * @returns {string|null}
 */
export function destinationKey(destination) {
    if (!destination) return null;
    if (destination.kind === 'folder') return `folder:${destination.folderId}`;
    return `root:${destination.isPublic}`;
}

/** The destination part of `POST /api/upload/check-hashes`, or null without a destination. */
export function checkParams(destination) {
    if (!destination) return null;
    if (destination.kind === 'folder') return { folder_id: destination.folderId };
    return { make_public: destination.isPublic };
}

/**
 * Where "View photos" goes after uploading.
 * @param {{folderId?: number, makePublic?: boolean}} params what `resolveDestination` returned
 */
export function viewPhotosUrl(params) {
    if (params.folderId !== undefined) return `/folders/${params.folderId}`;
    return params.makePublic ? '/?filter=family' : '/?filter=personal';
}

/**
 * The status shown for an item: hashing, waiting (no destination), checking, existing (already on
 * the server), queued, uploading, retrying, uploaded, failed or cancelled.
 * @param {string|null} key the current `destinationKey`
 */
export function itemStatus(item, key) {
    if (item.phase !== 'pending') return item.phase;
    if (item.hash === undefined) return 'hashing';
    if (item.hash === null) return 'queued';
    if (key === null) return 'waiting';
    const exists = item.checks.get(key);
    if (exists === undefined) return 'checking';
    return exists ? 'existing' : 'queued';
}

/** Statuses of files that will still be uploaded when the user starts (or the run goes on) */
const TO_UPLOAD = new Set(['hashing', 'waiting', 'checking', 'queued']);

export function willUpload(status) {
    return TO_UPLOAD.has(status);
}

/**
 * The counts of the done summary.
 * @returns {{uploaded: number, existing: number, failed: number, cancelled: number}}
 */
export function doneCounts(items, key) {
    const counts = { uploaded: 0, existing: 0, failed: 0, cancelled: 0 };
    for (const item of items) {
        const status = itemStatus(item, key);
        if (status in counts) counts[status] += 1;
    }
    return counts;
}

/**
 * Upload progress by bytes: the files of the current run that are uploaded, plus what the ones in
 * flight sent so far.
 * @returns {{loaded: number, total: number}}
 */
export function byteProgress(runItems) {
    let loaded = 0;
    let total = 0;
    for (const item of runItems) {
        total += item.size;
        if (item.phase === 'uploaded') loaded += item.size;
        else if (item.phase === 'uploading') loaded += Math.min(item.loaded, item.size);
    }
    return { loaded, total };
}
