// A toast that survives a page navigation or reload, e.g. "Folder deleted" shown on the Folders tab.

import { toast } from './toast.js';

const KEY = 'fp:flash-toast';

/** Shows `message` as a toast on the next page that calls `showFlashToast`. */
function flashToast(message, type = 'success') {
    sessionStorage.setItem(KEY, JSON.stringify({ message, type }));
}

export function showFlashToast() {
    const stored = sessionStorage.getItem(KEY);
    if (!stored) return;
    sessionStorage.removeItem(KEY);
    const { message, type } = JSON.parse(stored);
    toast(message, { type });
}

/** The server asks for a flash toast with `HX-Trigger: {"fp:flash-toast": {…}}` on responses that navigate. */
export function initFlashToasts() {
    document.body.addEventListener('fp:flash-toast', (event) => {
        flashToast(event.detail.message, event.detail.type);
    });
}
