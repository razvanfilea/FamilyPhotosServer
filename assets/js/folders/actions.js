// Folder management buttons: "New folder" on the Folders tab, and Share / Edit / Delete on the
// folder page.

import { register } from '../core/registry.js';
import { confirm, escapeHtml, openDialog } from '../core/dialog.js';
import { ApiError, deleteJson } from '../core/api.js';
import { openFolderNameDialog } from './name-dialog.js';
import { flashToast } from './flash.js';

register('new-folder-button', (button) => {
    button.addEventListener('click', async () => {
        const folder = await openFolderNameDialog({ submit: 'create' });
        // Creating returns an existing folder with the same name; opening it is what Android does too
        if (folder) location.assign(`/folders/${folder.id}`);
    });
});

register('folder-actions', (actions) => {
    const folderId = Number(actions.dataset.folderId);
    const { folderName, messageRenamed, messageDeleted, deleteTitle, deleteBody, deleteConfirm } = actions.dataset;

    async function rename() {
        const folder = await openFolderNameDialog({ folderId, submit: 'rename' });
        if (!folder) return;
        flashToast(messageRenamed);
        location.reload();
    }

    async function remove() {
        let deleted = false;
        await confirm({
            title: deleteTitle,
            body: `<p>${deleteBody.replace('{name}', () => escapeHtml(folderName))}</p>`,
            confirmLabel: deleteConfirm,
            danger: true,
            onConfirm: async () => {
                try {
                    await deleteJson(`/api/folders/${folderId}`);
                    deleted = true;
                } catch (error) {
                    // The 409 "Move or delete the photos in this folder first" was shown as a toast
                    if (!(error instanceof ApiError) || error.status !== 409) throw error;
                }
            },
        });
        if (!deleted) return;
        flashToast(messageDeleted);
        location.assign('/folders');
    }

    actions.addEventListener('click', (event) => {
        const action = event.target.closest('[data-action]')?.dataset.action;
        if (action === 'edit') rename();
        else if (action === 'share') openDialog({ url: `/ui/dialogs/sharing/${folderId}` });
        else if (action === 'delete') remove();
    });
});
