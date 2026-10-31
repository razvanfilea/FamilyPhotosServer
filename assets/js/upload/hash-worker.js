// Module worker computing the BLAKE3 hash of files, so hashing hundreds of photos doesn't block
// the page. Messages in: {id, file}; out: {id, progress: bytesRead} (for large files), then
// {id, hash: '<64 hex>'} or {id, error}. It hashes one file at a time (hasher.js sends one per
// worker).

// The UMD build of hash-wasm defines `globalThis.hashwasm` when loaded as a module
import '../../vendor/hash-wasm/blake3.umd.min.js';

const PROGRESS_STEP = 16 * 1024 * 1024;

let blake3;

self.addEventListener('message', async (event) => {
    const { id, file } = event.data;
    try {
        blake3 ??= await globalThis.hashwasm.createBLAKE3();
        blake3.init();

        const reader = file.stream().getReader();
        let read = 0;
        let reported = 0;
        for (let chunk = await reader.read(); !chunk.done; chunk = await reader.read()) {
            blake3.update(chunk.value);
            read += chunk.value.length;
            if (read - reported >= PROGRESS_STEP) {
                reported = read;
                self.postMessage({ id, progress: read });
            }
        }
        self.postMessage({ id, hash: blake3.digest('hex') });
    } catch (error) {
        self.postMessage({ id, error: String(error instanceof Error ? error.message : error) });
    }
});
