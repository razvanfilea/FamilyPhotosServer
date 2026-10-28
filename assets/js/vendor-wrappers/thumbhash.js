// ES module wrapper around the vendored ThumbHash decoder, taking base64 hashes.

import { thumbHashToDataURL as hashToDataURL, thumbHashToRGBA } from '../../vendor/thumbhash.js';

/** @returns {Uint8Array} */
export function base64ToBytes(base64) {
    const binary = atob(base64);
    const bytes = new Uint8Array(binary.length);
    for (let i = 0; i < binary.length; i++) {
        bytes[i] = binary.charCodeAt(i);
    }
    return bytes;
}

/** A PNG data URL of the placeholder for a base64 thumbhash. */
export function thumbHashToDataURL(base64) {
    return hashToDataURL(base64ToBytes(base64));
}

/** Draws the placeholder for a base64 thumbhash onto a canvas (sized to the hash's resolution). */
export function drawThumbHash(canvas, base64) {
    const { w, h, rgba } = thumbHashToRGBA(base64ToBytes(base64));
    canvas.width = w;
    canvas.height = h;
    canvas.getContext('2d').putImageData(new ImageData(new Uint8ClampedArray(rgba), w, h), 0, 0);
}
