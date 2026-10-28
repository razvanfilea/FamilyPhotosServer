// The viewer's info panel: a side panel from 1024 px, a bottom sheet below. It stays open while
// navigating and always shows the current photo: each load aborts the previous one, and a
// response for another photo is never shown.

import { api } from '../core/api.js';

/**
 * @param {HTMLElement} panel the `<aside>`
 * @param {Record<string, string>} strings
 * @param {(open: boolean) => void} onToggle
 */
export function createInfoPanel(panel, strings, onToggle) {
    const body = panel.querySelector('[data-slot="info"]');
    let photoId = null;
    let request = null;

    async function load() {
        request?.abort();
        if (panel.hidden || photoId === null || body.dataset.photoId === String(photoId)) return;

        const id = photoId;
        const controller = new AbortController();
        request = controller;
        body.setAttribute('aria-busy', 'true');
        try {
            const html = await api(`/ui/viewer/info/${id}`, { signal: controller.signal, silent: true });
            if (controller.signal.aborted || id !== photoId) return;
            body.innerHTML = html ?? '';
            body.dataset.photoId = String(id);
        } catch (error) {
            if (error.name === 'AbortError' || id !== photoId) return;
            const message = document.createElement('p');
            message.className = 'viewer-info__error';
            message.textContent = strings.infoError;
            body.replaceChildren(message);
        } finally {
            if (request === controller) {
                request = null;
                body.removeAttribute('aria-busy');
            }
        }
    }

    function setOpen(open) {
        if (panel.hidden === !open) return;
        panel.hidden = !open;
        onToggle(open);
        load();
    }

    return {
        isOpen: () => !panel.hidden,
        toggle: () => setOpen(panel.hidden),
        close: () => setOpen(false),
        /** Shows the details of another photo (loaded now if the panel is open, else when it opens) */
        show(id) {
            photoId = id;
            if (body.dataset.photoId !== String(id)) {
                body.replaceChildren();
                delete body.dataset.photoId;
            }
            load();
        },
        clear() {
            request?.abort();
            photoId = null;
            body.replaceChildren();
            delete body.dataset.photoId;
        },
    };
}
