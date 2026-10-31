// The app dialog: one native <dialog id="app-dialog"> (focus trap and Esc for free), filled with
// HTML from a string, a Node, a <template>, or an HTMX fragment. HTMX elements can load a fragment
// into it with `hx-get="…" hx-target="#app-dialog-content" hx-swap="innerHTML"`; it opens once the
// content is swapped in. Buttons with `data-dialog-close` close it.

import { mount } from './registry.js';
import { popScope, pushScope } from './keyboard.js';

const SCOPE = 'dialog';

const dialogElement = () => /** @type {HTMLDialogElement} */ (document.getElementById('app-dialog'));
const contentElement = () => document.getElementById('app-dialog-content');

let onCloseCallback;
/** While a confirm action runs, the dialog can't be dismissed (Esc, backdrop) */
let busy = false;

function setBusy(value) {
    busy = value;
    dialogElement().querySelector('.modal-backdrop button').disabled = value;
}

function clearContent() {
    contentElement().replaceChildren();
}

function show() {
    const dialog = dialogElement();
    if (!dialog.open) {
        pushScope(SCOPE, { modal: true });
        dialog.showModal();
    }
    mount(contentElement());
}

function setContent(content) {
    clearContent();
    const target = contentElement();
    if (typeof content === 'string') {
        target.innerHTML = content;
    } else if (content instanceof HTMLTemplateElement) {
        target.append(content.content.cloneNode(true));
    } else {
        target.append(content);
    }
    window.htmx?.process(target);
}

/**
 * @param {{html?: string, node?: Node, template?: HTMLTemplateElement, url?: string, onClose?: (returnValue: string) => void}} options
 *   `html` must be trusted markup: escape user data with `escapeHtml` or pass a Node.
 * @returns {Promise<void>} resolves once the dialog is open
 */
export async function openDialog({ html, node, template, url, onClose } = {}) {
    // Replacing the content of an open dialog counts as closing the previous one
    const previousCallback = onCloseCallback;
    onCloseCallback = onClose;
    previousCallback?.('');
    if (url) {
        // Errors are reported by the global HTMX handlers; the dialog opens on htmx:after:swap
        await window.htmx.ajax('GET', url, { target: '#app-dialog-content', swap: 'innerHTML' });
        return;
    }
    setContent(html ?? node ?? template);
    show();
}

export function closeDialog(returnValue = '') {
    const dialog = dialogElement();
    if (dialog.open) dialog.close(returnValue);
}

export function isDialogOpen() {
    return dialogElement().open;
}

/**
 * Asks for confirmation. `onConfirm` runs while the dialog shows a busy confirm button; if it
 * throws, the dialog stays open so the user can retry or cancel.
 * @param {{title: string, body?: string|Node, confirmLabel?: string, cancelLabel?: string, danger?: boolean, onConfirm?: () => Promise<void>|void}} options
 *   A string `body` is trusted HTML.
 * @returns {Promise<boolean>} true once confirmed (and `onConfirm` succeeded), false if dismissed
 */
export function confirm({ title, body = '', confirmLabel = 'OK', cancelLabel, danger = false, onConfirm }) {
    const fragment = /** @type {DocumentFragment} */ (
        /** @type {HTMLTemplateElement} */ (document.getElementById('confirm-dialog-template')).content.cloneNode(true)
    );
    fragment.querySelector('[data-slot="title"]').textContent = title;
    const bodySlot = fragment.querySelector('[data-slot="body"]');
    if (typeof body === 'string') {
        bodySlot.innerHTML = body;
    } else {
        bodySlot.append(body);
    }
    if (!body) bodySlot.remove();

    const confirmButton = fragment.querySelector('[data-slot="confirm"]');
    const cancelButton = fragment.querySelector('[data-slot="cancel"]');
    confirmButton.textContent = confirmLabel;
    if (cancelLabel) cancelButton.textContent = cancelLabel;
    if (danger) confirmButton.classList.replace('btn-primary', 'btn-error');

    return new Promise((resolve) => {
        let confirmed = false;

        cancelButton.addEventListener('click', () => closeDialog());
        confirmButton.addEventListener('click', async () => {
            confirmButton.disabled = true;
            cancelButton.disabled = true;
            confirmButton.classList.add('btn-disabled');
            const spinner = document.createElement('span');
            spinner.className = 'loading loading-spinner loading-sm';
            confirmButton.prepend(spinner);
            setBusy(true);
            try {
                await onConfirm?.();
                confirmed = true;
                setBusy(false);
                closeDialog('confirm');
            } catch {
                setBusy(false);
                spinner.remove();
                confirmButton.disabled = false;
                cancelButton.disabled = false;
                confirmButton.classList.remove('btn-disabled');
            }
        });

        openDialog({ node: fragment, onClose: () => resolve(confirmed) });
        (danger ? cancelButton : confirmButton).focus();
    });
}

/** Escapes text for use inside HTML strings given to `openDialog` / `confirm`. */
export function escapeHtml(text) {
    const div = document.createElement('div');
    div.textContent = text;
    return div.innerHTML;
}

export function initDialog() {
    const dialog = dialogElement();

    dialog.addEventListener('cancel', (event) => {
        if (busy) event.preventDefault();
    });

    dialog.addEventListener('close', () => {
        popScope(SCOPE);
        const callback = onCloseCallback;
        onCloseCallback = undefined;
        clearContent();
        callback?.(dialog.returnValue);
    });

    dialog.addEventListener('click', (event) => {
        if (!busy && /** @type {Element} */ (event.target).closest('[data-dialog-close]')) closeDialog();
    });

    document.body.addEventListener('htmx:after:swap', (event) => {
        const { ctx } = event.detail;
        // Error responses aren't swapped (and 204 has nothing to show)
        if (ctx.target === contentElement() && ctx.response.status < 300 && ctx.response.status !== 204) show();
    });
}
