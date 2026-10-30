// Settings: the password form posts to /api/profile/password.
import { register } from '../core/registry.js';
import { postJson } from '../core/api.js';
import { toast } from '../core/toast.js';

register('settings', (root) => {
    const texts = root.dataset;

    const form = root.querySelector('[data-slot="password-form"]');
    const error = form.querySelector('[data-slot="password-error"]');
    const submit = form.querySelector('button[type="submit"]');

    const showError = (message) => {
        error.textContent = message;
        error.hidden = !message;
    };

    form.addEventListener('input', () => showError(''));
    form.addEventListener('submit', async (event) => {
        event.preventDefault();
        const { current_password: current, new_password: next, confirm_password: confirmation } = form.elements;

        if ([...next.value].length < 8) {
            showError(texts.errorPasswordShort);
            return;
        }
        if (next.value !== confirmation.value) {
            showError(texts.errorPasswordMismatch);
            return;
        }

        submit.disabled = true;
        try {
            await postJson('/api/profile/password', {
                current_password: current.value,
                new_password: next.value,
            }, { silent: true });
            form.reset();
            toast(texts.messagePasswordChanged, { type: 'success' });
        } catch (failure) {
            showError(failure.message);
        } finally {
            submit.disabled = false;
        }
    });
});
