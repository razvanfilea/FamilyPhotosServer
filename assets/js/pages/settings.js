// Settings: display preferences are saved as soon as they change. The password form is HTMX's
// (`hx-post` to /ui/settings/password); typing only clears its error.
import { register } from '../core/registry.js';
import { setPref } from '../core/prefs.js';
import { toast } from '../core/toast.js';
import { clearPreviews } from '../core/service-worker.js';

register('settings', (root) => {
    const texts = root.dataset;

    root.addEventListener('change', (event) => {
        const input = /** @type {HTMLInputElement} */ (event.target);
        const key = input.dataset.pref ?? input.closest('[data-pref-group]')?.dataset.prefGroup;
        if (!key) return;
        setPref(key, input.value);
        toast(texts.messageSaved, { type: 'success' });
    });

    // Only where the browser can keep a cache (not on plain HTTP outside localhost)
    if (globalThis.caches) root.querySelector('[data-role="storage"]').hidden = false;

    root.addEventListener('click', async (event) => {
        if (!/** @type {Element} */ (event.target).closest('[data-action="clear-previews"]')) return;
        await clearPreviews();
        toast(texts.messageCleared, { type: 'success' });
    });

    // A refused password change: the cursor goes to the field the error is about
    root.addEventListener('htmx:after:swap', () => {
        const error = root.querySelector('[data-slot="password-error"]');
        const field = error?.dataset.field;
        if (field && !error.hidden) /** @type {HTMLElement|null} */ (root.querySelector(`[name="${field}"]`))?.focus();
    });

    root.addEventListener('input', (event) => {
        const form = /** @type {Element} */ (event.target).closest('[data-slot="password-form"]');
        const error = form?.querySelector('[data-slot="password-error"]');
        if (error) error.hidden = true;
    });
});
