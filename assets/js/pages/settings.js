// Settings: the password form is HTMX's (`hx-post` to /ui/settings/password); typing only clears
// its error.
import { register } from '../core/registry.js';

register('settings', (root) => {
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
