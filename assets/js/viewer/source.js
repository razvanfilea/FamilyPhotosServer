// The photos the viewer walks through: a window of a context
// around the current photo, extended from /api/context/neighbors as the viewer nears its edges,
// so the whole context can be walked without loading it all.

import { getJson } from '../core/api.js';

/** Photos loaded before and after the opened photo */
export const WINDOW_BEFORE = 30;
export const WINDOW_AFTER = 30;
/** The window is extended once the current photo is this close to one of its edges */
export const EXTEND_THRESHOLD = 10;
/** Photos loaded by each extension */
const EXTEND_LIMIT = 50;

const NEIGHBORS = '/api/context/neighbors';

/** @typedef {{id: number, name: string, created_at: number, is_video: boolean, thumb_hash: string|null, file_size: number, user_id: string|null, folder_id: number|null, is_favorite: boolean, trashed_on: number|null, deletable: boolean}} ViewerPhoto */

export class ContextSource {
    /** @param {string} contextKey */
    constructor(contextKey) {
        this.contextKey = contextKey;
        /** Context index of `photos[0]` */
        this.start = 0;
        /** @type {ViewerPhoto[]} */
        this.photos = [];
        this.total = 0;
        /** @type {{before?: Promise<boolean>, after?: Promise<boolean>}} */
        this.loading = {};
    }

    /** Context index after the last loaded photo */
    get end() {
        return this.start + this.photos.length;
    }

    /**
     * Loads the window around a photo.
     * @returns {Promise<number>} the photo's context index
     * @throws {ApiError} 404 when the photo isn't in the context
     */
    async open(photoId) {
        const response = await getJson(NEIGHBORS, {
            query: { ctx: this.contextKey, photo_id: photoId, before: WINDOW_BEFORE, after: WINDOW_AFTER },
            silent: true,
        });
        this.start = response.start;
        this.photos = response.photos;
        this.total = response.total;
        this.loading = {};
        return response.index;
    }

    /** @returns {ViewerPhoto|undefined} */
    get(index) {
        return index >= this.start ? this.photos[index - this.start] : undefined;
    }

    /** @returns {number} the context index of a loaded photo, -1 if it isn't loaded */
    indexOf(photoId) {
        const position = this.photos.findIndex((photo) => photo.id === photoId);
        return position < 0 ? -1 : this.start + position;
    }

    /**
     * Extends the window if `index` is within EXTEND_THRESHOLD of one of its edges.
     * @returns {Promise<boolean>} whether photos were added
     */
    async ensure(index) {
        const loads = [];
        if (index - this.start < EXTEND_THRESHOLD && this.start > 0) loads.push(this.extend('before'));
        if (this.end - index <= EXTEND_THRESHOLD && this.end < this.total) loads.push(this.extend('after'));
        const results = await Promise.all(loads);
        return results.some(Boolean);
    }

    /** @param {'before'|'after'} direction */
    extend(direction) {
        this.loading[direction] ??= this.load(direction).finally(() => {
            delete this.loading[direction];
        });
        return this.loading[direction];
    }

    /**
     * Loads EXTEND_LIMIT photos past an edge. The request overlaps the window by one photo: if
     * that photo isn't the expected one (the context changed in a way the window doesn't know
     * about), the window is reloaded around the edge photo instead.
     */
    async load(direction) {
        const edge = direction === 'after' ? this.photos.at(-1) : this.photos[0];
        if (!edge) return false;
        const start = direction === 'after' ? this.end - 1 : Math.max(0, this.start - EXTEND_LIMIT);
        const limit = direction === 'after' ? EXTEND_LIMIT + 1 : this.start - start + 1;

        const response = await getJson(NEIGHBORS, { query: { ctx: this.contextKey, start, limit }, silent: true });
        // The window may have changed meanwhile (photos removed)
        const current = direction === 'after' ? this.photos.at(-1) : this.photos[0];
        const overlap = direction === 'after' ? response.photos[0] : response.photos.at(-1);
        if (current?.id !== edge.id || overlap?.id !== edge.id) {
            await this.open(edge.id);
            return true;
        }

        this.total = response.total;
        if (direction === 'after') {
            this.photos.push(...response.photos.slice(1));
        } else {
            this.photos.unshift(...response.photos.slice(0, -1));
            this.start = response.start;
        }
        return response.photos.length > 1;
    }

    /**
     * Drops photos removed from the context.
     * @param {number[]} ids
     * @returns {number} how many were in the window
     */
    remove(ids) {
        const removed = new Set(ids.map(Number));
        const before = this.photos.length;
        this.photos = this.photos.filter((photo) => !removed.has(photo.id));
        const count = before - this.photos.length;
        this.total = Math.max(0, this.total - count);
        return count;
    }

    /** Merges new data into a loaded photo. */
    update(photoId, changes) {
        const photo = this.photos.find((candidate) => candidate.id === photoId);
        if (photo) Object.assign(photo, changes);
        return photo;
    }
}
