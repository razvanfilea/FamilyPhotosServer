// The sharing dialog (templates/sharing/dialog.html). The dialog is rendered by the server; every
// change goes through the JSON API and then fetches the dialog again, so it always shows what the
// server has.

import { register } from '../core/registry.js';
import { ApiError, api, deleteJson, postJson } from '../core/api.js';
import { toast } from '../core/toast.js';
import { dateExpiry, presetExpiry } from './expiry.js';

const PERMISSIONS = {
    'view': { can_upload: false, can_delete: false },
    'upload': { can_upload: true, can_delete: false },
    'upload-delete': { can_upload: true, can_delete: true },
};

function setIcon(container, name) {
    const use = container.querySelector('use');
    use.setAttribute('href', use.getAttribute('href').replace(/#.*$/, `#${name}`));
}

/** Keeps the folder page's badge in line: "Shared (2)" / "Personal" for the user's own folders */
function updateBadge(memberCount) {
    const label = document.querySelector('[data-bind="folder-badge-label"]');
    const icon = document.querySelector('[data-bind="folder-badge-icon"]');
    if (!label || !icon) return;
    label.textContent = memberCount > 0 ? `Shared (${memberCount})` : 'Personal';
    setIcon(icon, memberCount > 0 ? 'group' : 'person');
}

/**
 * The expiry chosen in an expiry field: `{value}` (unix seconds, null for none), `{keep: true}`
 * to leave it as is, or `{invalid: true}` for a custom date that isn't filled in.
 */
function readExpiry(field) {
    const choice = field.querySelector('[data-role="expiry"]').value;
    if (choice === 'keep') return { keep: true };
    if (choice === 'custom') {
        const value = dateExpiry(field.querySelector('[data-role="expiry-date"]').value);
        return value === null ? { invalid: true } : { value };
    }
    return { value: presetExpiry(choice) };
}

register('sharing-dialog', (root) => {
    const folderId = Number(root.dataset.folderId);
    const messages = root.dataset;
    let busy = false;

    if (root.dataset.canModify === 'true') {
        updateBadge(Number(root.dataset.memberCount));
    }

    /**
     * Runs a change, shows its toast and re-renders the dialog (keeping a member's menu open)
     * @param {*} request
     * @param {string} message
     * @param {{keepMenuOf?: any}} [options]
     */
    async function change(request, message, { keepMenuOf } = {}) {
        if (busy) return;
        busy = true;
        try {
            try {
                await request();
            } catch (error) {
                // The API wrapper already showed the server's message
                if (error instanceof ApiError) return;
                throw error;
            }
            toast(message, { type: 'success' });
            await window.htmx.ajax('GET', `/ui/dialogs/sharing/${folderId}`, {
                target: '#app-dialog-content',
                swap: 'innerHTML',
            });
        } finally {
            // The fragment replaces this dialog when it loads; if it fails, keep this one usable
            busy = false;
        }
        if (keepMenuOf) {
            document
                .querySelector(`[data-share-id="${keepMenuOf}"] [data-action="toggle-menu"]`)
                ?.click();
        }
    }

    const memberOf = (element) => element.closest('[data-testid="sharing-member"]');
    const flagsOf = (member) => ({
        can_upload: member.dataset.canUpload === 'true',
        can_delete: member.dataset.canDelete === 'true',
    });

    function updateMember(member, extra, message) {
        const shareId = member.dataset.shareId;
        return change(
            () => api(`/api/sharing/${shareId}`, { method: 'PUT', json: { ...flagsOf(member), ...extra } }),
            message,
            { keepMenuOf: shareId },
        );
    }

    root.addEventListener('click', (event) => {
        const button = /** @type {Element} */ (event.target).closest('[data-action]');
        if (!button || !root.contains(button)) return;
        const member = memberOf(button);

        switch (button.dataset.action) {
            case 'toggle-menu': {
                const menu = member.querySelector('[data-role="menu"]');
                menu.hidden = !menu.hidden;
                button.setAttribute('aria-expanded', String(!menu.hidden));
                break;
            }
            case 'permission':
                updateMember(member, PERMISSIONS[button.dataset.permission], messages.messageUpdated);
                break;
            case 'set-expiry': {
                const expiry = readExpiry(member.querySelector('[data-role="expiry-field"]'));
                if (expiry.invalid) {
                    toast(messages.messagePickDate, { type: 'error' });
                    break;
                }
                updateMember(member, { expires_at: expiry.value }, messages.messageUpdated);
                break;
            }
            case 'remove':
                button.hidden = true;
                member.querySelector('[data-role="confirm-remove"]').hidden = false;
                member.querySelector('[data-action="confirm-remove"]').focus();
                break;
            case 'cancel-remove':
                member.querySelector('[data-action="remove"]').hidden = false;
                member.querySelector('[data-role="confirm-remove"]').hidden = true;
                break;
            case 'confirm-remove':
                change(() => deleteJson(`/api/sharing/${member.dataset.shareId}`), messages.messageRemoved);
                break;
            case 'add': {
                const expiry = readExpiry(root.querySelector('.fp-sharing__add [data-role="expiry-field"]'));
                if (expiry.invalid) {
                    toast(messages.messagePickDate, { type: 'error' });
                    break;
                }
                change(
                    () => postJson('/api/sharing', {
                        folder_id: folderId,
                        grantee_id: button.dataset.userId,
                        can_upload: false,
                        can_delete: false,
                        expires_at: expiry.value,
                    }),
                    messages.messageAdded,
                );
                break;
            }
        }
    });

    root.addEventListener('change', (event) => {
        const select = /** @type {Element} */ (event.target).closest('[data-role="expiry"]');
        if (!select) return;
        const field = select.closest('[data-role="expiry-field"]');
        field.querySelector('[data-role="custom-expiry"]').hidden = select.value !== 'custom';

        // Presets apply right away for a member; "Custom date" waits for the Set button
        const member = memberOf(select);
        if (member && select.value !== 'custom' && select.value !== 'keep') {
            const expiry = readExpiry(field);
            updateMember(member, { expires_at: expiry.value }, messages.messageUpdated);
        }
    });
});
