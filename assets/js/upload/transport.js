// Sending one file to `POST /api/upload` with XMLHttpRequest (fetch can't report upload progress).

/**
 * The upload URL of a file. The date isn't sent: the server reads it from the file (EXIF, file
 * name…) and only falls back to the file's modification time.
 * @param {{folderId?: number, makePublic?: boolean}} params what `resolveDestination` returned
 * @param {{file: File, hash?: string|null}} item
 */
export function uploadUrl(params, item) {
    const query = new URLSearchParams();
    if (params.folderId !== undefined) {
        query.set('folder_id', String(params.folderId));
    } else {
        query.set('make_public', String(Boolean(params.makePublic)));
    }
    if (item.hash) query.set('hash', item.hash);
    if (item.file.lastModified > 0) {
        query.set('fallback_time', String(Math.floor(item.file.lastModified / 1000)));
    }
    return `/api/upload?${query}`;
}

/**
 * The message of a failed upload. 4xx text bodies are the server's user-facing messages.
 * @param {number} status 0 for network errors
 * @param {{noPermission: string, tooLarge: string, network: string, generic: string}} errors
 */
export function uploadErrorMessage(status, body, errors) {
    if (status === 0) return errors.network;
    if (status === 413) return errors.tooLarge;
    // The upload answers 401 for a folder the user can't upload to (kept for the Android app)
    if (status === 401) return errors.noPermission;
    const text = body?.trim() ?? '';
    if (status >= 400 && status < 500 && text && text.length <= 200 && !text.startsWith('<')) return text;
    return errors.generic;
}

/**
 * @param {string} url
 * @param {File} file
 * @param {{signal: AbortSignal, onProgress: (loaded: number) => void}} options `loaded` is in
 *   bytes of the file (the multipart framing is scaled away)
 * @returns {Promise<{status: number, body: string}>} status 0 for network errors; rejects with an
 *   AbortError when cancelled
 */
export function sendFile(url, file, { signal, onProgress }) {
    return new Promise((resolve, reject) => {
        const xhr = new XMLHttpRequest();
        xhr.open('POST', url);
        xhr.setRequestHeader('Accept', 'application/json');
        xhr.upload.addEventListener('progress', (event) => {
            if (event.lengthComputable && event.total > 0) {
                onProgress(Math.round((event.loaded / event.total) * file.size));
            }
        });
        xhr.addEventListener('load', () => resolve({ status: xhr.status, body: xhr.responseText }));
        xhr.addEventListener('error', () => resolve({ status: 0, body: '' }));
        xhr.addEventListener('abort', () => reject(new DOMException('Cancelled', 'AbortError')));

        if (signal.aborted) {
            reject(new DOMException('Cancelled', 'AbortError'));
            return;
        }
        signal.addEventListener('abort', () => xhr.abort(), { once: true });

        const form = new FormData();
        form.append('file', file, file.name);
        xhr.send(form);
    });
}
