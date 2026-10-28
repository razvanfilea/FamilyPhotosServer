// Turning the picker's choice into what the API wants.

import { postJson } from '../core/api.js';

/**
 * Creates the folder of a `new` destination (the server returns the existing folder with the same
 * name, like the Android app) and returns the folder and visibility parameters of the move/upload
 * request: `{folderId}` for a folder, `{makePublic}` for the root of the Personal or Family gallery.
 * @param {{kind: string, folderId?: number, name?: string, isPublic?: boolean}} destination
 * @returns {Promise<{folderId?: number, makePublic?: boolean}>}
 * @throws {ApiError} if creating the folder fails
 */
export async function resolveDestination(destination) {
    switch (destination.kind) {
        case 'root':
            return { makePublic: destination.isPublic };
        case 'folder':
            return { folderId: destination.folderId };
        case 'new': {
            const folder = await postJson('/api/folders', { name: destination.name, is_public: destination.isPublic });
            return { folderId: folder.id };
        }
        default:
            throw new Error(`Unknown destination kind: ${destination.kind}`);
    }
}
