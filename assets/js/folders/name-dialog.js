// The create/rename folder dialog. It is a <dialog> of its own (not the app dialog), so it can be
// opened on top of other dialogs, e.g. from the destination picker inside the move dialog.

import { register } from '../core/registry.js';
import { popScope, pushScope } from '../core/keyboard.js';

const SCOPE = 'folder-name-dialog';

/** @type {WeakMap<HTMLDialogElement, {resolve: (result: object|null) => void}>} */
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
 * Opens the dialog to create a folder, or to rename one (`folderId`). The form is HTMX's: it posts
 * to `/ui/folders[/{id}]` and the response redirects (or shows the reason in the form again).
 *
 * With `returnOnly` confirming only hands back the choice, without any request (the destination
 * picker creates the folder when the photos are moved).
 *
 * @param {{folderId?: number, initialName?: string, initialPublic?: boolean, returnOnly?: boolean}} options
 * @returns {Promise<{name: string, isPublic: boolean}|null>} the choice in `returnOnly` mode; null if it was dismissed
 */
export function openFolderNameDialog({ folderId, initialName = '', initialPublic = false, returnOnly = false } = {}) {
    return new Promise((resolve) => {
        const dialog = createDialog();
        const box = dialog.querySelector('.modal-box');
        let result = null;

        sessions.set(dialog, {
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
            ? { name: initialName, public: initialPublic, ...(returnOnly && { return: true }) }
            : { folder_id: folderId };

        (async () => {
            await window.htmx.ajax('GET', `/ui/dialogs/folder-name?${new URLSearchParams(/** @type {any} */ (query))}`, {
                target: box,
                swap: 'innerHTML',
            });
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
    const input = /** @type {HTMLInputElement} */ (form.elements.namedItem('name'));
    const confirm = /** @type {HTMLButtonElement} */ (form.querySelector('[data-testid="folder-name-confirm"]'));
    const error = form.querySelector('[data-role="error"]');
    const warning = form.querySelector('[data-role="warning"]');
    const renaming = form.hasAttribute('data-renaming');
    const initialPublic = form.dataset.initialPublic === 'true';

    const isPublic = () => /** @type {HTMLInputElement|null} */ (form.querySelector('input[name="folder-visibility"]:checked'))?.value === 'family';

    const update = () => {
        confirm.disabled = input.value.trim() === '';

        const changed = renaming && isPublic() !== initialPublic;
        warning.hidden = !changed;
        if (changed) {
            warning.textContent = isPublic() ? form.dataset.warningToFamily : form.dataset.warningToPersonal;
        }
    };
    const edited = () => {
        error.hidden = true;
        update();
    };
    form.addEventListener('input', edited);
    form.addEventListener('change', edited);
    update();
    // Back from a refusal: the cursor goes to the name that was refused
    if (!error.hidden) {
        input.focus();
        input.select();
    }

    // Without a request: the picker takes the choice
    if (form.hasAttribute('data-return')) {
        form.addEventListener('submit', (event) => {
            event.preventDefault();
            if (!confirm.disabled) sessions.get(dialog).resolve({ name: input.value.trim(), isPublic: isPublic() });
        });
    }
});
