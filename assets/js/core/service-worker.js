// Registers the service worker (assets/sw.js, served as /sw.js), offers its updates and keeps its
// preview cache in line with what the user removes.

import { EVENTS } from './events.js';
import { t } from './strings.js';
import { toast } from './toast.js';

// A toast that stays until it's used (setTimeout's longest delay)
const UNTIL_USED = 2 ** 31 - 1;

/**
 * Tells the page when a new worker has installed and is waiting for the page to let it take over, and
 * reloads once it has.
 * @param {{waiting: ServiceWorker | null, addEventListener: Function}} registration
 * @param {{controller: ServiceWorker | null, addEventListener: Function}} container
 * @param {{offer: (accept: () => void) => void, reload: () => void}} handlers
 * @returns {() => void} offers the update again (a navigation swaps the toast away)
 */
export function watchForUpdate(registration, container, { offer, reload }) {
    // No controller yet: the first worker taking the page over is not an update
    const hadController = container.controller != null;
    const accept = () => registration.waiting?.postMessage({ type: 'skip-waiting' });
    const offerIfWaiting = () => {
        if (registration.waiting && container.controller) offer(accept);
    };

    registration.addEventListener('updatefound', () => {
        const worker = /** @type {any} */ (registration).installing;
        worker?.addEventListener('statechange', () => {
            if (worker.state === 'installed') offerIfWaiting();
        });
    });
    let reloading = false;
    container.addEventListener('controllerchange', () => {
        if (!hadController || reloading) return;
        reloading = true;
        reload();
    });
    offerIfWaiting();
    return offerIfWaiting;
}

// The cache the worker stores previews in (assets/sw.js)
const PREVIEW_CACHE = 'fp-previews';

/** @param {unknown} message */
function post(message) {
    navigator.serviceWorker?.controller?.postMessage(message);
}

/** Drops the stored previews of photos that are gone (or whose id may be reused). */
function forgetPreviews(/** @type {number[]} */ ids) {
    post({ type: 'forget-previews', ids });
}

/** Drops every stored preview, e.g. those of the user who is signing out. */
export async function clearPreviews() {
    await globalThis.caches?.delete(PREVIEW_CACHE);
}

export function initServiceWorker() {
    // Absent on plain HTTP outside localhost
    if (!('serviceWorker' in navigator)) return;

    document.addEventListener(EVENTS.photosRemoved, (event) => {
        forgetPreviews(/** @type {CustomEvent} */ (event).detail.ids);
    });

    navigator.serviceWorker
        .register('/sw.js')
        .then((registration) => {
            let dismiss = () => {};
            const offerAgain = watchForUpdate(registration, navigator.serviceWorker, {
                offer: (accept) => {
                    dismiss();
                    dismiss = toast(t('updateAvailable'), {
                        timeout: UNTIL_USED,
                        action: { label: t('reload'), onClick: accept },
                    });
                },
                reload: () => location.reload(),
            });
            // Navigating in place replaces the page's toasts
            document.addEventListener('htmx:after:swap', offerAgain);
        })
        // Blocked (a private window, a browser setting): the site works without it
        .catch(() => {});
}
