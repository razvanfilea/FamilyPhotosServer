// Writes an EXIF DateTimeOriginal into JPEG files, because ImageMagick can't write EXIF tags.
// Usage as a script: node playwright/harness/exif.mjs <file.jpg> "YYYY:MM:DD HH:MM:SS"

import { readFileSync, writeFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';

/** A minimal little-endian EXIF APP1 segment: IFD0 → Exif IFD → DateTimeOriginal. */
function exifSegment(dateTime) {
    if (!/^\d{4}:\d{2}:\d{2} \d{2}:\d{2}:\d{2}$/.test(dateTime)) {
        throw new Error(`Invalid EXIF date: ${dateTime}`);
    }

    const tiff = Buffer.alloc(64);
    tiff.write('II', 0, 'ascii');
    tiff.writeUInt16LE(42, 2);
    tiff.writeUInt32LE(8, 4); // IFD0 offset
    // IFD0: one entry, ExifIFDPointer (LONG) → 26
    tiff.writeUInt16LE(1, 8);
    tiff.writeUInt16LE(0x8769, 10);
    tiff.writeUInt16LE(4, 12);
    tiff.writeUInt32LE(1, 14);
    tiff.writeUInt32LE(26, 18);
    tiff.writeUInt32LE(0, 22);
    // Exif IFD: one entry, DateTimeOriginal (ASCII, 20 bytes) → 44
    tiff.writeUInt16LE(1, 26);
    tiff.writeUInt16LE(0x9003, 28);
    tiff.writeUInt16LE(2, 30);
    tiff.writeUInt32LE(20, 32);
    tiff.writeUInt32LE(44, 36);
    tiff.writeUInt32LE(0, 40);
    tiff.write(`${dateTime}\0`, 44, 'ascii');

    const header = Buffer.from('Exif\0\0', 'ascii');
    const length = Buffer.alloc(2);
    length.writeUInt16BE(2 + header.length + tiff.length);
    return Buffer.concat([Buffer.from([0xff, 0xe1]), length, header, tiff]);
}

/** Returns the JPEG bytes with an EXIF segment inserted right after the SOI marker. */
export function withExifDate(jpeg, dateTime) {
    if (jpeg[0] !== 0xff || jpeg[1] !== 0xd8) {
        throw new Error('Not a JPEG file');
    }
    return Buffer.concat([jpeg.subarray(0, 2), exifSegment(dateTime), jpeg.subarray(2)]);
}

/** Formats a Date as an EXIF date in UTC ("wall clock", like the server stores it). */
export function exifDate(date) {
    return date.toISOString().slice(0, 19).replace('T', ' ').replace(/-/g, ':');
}

export function setExifDate(file, dateTime) {
    writeFileSync(file, withExifDate(readFileSync(file), dateTime));
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
    const [file, dateTime] = process.argv.slice(2);
    setExifDate(file, dateTime);
}
