// The delete dialog (Android's DeletePhotosDialog), shared by the viewer, the selection bar, the
// trash and the duplicates page:
//
//     const deleted = await openDeleteDialog({
//         ids: [12, 13],
//         permanent: false,
//         onConfirm: async ({ setStatus }) => { await postJson('/api/trash', ids, { silent: true }); },
//     });
//
// While `onConfirm` runs the confirm button shows a spinner and the dialog can't be dismissed. It
// closes (resolving to true) only when `onConfirm` succeeds; if it throws, the error message is
// shown in the dialog, which stays open. Call the API with `silent: true` so the error isn't also
// shown as a toast. The markup and texts are templates/dialogs/delete_photos.html.

import { closeDialog, openDialog } from '../core/dialog.js';
import { t } from '../core/strings.js';

/** Thumbnails shown before the "+N" chip */
const MAX_THUMBNAILS = 8;

function thumbnails(list, ids) {
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
        more.className = 'delete-dialog__more';
        more.textContent = `+${ids.length - shown.length}`;
        list.append(more);
    }
}

/**
 * @param {{ids: number[], permanent?: boolean, onConfirm: (helpers: {setStatus: (text: string) => void}) => Promise<void>|void, title?: string, message?: string}} options
 *   `title` / `message` replace the default texts; `setStatus` shows a progress line (e.g. "3 of 10").
 * @returns {Promise<boolean>} true once deleted, false when cancelled
 */
export function openDeleteDialog({ ids, permanent = false, onConfirm, title, message }) {
    const template = /** @type {HTMLTemplateElement} */ (document.getElementById('delete-dialog-template'));
    const texts = template.dataset;
    const fragment = /** @type {DocumentFragment} */ (template.content.cloneNode(true));
    const slot = (name) => fragment.querySelector(`[data-slot="${name}"]`);
    const one = ids.length === 1;

    slot('title').textContent = title
        ?? (permanent ? texts.titlePermanent : one ? texts.titleOne : texts.titleMany);
    slot('message').textContent = message
        ?? (permanent
            ? (one ? texts.messagePermanentOne : texts.messagePermanentMany)
            : (one ? texts.messageOne : texts.messageMany));
    slot('icon-trash').hidden = permanent;
    slot('icon-permanent').hidden = !permanent;
    thumbnails(slot('thumbs'), ids);

    const confirmButton = slot('confirm');
    const cancelButton = slot('cancel');
    const status = slot('status');
    const error = slot('error');
    confirmButton.textContent = permanent ? texts.confirmPermanent : texts.confirm;

    const dialog = document.getElementById('app-dialog');
    const backdrop = dialog.querySelector('.modal-backdrop button');
    let busy = false;
    let deleted = false;

    const preventDismiss = (event) => {
        if (busy) event.preventDefault();
    };

    function setBusy(value) {
        busy = value;
        confirmButton.disabled = value;
        cancelButton.disabled = value;
        backdrop.disabled = value;
        confirmButton.querySelector('.loading')?.remove();
        if (value) {
            const spinner = document.createElement('span');
            spinner.className = 'loading loading-spinner loading-sm';
            confirmButton.prepend(spinner);
        }
    }

    const setStatus = (text) => {
        status.textContent = text;
        status.hidden = !text;
    };

    cancelButton.addEventListener('click', () => closeDialog());
    confirmButton.addEventListener('click', async () => {
        error.hidden = true;
        setBusy(true);
        try {
            await onConfirm({ setStatus });
            deleted = true;
            setBusy(false);
            closeDialog('confirm');
        } catch (failure) {
            setBusy(false);
            setStatus('');
            error.textContent = failure?.message || t('genericError');
            error.hidden = false;
        }
    });

    return new Promise((resolve) => {
        dialog.addEventListener('cancel', preventDismiss);
        openDialog({
            node: fragment,
            onClose: () => {
                dialog.removeEventListener('cancel', preventDismiss);
                backdrop.disabled = false;
                resolve(deleted);
            },
        });
        cancelButton.focus();
    });
}
