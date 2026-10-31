// The move dialog: the destination picker (`/ui/dialogs/move`) in `#app-dialog`, with the
// thumbnails of the photos above it. Used by the selection bar and the viewer:
//
//     const result = await openMoveDialog({ ids });   // {moved: Photo[]} or null when cancelled
//
// It doesn't show a success toast: the caller does ("N photos moved"). `moved` is what the
// server moved, which can be fewer than `ids` (photos the user can't move are skipped). Errors
// are shown in the dialog, which stays open.

import { ApiError, postJson } from '../core/api.js';
import { closeDialog, openDialog } from '../core/dialog.js';
import { resolveDestination } from '../destination-picker/destination.js';
import { DESTINATION_CHANGE_EVENT } from '../destination-picker/picker.js';
import { BULK_CHUNK, chunk, visibilityChange } from './math.js';
import { s } from './strings.js';

/** Thumbnails shown before the "+N" chip */
const MAX_THUMBNAILS = 8;

function showcase(slot, ids) {
    const list = document.createElement('ul');
    list.className = 'move-showcase__thumbs';
    const shown = ids.length > MAX_THUMBNAILS ? ids.slice(0, MAX_THUMBNAILS - 1) : ids;
    for (const id of shown) {
        const item = document.createElement('li');
        const image = document.createElement('img');
        image.src = `/api/preview/${id}`;
        image.alt = '';
        image.decoding = 'async';
        item.append(image);
        list.append(item);
    }
    if (shown.length < ids.length) {
        const more = document.createElement('li');
        more.className = 'move-showcase__more';
        more.textContent = `+${ids.length - shown.length}`;
        list.append(more);
    }
    const label = document.createElement('p');
    label.className = 'move-showcase__count';
    label.textContent = s('movePhotos', { count: ids.length });
    slot.replaceChildren(list, label);
}

/** Who can see the photos now, as far as the tiles on the page tell */
function visibilityOf(ids) {
    const sources = { public: false, private: false };
    for (const id of ids) {
        const tile = document.querySelector(`.photo-tile[data-photo-id="${id}"]`);
        if (!tile) continue;
        if (tile.dataset.owner) sources.private = true;
        else sources.public = true;
    }
    return sources;
}

/** POSTs to /api/move in chunks; a failure after some photos moved ends with those */
async function moveAll(ids, target) {
    const query = target.folderId !== undefined
        ? { target_folder_id: target.folderId }
        : { make_public: target.makePublic };
    const moved = [];
    for (const part of chunk(ids, BULK_CHUNK)) {
        try {
            moved.push(...await postJson('/api/move', part, { query, silent: true }));
        } catch (error) {
            if (moved.length > 0) break;
            throw error instanceof ApiError && error.status === 404 ? new Error(s('moveNotFound')) : error;
        }
    }
    return moved;
}

/**
 * @param {{ids: number[]}} options
 * @returns {Promise<{moved: {id: number, user_id: string | null, folder_id: number | null}[]} | null>} the photos moved, or null if cancelled or the dialog didn't open
 */
export function openMoveDialog({ ids }) {
    return new Promise((resolve) => {
        let result = null;
        const dialog = document.getElementById('app-dialog');
        const backdrop = dialog.querySelector('.modal-backdrop button');
        const keepOpen = (event) => event.preventDefault();

        function setup(root) {
            const confirm = root.querySelector('[data-testid="move-confirm"]');
            const cancel = root.querySelector('[data-testid="move-cancel"]');
            const picker = () => root.querySelector('[data-component~="destination-picker"]').fpPicker;
            const sources = visibilityOf(ids);

            showcase(root.querySelector('[data-slot="showcase"]'), ids);

            const note = document.createElement('p');
            note.className = 'move-note';
            note.dataset.testid = 'move-note';
            note.setAttribute('role', 'status');
            note.hidden = true;
            const error = document.createElement('p');
            error.className = 'move-error';
            error.dataset.testid = 'move-error';
            error.setAttribute('role', 'alert');
            error.hidden = true;
            root.querySelector('.modal-action').before(note, error);

            function showNote(destination) {
                const change = destination && visibilityChange(sources, destination.isPublic);
                note.textContent = change === 'family' ? s('moveNoteFamily') : change === 'private' ? s('moveNoteHidden') : '';
                note.hidden = !change;
            }
            root.addEventListener(DESTINATION_CHANGE_EVENT, (event) => showNote(event.detail.destination));
            // The picker may have chosen its default before we listened
            showNote(picker()?.getValue() ?? null);

            function setBusy(busy) {
                confirm.disabled = busy;
                cancel.disabled = busy;
                backdrop.disabled = busy;
                confirm.querySelector('.loading')?.remove();
                if (busy) {
                    const spinner = document.createElement('span');
                    spinner.className = 'loading loading-spinner loading-sm';
                    confirm.prepend(spinner);
                    dialog.addEventListener('cancel', keepOpen);
                } else {
                    dialog.removeEventListener('cancel', keepOpen);
                }
            }

            confirm.addEventListener('click', async () => {
                const destination = picker()?.getValue();
                if (!destination) return;
                error.hidden = true;
                setBusy(true);
                try {
                    const moved = await moveAll(ids, await resolveDestination(destination, { silent: true }));
                    if (moved.length === 0) throw new Error(s('moveNothing'));
                    result = { moved };
                    setBusy(false);
                    closeDialog('confirm');
                } catch (failure) {
                    setBusy(false);
                    error.textContent = failure instanceof Error ? failure.message : String(failure);
                    error.hidden = false;
                }
            });
        }

        const finish = () => {
            dialog.removeEventListener('cancel', keepOpen);
            backdrop.disabled = false;
            resolve(result);
        };

        // The dialog opens once the fragment is swapped in; if loading failed (reported by the
        // global handlers) it never does
        openDialog({ url: `/ui/dialogs/move?count=${ids.length}`, onClose: finish })
            .then(() => {
                const root = document.querySelector('#app-dialog-content [data-component~="move-dialog"]');
                if (root) setup(root);
                else finish();
            })
            .catch(finish);
    });
}
