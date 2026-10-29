// The state of the files on the upload page, as pure functions.
//
// An item is {size, phase, loaded, …}: `phase` is 'pending' until the upload queue takes it, then
// 'queued' | 'uploading' | 'retrying' | 'uploaded' | 'failed' | 'cancelled' (set by queue.js, or by
// Cancel).

/**
 * Where "View photos" goes after uploading.
 * @param {{folderId?: number, makePublic?: boolean}} params what `resolveDestination` returned
 */
export function viewPhotosUrl(params) {
    if (params.folderId !== undefined) return `/folders/${params.folderId}`;
    return params.makePublic ? '/?filter=family' : '/?filter=personal';
}

/**
 * The status shown for an item: waiting (no destination), queued, uploading, retrying, uploaded,
 * failed or cancelled.
 * @param {boolean} hasDestination
 */
export function itemStatus(item, hasDestination) {
    if (item.phase !== 'pending') return item.phase;
    return hasDestination ? 'queued' : 'waiting';
}

/** Statuses of files that will still be uploaded when the user starts (or the run goes on) */
export function willUpload(status) {
    return status === 'waiting' || status === 'queued';
}

/**
 * The counts of the done summary.
 * @returns {{uploaded: number, failed: number, cancelled: number}}
 */
export function doneCounts(items) {
    const counts = { uploaded: 0, failed: 0, cancelled: 0 };
    for (const item of items) {
        if (item.phase in counts) counts[item.phase] += 1;
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
