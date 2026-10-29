// Which files the upload page accepts, and collecting them from inputs and drops (folders too).

/** Video extensions, as in `src/utils/media.rs` */
const VIDEO_EXTENSIONS = new Set([
    '3gp', '3g2', 'mp4', 'm4v', 'mov', 'mkv', 'webm', 'avi', 'mpg', 'mpeg', 'ts', 'mts', 'm2ts', 'flv',
    'wmv', 'ogv',
]);

/** Photos browsers often don't give a MIME type for (HEIC, camera RAW) */
const IMAGE_EXTENSIONS = new Set([
    'jpg', 'jpeg', 'png', 'gif', 'webp', 'avif', 'bmp', 'tif', 'tiff', 'heic', 'heif', 'dng', 'cr2',
    'cr3', 'nef', 'arw', 'orf', 'rw2', 'raf',
]);

/** Images the browser can show as a thumbnail */
const PREVIEWABLE_TYPES = new Set([
    'image/jpeg', 'image/png', 'image/gif', 'image/webp', 'image/avif', 'image/bmp',
]);

function extension(name) {
    const dot = name.lastIndexOf('.');
    return dot > 0 ? name.slice(dot + 1).toLowerCase() : '';
}

/** @param {{name: string, type: string}} file */
export function isVideo(file) {
    return file.type.startsWith('video/') || VIDEO_EXTENSIONS.has(extension(file.name));
}

/**
 * Photos and videos, except hidden files (like macOS's `._IMG_1234.JPG` metadata files).
 * @param {{name: string, type: string}} file
 */
export function isMedia(file) {
    if (file.name.startsWith('.')) return false;
    return file.type.startsWith('image/') || IMAGE_EXTENSIONS.has(extension(file.name)) || isVideo(file);
}

/** @param {{type: string}} file */
export function isPreviewable(file) {
    return PREVIEWABLE_TYPES.has(file.type);
}

/** Identifies a file the user already added (picking the same file twice adds it once). */
export function fileKey(file) {
    return `${file.webkitRelativePath || file.name}:${file.size}:${file.lastModified}`;
}

/**
 * Splits picked files into those to upload and those to skip.
 * @param {Iterable<File>} files
 * @param {{maxSize: number, known: Set<string>}} options `known` = the `fileKey`s already queued
 * @returns {{accepted: File[], unsupported: number, tooLarge: number, duplicates: number}}
 */
export function sortFiles(files, { maxSize, known }) {
    const result = { accepted: [], unsupported: 0, tooLarge: 0, duplicates: 0 };
    const seen = new Set(known);
    for (const file of files) {
        const key = fileKey(file);
        if (!isMedia(file)) {
            result.unsupported += 1;
        } else if (file.size > maxSize) {
            result.tooLarge += 1;
        } else if (seen.has(key)) {
            result.duplicates += 1;
        } else {
            seen.add(key);
            result.accepted.push(file);
        }
    }
    return result;
}

function readEntries(reader) {
    return new Promise((resolve, reject) => reader.readEntries(resolve, reject));
}

function entryFile(entry) {
    return new Promise((resolve, reject) => entry.file(resolve, reject));
}

/** The files under a dropped file or directory entry */
async function entryFiles(entry) {
    if (entry.isFile) {
        return [await entryFile(entry)];
    }
    if (!entry.isDirectory) return [];

    const reader = entry.createReader();
    const files = [];
    // readEntries returns the children in batches, and an empty batch at the end
    for (let batch = await readEntries(reader); batch.length > 0; batch = await readEntries(reader)) {
        for (const child of batch) {
            files.push(...await entryFiles(child));
        }
    }
    return files;
}

/**
 * The files of a drop, including everything inside dropped folders.
 * @param {DataTransfer} dataTransfer
 * @returns {Promise<File[]>}
 */
export async function droppedFiles(dataTransfer) {
    // The entries must be taken synchronously, before the drop event ends
    const entries = [...dataTransfer.items]
        .filter((item) => item.kind === 'file')
        .map((item) => item.webkitGetAsEntry?.());
    if (entries.length === 0 || entries.some((entry) => !entry)) {
        return [...dataTransfer.files];
    }

    const files = [];
    for (const entry of entries) {
        try {
            files.push(...await entryFiles(entry));
        } catch {
            // An unreadable entry (e.g. removed meanwhile) is skipped
        }
    }
    return files;
}
