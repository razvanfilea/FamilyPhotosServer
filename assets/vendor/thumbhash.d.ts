// Types for the vendored thumbhash.js (only for `npm run typecheck`).
export function thumbHashToApproximateAspectRatio(hash: Uint8Array): number;
export function thumbHashToRGBA(hash: Uint8Array): { w: number; h: number; rgba: Uint8Array | number[] };
export function rgbaToDataURL(w: number, h: number, rgba: Uint8Array | number[]): string;
export function thumbHashToDataURL(hash: Uint8Array): string;
