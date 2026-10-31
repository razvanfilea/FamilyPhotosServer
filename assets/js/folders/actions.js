// Folder management buttons: "New folder" on the Folders tab and Edit on the folder page. Share and
// Delete are HTMX buttons (`hx-get` into the app dialog); the folder name form is HTMX's too.

import { register } from '../core/registry.js';
import { openFolderNameDialog } from './name-dialog.js';

register('new-folder-button', (button) => {
    button.addEventListener('click', () => openFolderNameDialog());
});

register('folder-actions', (actions) => {
    const folderId = Number(actions.dataset.folderId);

    actions.addEventListener('click', (event) => {
        if (/** @type {Element} */ (event.target).closest('[data-action="edit"]')) openFolderNameDialog({ folderId });
    });
});
