// The sharing dialog (templates/sharing/dialog.html). Every change is an HTMX request that gets the
// dialog back from the server; this only handles what happens in the browser before one is made: the
// member menus, the remove confirmation and the custom expiry date.

import { register } from '../core/registry.js';

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

register('sharing-dialog', (root) => {
    if (root.dataset.canModify === 'true') {
        updateBadge(Number(root.dataset.memberCount));
    }

    const memberOf = (element) => element.closest('[data-testid="sharing-member"]');

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
            case 'remove':
                button.hidden = true;
                member.querySelector('[data-role="confirm-remove"]').hidden = false;
                member.querySelector('[data-action="confirm-remove"]').focus();
                break;
            case 'cancel-remove':
                member.querySelector('[data-action="remove"]').hidden = false;
                member.querySelector('[data-role="confirm-remove"]').hidden = true;
                break;
        }
    });

    root.addEventListener('change', (event) => {
        const select = /** @type {Element} */ (event.target).closest('[data-role="expiry"]');
        if (!select) return;
        const field = select.closest('[data-role="expiry-field"]');
        field.querySelector('[data-role="custom-expiry"]').hidden = select.value !== 'custom';

        // Presets apply right away for a member; "Custom date" waits for the Set button
        if (memberOf(select) && select.value !== 'custom' && select.value !== 'keep') {
            /** @type {HTMLFormElement} */ (field).requestSubmit();
        }
    });
});
