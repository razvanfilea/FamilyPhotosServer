// The create/rename folder dialog. It is a <dialog> of its own (not the app dialog), so it can be
// opened on top of other dialogs, e.g. from the destination picker inside the move dialog.

import { register } from '../core/registry.js';
import { postJson, api, ApiError } from '../core/api.js';
import { popScope, pushScope } from '../core/keyboard.js';

const SCOPE = 'folder-name-dialog';

/** @type {WeakMap<HTMLDialogElement, {submit: string, folderId?: number, resolve: (result: object|null) => void}>} */
const sessions = new WeakMap();

function createDialog() {
    const dialog = document.createElement('dialog');
    dialog.className = 'modal modal-bottom sm:modal-middle';
    dialog.setAttribute('aria-labelledby', 'folder-name-title');
    dialog.dataset.testid = 'folder-name-dialog';
    dialog.innerHTML = `
        <div class="modal-box"></div>
        <form method="dialog" class="modal-backdrop"><button type="submit" tabindex="-1">Close</button></form>`;
    return dialog;
}

/**
 * Opens the dialog to create a folder, or to rename one (`folderId`).
 *
 * `submit` says what confirming does: `'create'` creates the folder (`POST /api/folders`),
 * `'rename'` updates it (`PATCH /api/folders/{id}`), and `'return'` only hands back the choice
 * without calling the API (the destination picker creates the folder when the photos are moved).
 *
 * @param {{folderId?: number, initialName?: string, initialPublic?: boolean, submit?: 'create'|'rename'|'return'}} options
 * @returns {Promise<{id?: number, name: string, isPublic: boolean}|null>} null if it was dismissed
 */
export function openFolderNameDialog({ folderId, initialName = '', initialPublic = false, submit = 'create' } = {}) {
    return new Promise((resolve) => {
        const dialog = createDialog();
        const box = dialog.querySelector('.modal-box');
        let result = null;

        sessions.set(dialog, {
            submit,
            folderId,
            resolve: (value) => {
                result = value;
                dialog.close();
            },
        });

        dialog.addEventListener('close', () => {
            popScope(SCOPE);
            dialog.remove();
            resolve(result);
        });
        dialog.addEventListener('click', (event) => {
            if (/** @type {Element} */ (event.target).closest('[data-dialog-close]')) dialog.close();
        });

        document.body.append(dialog);
        const query = folderId === undefined
            ? { name: initialName, public: initialPublic }
            : { folder_id: folderId };

        (async () => {
            try {
                await window.htmx.ajax('GET', `/ui/dialogs/folder-name?${new URLSearchParams(/** @type {any} */ (query))}`, {
                    target: box,
                    swap: 'innerHTML',
                });
            } catch {
                // Reported below
            }
            // A failed request leaves the dialog empty; the global handler already showed the toast
            if (!box.firstElementChild) {
                dialog.remove();
                resolve(null);
                return;
            }
            pushScope(SCOPE, { modal: true });
            dialog.showModal();
        })();
    });
}

register('folder-name-form', (/** @type {HTMLFormElement} */ form) => {
    const dialog = form.closest('dialog');
    const session = sessions.get(dialog);
    const input = /** @type {HTMLInputElement} */ (form.elements.namedItem('name'));
    const confirm = form.querySelector('[data-testid="folder-name-confirm"]');
    const error = form.querySelector('[data-role="error"]');
    const warning = form.querySelector('[data-role="warning"]');
    const renaming = form.hasAttribute('data-renaming');
    const initialName = form.dataset.initialName;
    const initialPublic = form.dataset.initialPublic === 'true';
    let busy = false;

    const isPublic = () => form.querySelector('input[name="folder-visibility"]:checked')?.value === 'family';
    const trimmedName = () => input.value.trim();

    const update = () => {
        confirm.disabled = busy || trimmedName() === '';
        error.hidden = true;

        const changed = renaming && isPublic() !== initialPublic;
        warning.hidden = !changed;
        if (changed) {
            warning.textContent = isPublic() ? form.dataset.warningToFamily : form.dataset.warningToPersonal;
        }
    };
    form.addEventListener('input', update);
    form.addEventListener('change', update);
    update();

    form.addEventListener('submit', async (event) => {
        event.preventDefault();
        if (confirm.disabled) return;

        const name = trimmedName();
        const outcome = { name, isPublic: isPublic() };

        if (session.submit === 'return') {
            session.resolve(outcome);
            return;
        }
        if (renaming && name === initialName && outcome.isPublic === initialPublic) {
            session.resolve(null);
            return;
        }

        busy = true;
        confirm.disabled = true;
        confirm.classList.add('btn-disabled');
        try {
            const folder = renaming
                ? await api(`/api/folders/${session.folderId}`, {
                    method: 'PATCH',
                    json: { name, is_public: outcome.isPublic },
                    silent: true,
                })
                : await postJson('/api/folders', { name, is_public: outcome.isPublic }, { silent: true });
            session.resolve({ ...outcome, id: folder.id, name: folder.name });
        } catch (failure) {
            if (!(failure instanceof ApiError)) throw failure;
            error.textContent = failure.message;
            busy = false;
            confirm.classList.remove('btn-disabled');
            confirm.disabled = false;
            error.hidden = false;
        }
    });
});
