// Toasts (Android snackbars): typed, bottom-centre, auto-dismissed, with an optional action.
// The server can trigger one with `HX-Trigger: {"fp:toast": {"message": "…", "type": "success"}}`.

import { EVENTS } from './events.js';

const MAX_VISIBLE = 3;
const TIMEOUTS = { success: 4000, info: 4000, error: 6000 };

/**
 * @param {string} message
 * @param {{type?: 'success'|'error'|'info', action?: {label: string, onClick: () => void}, timeout?: number}} options
 * @returns {() => void} a function dismissing the toast
 */
export function toast(message, { type = 'info', action, timeout } = {}) {
    const region = document.getElementById('toasts');
    const template = document.getElementById('toast-template');
    const element = template.content.firstElementChild.cloneNode(true);

    element.classList.add(`fp-toast--${type}`);
    element.dataset.type = type;
    element.setAttribute('role', type === 'error' ? 'alert' : 'status');
    element.querySelector('[data-slot="message"]').textContent = message;

    let timer;
    const dismiss = () => {
        clearTimeout(timer);
        element.remove();
    };

    if (action) {
        const button = element.querySelector('[data-slot="action"]');
        button.textContent = action.label;
        button.hidden = false;
        button.addEventListener('click', () => {
            dismiss();
            action.onClick();
        });
    }
    element.querySelector('[data-slot="close"]').addEventListener('click', dismiss);

    // Keep the toast while it's hovered or focused, restart the timer afterwards
    const duration = timeout ?? TIMEOUTS[type] ?? TIMEOUTS.info;
    const start = () => {
        clearTimeout(timer);
        timer = setTimeout(dismiss, duration);
    };
    element.addEventListener('pointerenter', () => clearTimeout(timer));
    element.addEventListener('pointerleave', start);
    element.addEventListener('focusin', () => clearTimeout(timer));
    element.addEventListener('focusout', start);

    region.append(element);
    while (region.children.length > MAX_VISIBLE) {
        region.firstElementChild.remove();
    }
    start();

    return dismiss;
}

/** Shows toasts requested through the `fp:toast` event (e.g. from `HX-Trigger` response headers). */
export function initToasts() {
    document.body.addEventListener(EVENTS.toast, (event) => {
        const { message, type } = event.detail ?? {};
        if (message) toast(message, { type });
    });
}
