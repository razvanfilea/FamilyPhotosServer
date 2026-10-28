// A toast that survives a page navigation or reload, e.g. "Folder deleted" shown on the Folders tab.

import { toast } from '../core/toast.js';

const KEY = 'fp:flash-toast';

/** Shows `message` as a toast on the next page that calls `showFlashToast`. */
export function flashToast(message, type = 'success') {
    sessionStorage.setItem(KEY, JSON.stringify({ message, type }));
}

export function showFlashToast() {
    const stored = sessionStorage.getItem(KEY);
    if (!stored) return;
    sessionStorage.removeItem(KEY);
    const { message, type } = JSON.parse(stored);
    toast(message, { type });
}
