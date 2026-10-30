// What the selection bar's buttons do. `runAction(name, selection, {userId})` works on the
// selection of a grid (`element.fpSelection`) and reports with toasts and events:
//
//   - photos leaving the grid: `fp:photos-removed {ids, contextKey?}` (the grid drops their tiles
//     and the selection forgets them)
//   - photos coming back (Undo of a trash): `fp:photos-added {ids}` (the grid reloads its sections)
//
// Counts in the toasts are always those of the photos that really changed. Failed photos stay
// selected.

import { ApiError, api, postJson } from '../core/api.js';
import { confirm } from '../core/dialog.js';
import { EVENTS, emit } from '../core/events.js';
import { toast } from '../core/toast.js';
import { openDeleteDialog } from '../viewer/delete-dialog.js';
import { openMoveDialog } from './move-dialog.js';
import { s } from './strings.js';
import {
    BULK_CHUNK,
    MAX_QUIET_DOWNLOADS,
    MAX_SHARE,
    chunk,
    runPool,
    staysInContext,
} from './math.js';

const FAVORITE_CONCURRENCY = 6;
const DELETE_CONCURRENCY = 4;
const DOWNLOAD_SPACING = 300;

const removed = (ids, contextKey) => emit(document.body, EVENTS.photosRemoved, { ids, contextKey });
const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));

/** The server answers 404 when it won't touch some of the photos at all: say which rule it was */
function explain(error, notAllowed) {
    return error instanceof ApiError && error.status === 404 ? new Error(s(notAllowed)) : error;
}

/**
 * POSTs the ids in chunks, stopping at the first failure.
 * @returns {Promise<{done: number[], error: Error | null}>} the ids whose chunks succeeded
 */
async function postChunks(path, ids) {
    const done = [];
    for (const part of chunk(ids, BULK_CHUNK)) {
        try {
            await postJson(path, part, { silent: true });
        } catch (error) {
            return { done, error };
        }
        done.push(...part);
    }
    return { done, error: null };
}

/** Toast for an operation on `total` photos of which `count` worked */
function report(count, total, all, partial) {
    toast(count === total ? s(all, { count }) : s(partial, { count, total }), {
        type: count === total ? 'success' : 'info',
    });
}

// ---------- Trash ----------

/** Undo of a trash: the photos come back in every context */
async function undoTrash(ids) {
    const { done, error } = await postChunks('/api/trash/restore', ids);
    if (done.length > 0) {
        emit(document.body, EVENTS.photosAdded, { ids: done });
        report(done.length, ids.length, 'restored', 'restoredPartial');
    }
    if (error) toast(explain(error, 'cantRestore').message, { type: 'error' });
}

async function trash(selection) {
    const ids = selection.ids();
    const trashed = new Set();

    const deleted = await openDeleteDialog({
        ids,
        onConfirm: async () => {
            const { done, error } = await postChunks('/api/trash', ids.filter((id) => !trashed.has(id)));
            for (const id of done) trashed.add(id);
            if (error) throw explain(error, 'cantDelete');
        },
    });

    // A failed chunk can leave some photos trashed even when the dialog was cancelled afterwards
    const done = ids.filter((id) => trashed.has(id));
    if (done.length === 0) return;
    removed(done);
    toast(done.length === ids.length
        ? s('movedToTrash', { count: done.length })
        : s('movedToTrashPartial', { count: done.length, total: ids.length }), {
        type: deleted ? 'success' : 'info',
        action: { label: s('undo'), onClick: () => undoTrash(done) },
    });
}

async function restore(selection) {
    const ids = selection.ids();
    const { done, error } = await postChunks('/api/trash/restore', ids);
    if (done.length > 0) {
        removed(done, 'trash');
        report(done.length, ids.length, 'restored', 'restoredPartial');
    }
    if (error) toast(explain(error, 'cantRestore').message, { type: 'error' });
}

async function deleteForever(selection, _userId, title) {
    const ids = selection.ids();
    const deleted = new Set();

    await openDeleteDialog({
        ids,
        permanent: true,
        title,
        onConfirm: async ({ setStatus }) => {
            const todo = ids.filter((id) => !deleted.has(id));
            let finished = 0;
            const outcomes = await runPool(todo, DELETE_CONCURRENCY, async (id) => {
                try {
                    await api(`/api/delete/${id}`, { method: 'DELETE', silent: true });
                    deleted.add(id);
                } finally {
                    setStatus(`${++finished} of ${todo.length}`);
                }
            });
            // Nothing worked: stay open with the reason. Otherwise the toast reports what failed
            const failure = outcomes.find((outcome) => !outcome.ok);
            if (failure && deleted.size === 0) throw explain(failure.error, 'cantDeleteForever');
        },
    });

    const done = ids.filter((id) => deleted.has(id));
    if (done.length === 0) return;
    removed(done);
    toast(done.length === ids.length
        ? s('deletedForever', { count: done.length })
        : s('deletedForeverPartial', { count: done.length, total: ids.length }), {
        type: done.length === ids.length ? 'success' : 'error',
    });
}

// ---------- Move ----------

async function move(selection, userId) {
    const ids = selection.ids();
    const result = await openMoveDialog({ ids });
    if (!result) return;

    const { moved } = result;
    report(moved.length, ids.length, 'moved', 'movedPartial');
    const contextKey = selection.contextKey;
    const gone = moved.filter((photo) => !staysInContext(contextKey, photo, userId)).map((photo) => photo.id);
    if (gone.length > 0) removed(gone);
    const stays = moved.filter((photo) => !gone.includes(photo.id));
    if (stays.length > 0) emit(document.body, EVENTS.photosChanged, { photos: stays });
    // Photos the server skipped stay selected
    if (moved.length === ids.length) selection.clear();
}

// ---------- Favorites ----------

async function favorite(selection) {
    const ids = selection.ids();
    const unfavorite = selection.contextKey === 'favorites';
    const outcomes = await runPool(ids, FAVORITE_CONCURRENCY, (id) =>
        api(`/api/favorite/${id}`, { method: unfavorite ? 'DELETE' : 'POST', silent: true }));
    const done = ids.filter((_id, i) => outcomes[i].ok);

    const [all, partial] = unfavorite ? ['unfavorited', 'unfavoritedPartial'] : ['favorited', 'favoritedPartial'];
    if (done.length > 0) report(done.length, ids.length, all, partial);
    if (done.length < ids.length) {
        const failure = outcomes.find((outcome) => !outcome.ok);
        toast(failure.error.message, { type: 'error' });
    }
    if (unfavorite && done.length > 0) removed(done, 'favorites');
    else if (done.length === ids.length) selection.clear();
}

// ---------- Download and share ----------

async function download(selection) {
    const ids = selection.ids();
    if (ids.length > MAX_QUIET_DOWNLOADS) {
        const confirmed = await confirm({
            title: s('downloadTitle', { count: ids.length }),
            body: s('downloadBody'),
            confirmLabel: s('download'),
        });
        if (!confirmed) return;
    }

    toast(s('downloading', { count: ids.length }), { type: 'info' });
    for (const id of ids) {
        const link = document.createElement('a');
        link.href = `/api/download/${id}`;
        link.download = '';
        document.body.append(link);
        link.click();
        link.remove();
        await sleep(DOWNLOAD_SPACING);
    }
}

async function fetchFile(id) {
    const response = await fetch(`/api/download/${id}`, { credentials: 'same-origin' });
    if (!response.ok) throw new Error(`Download of ${id} failed with ${response.status}`);
    const blob = await response.blob();
    const name = /filename="([^"]+)"/.exec(response.headers.get('Content-Disposition') ?? '')?.[1] ?? `photo-${id}`;
    return new File([blob], name, { type: blob.type });
}

async function share(selection) {
    const ids = selection.ids();
    if (ids.length > MAX_SHARE) {
        toast(s('selectUpTo', { max: MAX_SHARE }), { type: 'info' });
        return;
    }
    try {
        await navigator.share({ files: await Promise.all(ids.map(fetchFile)) });
    } catch (error) {
        if (error.name !== 'AbortError') toast(s('shareFailed'), { type: 'error' });
    }
}

const ACTIONS = {
    favorite,
    download,
    share,
    move,
    delete: trash,
    restore,
    'delete-forever': deleteForever,
};

/**
 * @param {'favorite' | 'download' | 'share' | 'move' | 'delete' | 'restore' | 'delete-forever'} name
 * @param {{ids: () => number[], contextKey: string, clear: () => void}} selection `element.fpSelection` of a grid
 * @param {{userId: string}} user
 */
export function runAction(name, selection, { userId }) {
    return ACTIONS[name](selection, userId);
}

/**
 * Runs a bar action on a list of ids that aren't (all) in the grid: the page-level buttons of the
 * trash ("Restore all", "Empty trash"). `title` replaces the title of the permanent delete dialog.
 * @param {'restore' | 'delete-forever'} name
 * @param {number[]} ids
 * @param {{contextKey: string, title?: string}} options
 */
export function runOnIds(name, ids, { contextKey, title }) {
    return ACTIONS[name]({ ids: () => ids, contextKey, clear() {} }, '', title);
}
