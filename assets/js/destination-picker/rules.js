// The rules of the destination picker, as pure functions (they follow Android's UploadComposables).
//
// Destination = {kind: 'root', isPublic} | {kind: 'folder', folderId, name, isPublic, type}
//             | {kind: 'new', name, isPublic}

/** The Personal gallery, which is where photos go by default. */
export const DEFAULT_DESTINATION = Object.freeze({ kind: 'root', isPublic: false });

/**
 * What choosing a filter chip does to the current choice: Family and Personal pick the matching
 * gallery, Shared clears the choice (a shared folder must be picked), All keeps it.
 * @returns {object|null}
 */
export function destinationForFilter(filter, current) {
    switch (filter) {
        case 'family':
            return { kind: 'root', isPublic: true };
        case 'personal':
            return { kind: 'root', isPublic: false };
        case 'shared':
            return null;
        default:
            return current;
    }
}

/** Which kind of choice the picker shows (`data-selection`): root, folder, new or none. */
export function selectionKind(destination) {
    return destination?.kind ?? 'none';
}

/** The type label of a destination, for the "selected folder" row. */
export function typeLabel(destination) {
    if (destination.kind === 'folder' && destination.type === 'shared') return 'Shared';
    return destination.isPublic ? 'Family' : 'Personal';
}
