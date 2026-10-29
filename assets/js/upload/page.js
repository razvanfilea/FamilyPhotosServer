// The upload page (templates/upload/upload_page.html).
//
// Files flow like this: picked or dropped files are filtered (files.js) and get a row; hasher.js
// hashes them in workers right away; once a destination is chosen, their hashes are checked in
// batches with `POST /api/upload/check-hashes`, and matches show "Already on server". "Upload"
// resolves the destination once (creating a new folder), then queue.js uploads every file that
// isn't on the server yet, 3 at a time, through transport.js. Files that finish hashing or
// checking during a run join it.

import { register } from '../core/registry.js';
import { postJson, redirectToLogin } from '../core/api.js';
import { formatBytes, plural } from '../core/format.js';
import { DESTINATION_CHANGE_EVENT, resolveDestination } from '../destination-picker/picker.js';
import { droppedFiles, fileKey, isPreviewable, isVideo, sortFiles } from './files.js';
import { Hasher } from './hasher.js';
import { UploadQueue } from './queue.js';
import {
    byteProgress, checkParams, destinationKey, doneCounts, itemStatus, viewPhotosUrl, willUpload,
} from './status.js';
import { sendFile, uploadErrorMessage, uploadUrl } from './transport.js';

/** Hashes are checked as soon as this many are ready (or hashing is done) */
const CHECK_BATCH = 50;
/** …or after this long */
const CHECK_DELAY_MS = 1000;
/** The server accepts at most 1000 hashes per check */
const CHECK_MAX = 500;
const SHOWCASE_SIZE = 8;

const fill = (template, values) => template.replace(/\{(\w+)\}/g, (match, key) => values[key] ?? match);

/** The upload answers 401 both for an expired session and for a folder the user can't upload to */
async function sessionAlive() {
    try {
        const response = await fetch('/api/profile', { headers: { Accept: 'application/json' } });
        return response.status !== 401;
    } catch {
        return true;
    }
}

register('upload-page', (root) => {
    const strings = JSON.parse(root.querySelector('[data-role="strings"]').textContent);
    const maxSize = Number(root.dataset.maxFileSize);
    const part = (role) => root.querySelector(`[data-role="${role}"]`);
    const destinationSection = part('destination');
    const pickerHost = part('picker-host');
    const queueSection = part('queue');
    const list = part('list');
    const notices = part('notices');
    const fileInput = part('file-input');
    const folderInput = part('folder-input');
    const overlay = part('overlay');
    const progress = part('progress');
    const startButton = root.querySelector('[data-action="start"]');
    const cancelButton = root.querySelector('[data-action="cancel"]');
    const clearButton = root.querySelector('[data-action="clear"]');
    const retryFailedButton = root.querySelector('[data-action="retry-failed"]');
    const rowTemplate = part('row-template');
    const noticeTemplate = part('notice-template');

    /**
     * @typedef {{id: number, file: File, key: string, size: number, hash?: string|null, hashed: number, checks: Map<any, any>, checking: Set<any>, phase: string, loaded: number, error: any, thumbUrl: string|null, [field: string]: any}} UploadItem
     */
    /** @type {Map<number, UploadItem>} */
    const items = new Map();
    let nextId = 1;
    /** The picker's choice */
    let destination = null;
    /** idle | running | done */
    let state = 'idle';
    /** What `resolveDestination` returned for the current run */
    let runParams = null;
    let runItems = new Set();
    let checkTimer = 0;

    const currentKey = () => destinationKey(destination);
    const statusOf = (item) => itemStatus(item, currentKey());

    // ---------- Rendering: rows are updated one by one, at most once per frame ----------

    const dirty = new Set();
    let frame = 0;
    let showcaseDirty = false;

    function touch(item) {
        dirty.add(item);
        frame ||= requestAnimationFrame(flush);
    }

    function touchAll() {
        for (const item of items.values()) dirty.add(item);
        frame ||= requestAnimationFrame(flush);
    }

    function flush() {
        frame = 0;
        for (const item of dirty) {
            if (items.has(item.id)) renderRow(item);
        }
        dirty.clear();
        renderSummary();
        if (showcaseDirty) renderShowcase();
    }

    function statusLabel(item, status) {
        if (status === 'hashing' && item.hashed > 0) {
            return fill(strings.status.hashingPercent, { percent: Math.floor((item.hashed / item.size) * 100) });
        }
        if (status === 'uploading') {
            return fill(strings.status.uploading, { percent: Math.floor((Math.min(item.loaded, item.size) / (item.size || 1)) * 100) });
        }
        return strings.status[status];
    }

    function renderRow(item) {
        const status = statusOf(item);
        const { row } = item;
        row.dataset.status = status;
        row.querySelector('[data-role="status"]').textContent = statusLabel(item, status);
        // A hash failure is not fatal (the file is uploaded without a hash), so it is a tooltip
        row.querySelector('[data-role="status"]').title = item.hashError ? fill(strings.errors.hashFailed, { error: item.hashError }) : '';
        row.querySelector('[data-role="error"]').textContent = item.error && (status === 'failed' || status === 'cancelled') ? item.error : '';
        const fraction = status === 'uploaded' ? 1 : status === 'uploading' ? Math.min(item.loaded / (item.size || 1), 1) : 0;
        row.style.setProperty('--progress', String(fraction));
        // Rows can be removed before uploading, and retried after failing
        row.querySelector('[data-action="remove"]').hidden = state !== 'idle' || !(willUpload(status) || status === 'existing');
        row.querySelector('[data-action="retry"]').hidden = state !== 'done' || (status !== 'failed' && status !== 'cancelled');
    }

    function createRow(item) {
        const row = rowTemplate.content.firstElementChild.cloneNode(true);
        row.dataset.itemId = String(item.id);
        row.dataset.kind = isVideo(item.file) ? 'video' : 'image';
        row.querySelector('[data-role="name"]').textContent = item.file.name;
        row.querySelector('[data-role="name"]').title = item.file.webkitRelativePath || item.file.name;
        row.querySelector('[data-role="size"]').textContent = formatBytes(item.size);
        row.querySelector('[data-action="remove"]').setAttribute('aria-label', fill(strings.remove, { name: item.file.name }));
        row.querySelector('[data-action="retry"]').setAttribute('aria-label', fill(strings.retry, { name: item.file.name }));
        if (isPreviewable(item.file)) {
            item.thumbUrl = URL.createObjectURL(item.file);
            const img = document.createElement('img');
            img.className = 'fp-upload-row__img';
            img.alt = '';
            img.loading = 'lazy';
            img.decoding = 'async';
            img.src = item.thumbUrl;
            img.addEventListener('error', () => img.remove(), { once: true });
            row.querySelector('.fp-upload-row__thumb').append(img);
        }
        item.row = row;
        renderRow(item);
        return row;
    }

    function renderSummary() {
        const all = [...items.values()];
        const statuses = all.map(statusOf);
        const toUpload = statuses.filter(willUpload).length;
        const totalSize = all.reduce((sum, item) => sum + item.size, 0);

        root.dataset.state = state;
        queueSection.hidden = items.size === 0;
        clearButton.hidden = state !== 'idle';
        startButton.hidden = state !== 'idle';
        cancelButton.hidden = state !== 'running';
        progress.hidden = state === 'idle';
        part('summary').textContent = fill(strings.summary, { files: plural(all.length, 'file'), size: formatBytes(totalSize) });
        part('start-label').textContent = toUpload > 0
            ? fill(strings.start, { photos: plural(toUpload, 'photo') })
            : strings.startNone;
        startButton.disabled = state !== 'idle' || destination === null || toUpload === 0;
        part('hint').hidden = destination !== null || state !== 'idle';

        const { loaded, total } = byteProgress(runItems);
        progress.value = total > 0 ? loaded / total : 0;
        part('progress-text').textContent = state === 'idle'
            ? ''
            : fill(strings.progress, { uploaded: formatBytes(loaded), total: formatBytes(total) });

        if (state === 'done') {
            const counts = doneCounts(all, currentKey());
            const parts = Object.entries(counts)
                .filter(([name, count]) => count > 0 || name === 'uploaded')
                .map(([name, count]) => fill(strings.done[name], { count }));
            part('done-summary').textContent = parts.join(strings.done.separator);
            retryFailedButton.hidden = counts.failed + counts.cancelled === 0;
        }
    }

    /** The picker's showcase slot: the first thumbnails and "N files" */
    function renderShowcase() {
        const slot = pickerHost.querySelector('[data-slot="showcase"]');
        if (!slot) return;
        showcaseDirty = false;
        slot.replaceChildren();
        if (items.size === 0) return;

        const row = document.createElement('div');
        row.className = 'fp-upload-showcase';
        for (const item of [...items.values()].filter((i) => i.thumbUrl).slice(0, SHOWCASE_SIZE)) {
            const img = document.createElement('img');
            img.alt = '';
            img.decoding = 'async';
            img.src = item.thumbUrl;
            row.append(img);
        }
        const count = document.createElement('span');
        count.className = 'fp-upload-showcase__count';
        count.textContent = plural(items.size, 'file');
        row.append(count);
        slot.append(row);
    }

    function setState(next) {
        state = next;
        destinationSection.inert = state === 'running';
        touchAll();
        frame ||= requestAnimationFrame(flush);
    }

    // ---------- Adding and removing files ----------

    function showNotices(messages) {
        notices.replaceChildren(...messages.map((message) => {
            const notice = noticeTemplate.content.firstElementChild.cloneNode(true);
            notice.querySelector('[data-role="text"]').textContent = message;
            notice.querySelector('[data-action="dismiss"]').addEventListener('click', () => notice.remove());
            return notice;
        }));
    }

    function addFiles(files) {
        if (state === 'done') reset();

        const known = new Set([...items.values()].map((item) => item.key));
        const { accepted, unsupported, tooLarge, duplicates } = sortFiles(files, { maxSize, known });
        const messages = [];
        if (unsupported > 0) messages.push(fill(strings.skippedUnsupported, { files: plural(unsupported, 'file') }));
        if (tooLarge > 0) messages.push(fill(strings.skippedTooLarge, { files: plural(tooLarge, 'file') }));
        if (duplicates > 0) messages.push(fill(strings.skippedDuplicate, { files: plural(duplicates, 'file') }));
        showNotices(messages);

        const rows = document.createDocumentFragment();
        const added = accepted.map((file) => {
            const item = {
                id: nextId++, file, key: fileKey(file), size: file.size, hash: undefined, hashed: 0,
                checks: new Map(), checking: new Set(), phase: 'pending', loaded: 0, error: null, thumbUrl: null,
            };
            items.set(item.id, item);
            rows.append(createRow(item));
            return item;
        });
        list.append(rows);
        for (const item of added) hasher.add(item);

        showcaseDirty = true;
        frame ||= requestAnimationFrame(flush);
    }

    function forget(item) {
        hasher.remove(item);
        if (item.thumbUrl) URL.revokeObjectURL(item.thumbUrl);
        item.row.remove();
        items.delete(item.id);
    }

    function remove(item) {
        if (state === 'running') return;
        forget(item);
        showcaseDirty = true;
        frame ||= requestAnimationFrame(flush);
    }

    /** Back to an empty page ("Upload more", "Clear all") */
    function reset() {
        for (const item of [...items.values()]) forget(item);
        notices.replaceChildren();
        runItems = new Set();
        runParams = null;
        showcaseDirty = true;
        setState('idle');
    }

    // ---------- Hashing and checking ----------

    const hasher = new Hasher({
        onProgress(item, bytesRead) {
            item.hashed = bytesRead;
            touch(item);
        },
        onDone(item, hash, error) {
            if (!items.has(item.id)) return;
            item.hash = hash;
            item.hashError = error ?? null;
            touch(item);
            scheduleChecks();
            advance();
        },
    });

    /** Items whose hash still has to be checked for the current destination */
    function uncheckedItems(key) {
        return [...items.values()].filter((item) => item.phase === 'pending' && typeof item.hash === 'string'
            && !item.checks.has(key) && !item.checking.has(key));
    }

    function scheduleChecks() {
        const key = currentKey();
        if (key === null) return;
        const ready = uncheckedItems(key).length;
        if (ready === 0) return;
        if (ready >= CHECK_BATCH || hasher.idle) {
            clearTimeout(checkTimer);
            checkTimer = 0;
            runChecks();
        } else if (!checkTimer) {
            checkTimer = setTimeout(() => {
                checkTimer = 0;
                runChecks();
            }, CHECK_DELAY_MS);
        }
    }

    async function runChecks() {
        const key = currentKey();
        const params = checkParams(destination);
        if (key === null) return;

        const batch = uncheckedItems(key).slice(0, CHECK_MAX);
        for (const item of batch) item.checking.add(key);

        let existing = new Set();
        try {
            const response = await postJson('/api/upload/check-hashes', { hashes: batch.map((item) => item.hash), ...params }, { silent: true });
            existing = new Set(response.existing);
        } catch {
            // Upload them anyway: the server still recognises files it already has
        }
        for (const item of batch) {
            item.checking.delete(key);
            item.checks.set(key, existing.has(item.hash));
            touch(item);
        }
        scheduleChecks();
        advance();
    }

    // ---------- Uploading ----------

    let sessionExpired = false;
    let sessionCheck = null;

    /**
     * The first 401 checks the session once, for every upload that is in flight. When it has
     * expired, the rest of the queue is cancelled and the user is sent to the login page once.
     * Resolves to whether the session is alive.
     */
    function checkSession() {
        sessionCheck ??= sessionAlive().then((alive) => {
            if (alive) {
                sessionCheck = null;
            } else if (!sessionExpired) {
                sessionExpired = true;
                // The browser asks before leaving, because the run is not finished
                cancel();
                redirectToLogin();
            }
            return alive;
        });
        return sessionCheck;
    }

    async function send(item, { signal, onProgress }) {
        const { status, body } = await sendFile(uploadUrl(runParams, item), item.file, { signal, onProgress });
        if (status >= 200 && status < 300) {
            return { ok: true, photo: JSON.parse(body) };
        }
        if (status === 401 && (sessionExpired || !(await checkSession()))) {
            return { ok: false, status, message: strings.errors.sessionExpired };
        }
        return { ok: false, status, message: uploadErrorMessage(status, body, strings.errors) };
    }

    const queue = new UploadQueue({ send, onChange: touch, onIdle: maybeFinish });

    /** During a run, hands every file that is ready to the queue */
    function advance() {
        if (state !== 'running' || runParams === null) return;
        for (const item of items.values()) {
            if (item.phase === 'pending' && statusOf(item) === 'queued') {
                runItems.add(item);
                queue.push(item);
            }
        }
        maybeFinish();
    }

    function maybeFinish() {
        if (state !== 'running' || runParams === null || !queue.idle) return;
        if ([...items.values()].some((item) => willUpload(statusOf(item)))) return;
        part('view-photos').href = viewPhotosUrl(runParams);
        setState('done');
    }

    async function start() {
        if (state !== 'idle' || destination === null) return;
        const chosen = destination;
        setState('running');
        try {
            runParams = await resolveDestination(chosen);
        } catch {
            // api.js showed the error (e.g. an invalid folder name)
            setState('idle');
            return;
        }
        runItems = new Set();
        scheduleChecks();
        advance();
    }

    function cancel() {
        if (state !== 'running') return;
        for (const item of items.values()) {
            if (item.phase === 'pending' && willUpload(statusOf(item))) {
                item.phase = 'cancelled';
                touch(item);
            }
        }
        queue.cancel();
        for (const item of items.values()) {
            if (item.phase === 'pending') hasher.remove(item);
        }
        maybeFinish();
    }

    /** Uploads failed or cancelled files again */
    function retry(toRetry) {
        if (state === 'running' || toRetry.length === 0) return;
        for (const item of toRetry) {
            item.phase = 'pending';
            item.error = null;
            item.loaded = 0;
            // Cancelling stops hashing, so a cancelled file may still need its hash
            if (item.hash === undefined) hasher.add(item);
            touch(item);
        }
        state = 'idle';
        start();
    }

    // ---------- Events ----------

    root.addEventListener(DESTINATION_CHANGE_EVENT, (event) => {
        destination = event.detail.destination;
        touchAll();
        showcaseDirty = true;
        scheduleChecks();
    });

    // The showcase slot arrives with the picker
    pickerHost.addEventListener('htmx:afterSettle', () => {
        showcaseDirty = true;
        frame ||= requestAnimationFrame(flush);
    });

    root.addEventListener('click', (event) => {
        const button = /** @type {Element} */ (event.target).closest('[data-action]');
        if (!button || !root.contains(button)) return;
        const row = button.closest('[data-item-id]');
        const item = row ? items.get(Number(row.dataset.itemId)) : null;

        switch (button.dataset.action) {
            case 'choose-files':
                fileInput.click();
                break;
            case 'choose-folder':
                folderInput.click();
                break;
            case 'change-destination':
                part('destination-summary').hidden = true;
                pickerHost.hidden = false;
                break;
            case 'start':
                start();
                break;
            case 'cancel':
                cancel();
                break;
            case 'clear':
                if (state === 'idle') reset();
                break;
            case 'upload-more':
                reset();
                break;
            case 'retry-failed':
                retry([...items.values()].filter((i) => i.phase === 'failed' || i.phase === 'cancelled'));
                break;
            case 'remove':
                if (item) remove(item);
                break;
            case 'retry':
                if (item) retry([item]);
                break;
        }
    });

    for (const input of [fileInput, folderInput]) {
        input.addEventListener('change', () => {
            const files = [...input.files];
            input.value = '';
            addFiles(files);
        });
    }

    // Files can be dropped anywhere on the page
    let dragDepth = 0;
    const hasFiles = (event) => [...(event.dataTransfer?.types ?? [])].includes('Files');
    const onDragEnter = (event) => {
        if (!hasFiles(event)) return;
        event.preventDefault();
        dragDepth += 1;
        overlay.hidden = false;
    };
    const onDragLeave = (event) => {
        if (!hasFiles(event)) return;
        dragDepth = Math.max(0, dragDepth - 1);
        if (dragDepth === 0) overlay.hidden = true;
    };
    const onDragOver = (event) => {
        if (!hasFiles(event)) return;
        event.preventDefault();
        event.dataTransfer.dropEffect = 'copy';
    };
    const onDrop = async (event) => {
        if (!hasFiles(event)) return;
        event.preventDefault();
        dragDepth = 0;
        overlay.hidden = true;
        addFiles(await droppedFiles(event.dataTransfer));
    };
    const onBeforeUnload = (event) => {
        const hashing = [...items.values()].some((item) => item.phase === 'pending' && item.hash === undefined);
        if (state === 'running' || sessionExpired || hashing) {
            event.preventDefault();
            event.returnValue = '';
        }
    };

    document.addEventListener('dragenter', onDragEnter);
    document.addEventListener('dragleave', onDragLeave);
    document.addEventListener('dragover', onDragOver);
    document.addEventListener('drop', onDrop);
    window.addEventListener('beforeunload', onBeforeUnload);

    return () => {
        document.removeEventListener('dragenter', onDragEnter);
        document.removeEventListener('dragleave', onDragLeave);
        document.removeEventListener('dragover', onDragOver);
        document.removeEventListener('drop', onDrop);
        window.removeEventListener('beforeunload', onBeforeUnload);
        queue.cancel();
        hasher.destroy();
        clearTimeout(checkTimer);
        for (const item of items.values()) {
            if (item.thumbUrl) URL.revokeObjectURL(item.thumbUrl);
        }
    };
});
