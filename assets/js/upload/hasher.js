// Hashes the files of the upload page in Web Workers (hash-worker.js), two files at a time.

const WORKERS = 2;

function createWorker() {
    return new Worker(new URL('./hash-worker.js', import.meta.url), { type: 'module' });
}

export class Hasher {
    /**
     * @param {{onProgress: (item, bytesRead: number) => void, onDone: (item, hash: string|null, error?: string) => void}} callbacks
     *   `hash` is null when the file couldn't be hashed (it is then uploaded without a hash), and
     *   `error` says why
     */
    constructor({ onProgress, onDone }) {
        this.onProgress = onProgress;
        this.onDone = onDone;
        this.queue = [];
        /** @type {{worker: Worker, item: object|null}[]} */
        this.slots = [];
        this.broken = false;
        this.warned = false;
    }

    /** Hash failures fall back to "no hash", so make them diagnosable (once per page) */
    warn(error) {
        if (this.warned) return;
        this.warned = true;
        console.warn('Hashing failed, files are uploaded without a hash:', error);
    }

    get idle() {
        return this.queue.length === 0 && this.slots.every((slot) => slot.item === null);
    }

    /** @param {{id: number, file: File}} item */
    add(item) {
        if (this.queue.includes(item) || this.slots.some((slot) => slot.item === item)) return;
        this.queue.push(item);
        this.pump();
    }

    /** Drops a file that is still waiting (one being hashed finishes, and its result is ignored). */
    remove(item) {
        const index = this.queue.indexOf(item);
        if (index >= 0) this.queue.splice(index, 1);
    }

    finish(slot, hash, error) {
        const { item } = slot;
        slot.item = null;
        if (error) this.warn(error);
        this.onDone(item, hash, error);
        this.pump();
    }

    newSlot() {
        const slot = { worker: createWorker(), item: null };
        slot.worker.addEventListener('message', (event) => {
            const { progress, hash, error } = event.data;
            if (progress !== undefined) {
                this.onProgress(slot.item, progress);
            } else {
                this.finish(slot, hash ?? null, error);
            }
        });
        // The worker couldn't start (e.g. its script failed to load): upload without hashes
        slot.worker.addEventListener('error', (event) => {
            event.preventDefault();
            this.broken = true;
            this.warn(event.message || 'the hash worker failed to start');
            if (slot.item) this.finish(slot, null, event.message || 'The hash worker failed');
        });
        this.slots.push(slot);
        return slot;
    }

    pump() {
        if (this.broken) {
            for (const item of this.queue.splice(0)) this.onDone(item, null, 'The hash worker failed');
            return;
        }
        while (this.queue.length > 0) {
            const slot = this.slots.find((s) => s.item === null)
                ?? (this.slots.length < WORKERS ? this.newSlot() : null);
            if (!slot) return;
            slot.item = this.queue.shift();
            slot.worker.postMessage({ id: slot.item.id, file: slot.item.file });
        }
    }

    destroy() {
        this.queue = [];
        for (const slot of this.slots) slot.worker.terminate();
        this.slots = [];
    }
}
