// The destination picker: where moved or uploaded photos go. Markup: templates/destination_picker.
//
//   const picker = element.fpPicker;      // element = [data-component~="destination-picker"]
//   picker.getValue();                    // Destination | null
//   picker.onChange((destination) => …);  // returns an unsubscribe function
//   element.addEventListener('fp:destination-change', (e) => e.detail.destination);
//
// A Destination is {kind: 'root', isPublic} | {kind: 'folder', folderId, name, isPublic, type}
// | {kind: 'new', name, isPublic}, or null when nothing is chosen (e.g. under the Shared filter).
// `resolveDestination(destination)` (destination.js) turns it into API parameters.

import { register } from '../core/registry.js';
import { emit } from '../core/events.js';
import { FILTER_CHANGE_EVENT } from '../folders/browser.js';
import { openFolderNameDialog } from '../folders/name-dialog.js';
import { DEFAULT_DESTINATION, destinationForFilter, selectionKind, typeLabel } from './rules.js';

export { resolveDestination } from './destination.js';

/** Fired on the picker element whenever the choice changes: {destination} */
export const DESTINATION_CHANGE_EVENT = 'fp:destination-change';

function setIcon(container, name) {
    const use = container.querySelector('use');
    use.setAttribute('href', use.getAttribute('href').replace(/#.*$/, `#${name}`));
}

register('destination-picker', (root) => {
    const personal = root.querySelector('[data-dest="personal"]');
    const family = root.querySelector('[data-dest="family"]');
    const create = root.querySelector('[data-dest="new"]');
    const selectedName = root.querySelector('[data-role="selected-name"]');
    const selectedType = root.querySelector('[data-role="selected-type"]');
    const selectedIcon = root.querySelector('[data-role="selected-icon"]');
    const listeners = new Set();

    let value = null;

    function render() {
        root.dataset.selection = selectionKind(value);
        personal.setAttribute('aria-pressed', String(value?.kind === 'root' && !value.isPublic));
        family.setAttribute('aria-pressed', String(value?.kind === 'root' && value.isPublic));

        if (value?.kind === 'folder' || value?.kind === 'new') {
            selectedName.textContent = value.name;
            selectedType.textContent = typeLabel(value);
            setIcon(selectedIcon, value.kind === 'new' ? 'create_new_folder' : 'folder');
        }
    }

    function set(next) {
        value = next;
        render();
        for (const listener of listeners) listener(value);
        emit(root, DESTINATION_CHANGE_EVENT, { destination: value });
    }

    const initialFolderId = root.dataset.initialFolderId;
    if (initialFolderId) {
        value = {
            kind: 'folder',
            folderId: Number(initialFolderId),
            name: root.dataset.initialFolderName,
            isPublic: root.dataset.initialFolderPublic === 'true',
            type: root.dataset.initialFolderType,
        };
    } else {
        value = destinationForFilter(root.dataset.filter, DEFAULT_DESTINATION);
    }

    personal.addEventListener('click', () => set({ kind: 'root', isPublic: false }));
    family.addEventListener('click', () => set({ kind: 'root', isPublic: true }));

    create.addEventListener('click', async () => {
        const folder = await openFolderNameDialog({ submit: 'return' });
        if (folder) set({ kind: 'new', name: folder.name, isPublic: folder.isPublic });
    });

    root.querySelector('[data-role="clear"]').addEventListener('click', () => {
        set(destinationForFilter(root.dataset.filter, DEFAULT_DESTINATION));
    });

    root.addEventListener('click', (event) => {
        const card = event.target.closest('[data-role="folders"] button[data-folder-id]');
        if (!card) return;
        set({
            kind: 'folder',
            folderId: Number(card.dataset.folderId),
            name: card.dataset.folderName,
            isPublic: card.dataset.public === 'true',
            type: card.dataset.type,
        });
    });

    root.addEventListener(FILTER_CHANGE_EVENT, (event) => {
        set(destinationForFilter(event.detail.filter, value));
    });

    root.fpPicker = {
        getValue: () => (value ? { ...value } : null),
        onChange(callback) {
            listeners.add(callback);
            return () => listeners.delete(callback);
        },
    };

    render();
    // The first event also tells wrappers like the move dialog whether there is a choice
    emit(root, DESTINATION_CHANGE_EVENT, { destination: value });

    return () => {
        delete root.fpPicker;
        listeners.clear();
    };
});

/** The "Move photos" button of the move dialog is only enabled while there is a destination. */
register('move-dialog', (dialog) => {
    const confirm = dialog.querySelector('[data-testid="move-confirm"]');
    dialog.addEventListener(DESTINATION_CHANGE_EVENT, (event) => {
        confirm.disabled = event.detail.destination === null;
    });
});
