// Unit tests of the upload page's pure parts: file filtering, statuses, URLs and the queue.
import { expect, test } from '@playwright/test';
import { fileKey, isMedia, isPreviewable, isVideo, sortFiles } from '../../assets/js/upload/files.js';
import {
    byteProgress, checkParams, destinationKey, doneCounts, itemStatus, viewPhotosUrl, willUpload,
} from '../../assets/js/upload/status.js';
import { uploadErrorMessage, uploadUrl } from '../../assets/js/upload/transport.js';
import { RETRY_DELAYS, UploadQueue, isRetryable } from '../../assets/js/upload/queue.js';

const file = (name: string, type = '', size = 100, lastModified = 1_700_000_000_123) =>
    ({ name, type, size, lastModified, webkitRelativePath: '' }) as unknown as File;

test.describe('upload files', () => {
    test('accepts photos and videos, by MIME type or extension', () => {
        const cases: [File, boolean, boolean][] = [
            [file('a.jpg', 'image/jpeg'), true, false],
            [file('IMG_1.HEIC'), true, false],
            [file('raw.dng'), true, false],
            [file('clip.MOV'), true, true],
            [file('clip', 'video/mp4'), true, true],
            [file('x.mkv'), true, true],
            [file('notes.txt', 'text/plain'), false, false],
            [file('archive.zip', 'application/zip'), false, false],
            [file('.DS_Store'), false, false],
            [file('._IMG_1.JPG', 'image/jpeg'), false, false],
        ];
        for (const [f, media, video] of cases) {
            expect(isMedia(f), f.name).toBe(media);
            expect(isVideo(f), f.name).toBe(video);
        }
    });

    test('only shows thumbnails for images the browser decodes', () => {
        expect(isPreviewable(file('a.jpg', 'image/jpeg'))).toBe(true);
        expect(isPreviewable(file('a.heic', 'image/heic'))).toBe(false);
        expect(isPreviewable(file('a.mp4', 'video/mp4'))).toBe(false);
    });

    test('sorts out unsupported, too large and duplicate files', () => {
        const known = file('known.jpg', 'image/jpeg');
        const a = file('a.jpg', 'image/jpeg');
        const result = sortFiles(
            [a, a, file('b.txt', 'text/plain'), file('big.mp4', 'video/mp4', 2000), known, file('c.png', 'image/png')],
            { maxSize: 1000, known: new Set([fileKey(known)]) },
        );
        expect(result.accepted.map((f) => f.name)).toEqual(['a.jpg', 'c.png']);
        expect(result).toMatchObject({ unsupported: 1, tooLarge: 1, duplicates: 2 });
    });

    test('files are the same when path, size and date match', () => {
        expect(fileKey(file('a.jpg'))).toBe(fileKey(file('a.jpg')));
        expect(fileKey(file('a.jpg', '', 1))).not.toBe(fileKey(file('a.jpg', '', 2)));
        expect(fileKey(file('a.jpg', '', 1, 1))).not.toBe(fileKey(file('a.jpg', '', 1, 2)));
    });
});

test.describe('upload destinations', () => {
    const personal = { kind: 'root', isPublic: false };
    const family = { kind: 'root', isPublic: true };
    const folder = { kind: 'folder', folderId: 7, name: 'Trip', isPublic: false, type: 'shared' };
    const newFamilyFolder = { kind: 'new', name: 'New', isPublic: true };

    test('are checked in the scope the server deduplicates in', () => {
        expect(destinationKey(null)).toBeNull();
        expect(destinationKey(personal)).toBe('root:false');
        expect(destinationKey(family)).toBe('root:true');
        expect(destinationKey(folder)).toBe('folder:7');
        // A new folder isn't on the server yet: its gallery is checked
        expect(destinationKey(newFamilyFolder)).toBe('root:true');

        expect(checkParams(null)).toBeNull();
        expect(checkParams(personal)).toEqual({ make_public: false });
        expect(checkParams(folder)).toEqual({ folder_id: 7 });
        expect(checkParams(newFamilyFolder)).toEqual({ make_public: true });
    });

    test('"View photos" goes to the folder or the gallery', () => {
        expect(viewPhotosUrl({ folderId: 12 })).toBe('/folders/12');
        expect(viewPhotosUrl({ makePublic: true })).toBe('/?filter=family');
        expect(viewPhotosUrl({ makePublic: false })).toBe('/?filter=personal');
    });
});

test.describe('upload statuses', () => {
    const item = (fields: object) => ({ phase: 'pending', hash: 'h', checks: new Map(), size: 100, loaded: 0, ...fields });

    test('follow hashing, the destination and the checks', () => {
        const checked = new Map([['root:false', true], ['root:true', false]]);
        const cases: [object, string | null, string][] = [
            [item({ hash: undefined }), 'root:false', 'hashing'],
            [item({ hash: null }), null, 'queued'],
            [item({}), null, 'waiting'],
            [item({}), 'root:false', 'checking'],
            [item({ checks: checked }), 'root:false', 'existing'],
            [item({ checks: checked }), 'root:true', 'queued'],
            [item({ checks: checked }), 'folder:3', 'checking'],
            [item({ phase: 'uploading' }), 'root:false', 'uploading'],
            [item({ phase: 'failed', checks: checked }), 'root:false', 'failed'],
        ];
        for (const [i, key, expected] of cases) {
            expect(itemStatus(i, key), `${JSON.stringify(i)} ${key}`).toBe(expected);
        }
        expect(['hashing', 'waiting', 'checking', 'queued'].every(willUpload)).toBe(true);
        expect(['existing', 'uploading', 'uploaded', 'failed', 'cancelled'].some(willUpload)).toBe(false);
    });

    test('done counts and byte progress', () => {
        const items = [
            item({ phase: 'uploaded', size: 300 }),
            item({ phase: 'uploading', size: 100, loaded: 50 }),
            item({ phase: 'failed', size: 100, loaded: 90 }),
            item({ phase: 'cancelled' }),
            item({ checks: new Map([['k', true]]) }),
        ];
        expect(doneCounts(items, 'k')).toEqual({ uploaded: 1, existing: 1, failed: 1, cancelled: 1 });
        expect(byteProgress(items.slice(0, 3))).toEqual({ loaded: 350, total: 500 });
        // The multipart framing can make `loaded` slightly larger than the file
        expect(byteProgress([item({ phase: 'uploading', loaded: 120 })])).toEqual({ loaded: 100, total: 100 });
    });
});

test.describe('upload requests', () => {
    test('send the destination, the hash and a fallback time, never time_created', () => {
        const f = file('a.jpg', 'image/jpeg', 1, 1_700_000_000_999);
        expect(uploadUrl({ folderId: 4 }, { file: f, hash: 'ab' })).toBe('/api/upload?folder_id=4&hash=ab&fallback_time=1700000000');
        expect(uploadUrl({ makePublic: true }, { file: f, hash: null })).toBe('/api/upload?make_public=true&fallback_time=1700000000');
        expect(uploadUrl({ makePublic: false }, { file: file('b.jpg', '', 1, 0) })).toBe('/api/upload?make_public=false');
    });

    test('error messages', () => {
        const errors = { noPermission: 'perm', tooLarge: 'large', network: 'net', generic: 'generic' };
        const cases: [number, string, string][] = [
            [0, '', 'net'],
            [401, '', 'perm'],
            [413, 'length limit exceeded', 'large'],
            [400, 'Invalid file name', 'Invalid file name'],
            [403, ' No permission ', 'No permission'],
            [404, '', 'generic'],
            [422, '<html>…</html>', 'generic'],
            [500, 'Internal server error', 'generic'],
        ];
        for (const [status, body, expected] of cases) {
            expect(uploadErrorMessage(status, body, errors), `${status}`).toBe(expected);
        }
    });
});

test.describe('upload queue', () => {
    type Result = { ok: true, photo: object } | { ok: false, status: number, message: string };

    /** A transport whose responses the test resolves one by one */
    function manualTransport() {
        const calls: { item: any, resolve: (r: Result) => void, signal: AbortSignal }[] = [];
        const send = (item: any, { signal }: { signal: AbortSignal }) => new Promise<Result>((resolve, reject) => {
            calls.push({ item, resolve, signal });
            signal.addEventListener('abort', () => reject(new DOMException('Cancelled', 'AbortError')));
        });
        return { calls, send };
    }

    const flush = () => new Promise((resolve) => setTimeout(resolve, 0));

    test('retries network and server errors only', () => {
        expect([0, 500, 502, 503].every(isRetryable)).toBe(true);
        expect([400, 401, 403, 404, 409, 413].some(isRetryable)).toBe(false);
        expect(RETRY_DELAYS).toEqual([1000, 4000, 10000]);
    });

    test('uploads 3 files at a time', async () => {
        const { calls, send } = manualTransport();
        let idle = 0;
        const queue = new UploadQueue({ send, onIdle: () => { idle += 1; } });
        const items = [1, 2, 3, 4, 5].map((id) => ({ id }));
        items.forEach((item) => queue.push(item));

        expect(calls.map((c) => c.item.id)).toEqual([1, 2, 3]);
        expect(items.map((i: any) => i.phase)).toEqual(['uploading', 'uploading', 'uploading', 'queued', 'queued']);

        calls[1].resolve({ ok: true, photo: { id: 20 } });
        await flush();
        expect(calls.map((c) => c.item.id)).toEqual([1, 2, 3, 4]);
        expect(items[1]).toMatchObject({ phase: 'uploaded', photo: { id: 20 } });

        for (const call of [calls[0], calls[2], calls[3]]) call.resolve({ ok: true, photo: {} });
        await flush();
        calls[4].resolve({ ok: true, photo: {} });
        await flush();
        expect(items.every((i: any) => i.phase === 'uploaded')).toBe(true);
        expect(queue.idle).toBe(true);
        expect(idle).toBe(1);
    });

    test('retries with backoff, then fails with the message', async () => {
        const { calls, send } = manualTransport();
        const waits: number[] = [];
        const queue = new UploadQueue({ send, wait: async (ms: number) => { waits.push(ms); } });

        const recovered: any = { id: 1 };
        queue.push(recovered);
        calls[0].resolve({ ok: false, status: 500, message: 'generic' });
        await flush();
        calls[1].resolve({ ok: true, photo: {} });
        await flush();
        expect(recovered.phase).toBe('uploaded');
        expect(waits).toEqual([1000]);

        const failing: any = { id: 2 };
        queue.push(failing);
        for (let attempt = 0; attempt < 4; attempt += 1) {
            calls[2 + attempt].resolve({ ok: false, status: 0, message: 'net' });
            await flush();
        }
        expect(failing).toMatchObject({ phase: 'failed', error: 'net' });
        expect(calls).toHaveLength(6);
        expect(waits).toEqual([1000, 1000, 4000, 10000]);
    });

    test('client errors fail at once', async () => {
        const { calls, send } = manualTransport();
        const queue = new UploadQueue({ send, wait: async () => {} });
        const item: any = { id: 1 };
        queue.push(item);
        calls[0].resolve({ ok: false, status: 400, message: 'Invalid file name' });
        await flush();
        expect(item).toMatchObject({ phase: 'failed', error: 'Invalid file name' });
        expect(calls).toHaveLength(1);
    });

    test('cancel aborts the uploads and cancels the rest', async () => {
        const { calls, send } = manualTransport();
        let idle = 0;
        const queue = new UploadQueue({ send, onIdle: () => { idle += 1; } });
        const items: any[] = [1, 2, 3, 4].map((id) => ({ id }));
        items.forEach((item) => queue.push(item));
        calls[0].resolve({ ok: true, photo: {} });
        await flush();

        queue.cancel();
        await flush();
        expect(items.map((i) => i.phase)).toEqual(['uploaded', 'cancelled', 'cancelled', 'cancelled']);
        expect(calls.slice(1).every((c) => c.signal.aborted)).toBe(true);
        expect(queue.idle).toBe(true);
        expect(idle).toBe(1);
    });

    test('cancel also stops a retry that is waiting', async () => {
        const { calls, send } = manualTransport();
        const queue = new UploadQueue({ send });
        const item: any = { id: 1 };
        queue.push(item);
        calls[0].resolve({ ok: false, status: 503, message: 'generic' });
        await flush();
        expect(item.phase).toBe('retrying');
        queue.cancel();
        await flush();
        expect(item.phase).toBe('cancelled');
        expect(calls).toHaveLength(1);
    });
});
