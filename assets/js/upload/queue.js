// The upload queue: 3 files at a time, automatic retries of network errors and 5xx responses
// (1 s, 4 s, then 10 s apart, i.e. 3 retries like the Android app), and cancelling.
//
// The transport is injected: `send(item, {signal, onProgress(loadedBytes)})` resolves to
// `{ok: true, photo}` or `{ok: false, status, message}` (status 0 = network error), and rejects
// with an AbortError when `signal` aborts. queue.js sets `item.phase`, `item.loaded`,
// `item.error` and `item.photo`.

const CONCURRENCY = 3;
export const RETRY_DELAYS = Object.freeze([1000, 4000, 10000]);

/** Network errors and server errors are worth retrying; other errors won't change. */
export function isRetryable(status) {
    return status === 0 || status >= 500;
}

function abortError() {
    return new DOMException('Cancelled', 'AbortError');
}

/** Waits `ms`, or rejects with an AbortError as soon as `signal` aborts. */
function sleep(ms, signal) {
    return new Promise((resolve, reject) => {
        if (signal.aborted) {
            reject(abortError());
            return;
        }
        const timer = setTimeout(resolve, ms);
        signal.addEventListener('abort', () => {
            clearTimeout(timer);
            reject(abortError());
        }, { once: true });
    });
}

export class UploadQueue {
    /**
     * @param {{send: Function, onChange?: (item) => void, onIdle?: () => void, concurrency?: number,
     *   delays?: readonly number[], wait?: (ms: number, signal: AbortSignal) => Promise<void>}} options
     */
    constructor({ send, onChange = () => {}, onIdle = () => {}, concurrency = CONCURRENCY, delays = RETRY_DELAYS, wait = sleep }) {
        this.send = send;
        this.onChange = onChange;
        this.onIdle = onIdle;
        this.concurrency = concurrency;
        this.delays = delays;
        this.wait = wait;
        this.waiting = [];
        /** @type {Map<object, AbortController>} */
        this.active = new Map();
    }

    get idle() {
        return this.waiting.length === 0 && this.active.size === 0;
    }

    push(item) {
        item.phase = 'queued';
        item.loaded = 0;
        item.error = null;
        this.waiting.push(item);
        this.onChange(item);
        this.pump();
    }

    /** Stops everything: in-flight uploads are aborted, and every file still waiting is cancelled. */
    cancel() {
        for (const item of this.waiting.splice(0)) {
            item.phase = 'cancelled';
            this.onChange(item);
        }
        for (const controller of this.active.values()) {
            controller.abort();
        }
        if (this.idle) this.onIdle();
    }

    pump() {
        while (this.active.size < this.concurrency && this.waiting.length > 0) {
            this.run(this.waiting.shift());
        }
    }

    async run(item) {
        const controller = new AbortController();
        const { signal } = controller;
        this.active.set(item, controller);

        try {
            for (let attempt = 0; ; attempt += 1) {
                item.phase = 'uploading';
                item.loaded = 0;
                this.onChange(item);

                const result = await this.send(item, {
                    signal,
                    onProgress: (loaded) => {
                        item.loaded = loaded;
                        this.onChange(item);
                    },
                });
                if (result.ok) {
                    item.phase = 'uploaded';
                    item.photo = result.photo;
                    break;
                }
                if (!isRetryable(result.status) || attempt >= this.delays.length) {
                    item.phase = 'failed';
                    item.error = result.message;
                    break;
                }

                item.phase = 'retrying';
                this.onChange(item);
                await this.wait(this.delays[attempt], signal);
            }
        } catch (error) {
            if (error instanceof Error && error.name === 'AbortError') {
                item.phase = 'cancelled';
            } else {
                item.phase = 'failed';
                item.error = error instanceof Error ? error.message : String(error);
            }
        } finally {
            this.active.delete(item);
            this.onChange(item);
            this.pump();
            if (this.idle) this.onIdle();
        }
    }
}
