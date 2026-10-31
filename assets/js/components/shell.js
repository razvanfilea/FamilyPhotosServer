// Components of the navigation shell and the login page.

import { register } from '../core/registry.js';
import { confirm } from '../core/dialog.js';
import { api } from '../core/api.js';

/** <details> menus: close on outside click, Escape, and after choosing an item. */
register('dropdown', (/** @type {HTMLDetailsElement} */ details) => {
    const close = () => details.removeAttribute('open');
    const onDocumentClick = (event) => {
        if (!details.contains(event.target)) close();
    };
    const onKeyDown = (event) => {
        if (event.key === 'Escape' && details.open) {
            close();
            details.querySelector('summary').focus();
        }
    };
    const onItemClick = (event) => {
        if (/** @type {Element} */ (event.target).closest('li a, li button')) close();
    };

    document.addEventListener('click', onDocumentClick);
    details.addEventListener('keydown', onKeyDown);
    details.addEventListener('click', onItemClick);
    return () => {
        document.removeEventListener('click', onDocumentClick);
    };
});

async function signOut() {
    await api('/api/logout', { method: 'POST' });
    location.assign('/login');
}

register('sign-out', (button) => {
    const template = /** @type {HTMLTemplateElement} */ (document.getElementById('sign-out-dialog'));
    button.addEventListener('click', () => {
        confirm({
            title: template.content.querySelector('h2').textContent,
            body: template.content.querySelector('p').textContent,
            confirmLabel: button.textContent.trim(),
            onConfirm: signOut,
        });
    });
});

register('password-toggle', (button) => {
    const input = button.closest('.input').querySelector('input');
    button.addEventListener('click', () => {
        const visible = input.type === 'password';
        input.type = visible ? 'text' : 'password';
        button.setAttribute('aria-pressed', String(visible));
        button.setAttribute('aria-label', visible ? button.dataset.labelHide : button.dataset.labelShow);
    });
});

register('login-form', (/** @type {HTMLFormElement} */ form) => {
    const username = /** @type {HTMLInputElement} */ (form.elements.namedItem('user_id'));
    const password = /** @type {HTMLInputElement} */ (form.elements.namedItem('password'));
    const submit = form.querySelector('button[type="submit"]');

    const updateSubmit = () => {
        submit.disabled = username.value.trim() === '' || password.value === '';
    };
    form.addEventListener('input', updateSubmit);
    updateSubmit();

    // Like the Android app, user names are trimmed and lowercased before logging in
    form.addEventListener('htmx:config:request', (event) => {
        event.detail.ctx.request.body.set('user_id', username.value.trim().toLowerCase());
    });

    form.addEventListener('htmx:error', (event) => {
        if (event.detail.ctx?.response || event.detail.error?.name === 'AbortError') return;
        const error = form.querySelector('[data-slot="network-error"]').content.cloneNode(true);
        document.getElementById('login-error').replaceWith(error);
    });
});

/**
 * Tapping the tab that is already open scrolls to the top (captured, so the link doesn't reload the page).
 * A second tap on Photos, once at the top, brings the "On this day" row back to its first card.
 */
register('nav-top', (nav) => {
    nav.addEventListener('click', (event) => {
        const link = /** @type {Element} */ (event.target).closest('a[aria-current="page"]');
        if (!link || event.ctrlKey || event.metaKey || event.shiftKey) return;
        event.preventDefault();
        event.stopPropagation();
        const behavior = matchMedia('(prefers-reduced-motion: reduce)').matches ? 'auto' : 'smooth';
        const memories = link.getAttribute('href') === '/' && scrollY < 2 ? document.querySelector('.memories') : null;
        if (memories) memories.scrollTo({ left: 0, behavior });
        else scrollTo({ top: 0, behavior });
    }, true);
});
