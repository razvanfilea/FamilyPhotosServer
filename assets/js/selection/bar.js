// The selection bar (component `selection-bar`, templates/selection/bar.html): shows the number of
// selected photos and the actions of the grid's context, and runs them. It listens to
// `fp:selection-change`, which carries the grid element (`event.target`) with its `fpSelection`.

import { register } from '../core/registry.js';
import { EVENTS, on } from '../core/events.js';
import { toast } from '../core/toast.js';
import { runAction } from './actions.js';
import { register as registerShortcut } from '../core/keyboard.js';
import { s } from './strings.js';

/** Actions per grid context; everything that isn't the trash shows the first row */
const ACTIONS = {
    default: ['favorite', 'download', 'share', 'move', 'delete'],
    trash: ['restore', 'delete-forever'],
};

/** Whether the browser can share files (the Share action is hidden otherwise) */
function canShareFiles() {
    return typeof navigator.canShare === 'function'
        && navigator.canShare({ files: [new File([''], 'photo.jpg', { type: 'image/jpeg' })] });
}

register('selection-bar', (bar) => {
    const abort = new AbortController();
    const userId = bar.dataset.userId;
    const countElement = bar.querySelector('[data-slot="count"]');
    const spinner = bar.querySelector('[data-slot="spinner"]');
    const buttons = Object.fromEntries(
        [...bar.querySelectorAll('.selection-bar__action')].map((button) => [button.dataset.action, button]),
    );
    const shareable = canShareFiles();
    /** The grid element whose selection the bar shows */
    let grid = null;
    let running = false;

    function setLabel(button, text) {
        button.title = text;
        button.setAttribute('aria-label', text);
        button.querySelector('[data-slot="label"]').textContent = text;
    }

    /** A disabled action explains itself (tooltip, and a toast when pressed: touch has no tooltips) */
    function setDisabled(button, reason) {
        button.setAttribute('aria-disabled', String(Boolean(reason)));
        button.dataset.reason = reason ?? '';
        if (reason) button.title = reason;
    }

    function update({ count, resolving, contextKey }, source) {
        bar.dataset.visible = String(count > 0);
        bar.inert = count === 0;
        if (count === 0) return;
        if (source?.fpSelection) grid = source;

        if (countElement.textContent !== String(count)) {
            countElement.textContent = String(count);
            countElement.classList.remove('is-changing');
            void countElement.offsetWidth;
            countElement.classList.add('is-changing');
        }
        spinner.hidden = !resolving;

        const visible = ACTIONS[contextKey === 'trash' ? 'trash' : 'default'];
        const summary = grid.fpSelection.summary(userId);
        for (const [name, button] of Object.entries(buttons)) {
            button.hidden = !visible.includes(name) || (name === 'share' && !shareable);
        }
        setLabel(buttons.favorite, s(contextKey === 'favorites' ? 'unfavorite' : 'favorite'));
        // Ids still being resolved aren't complete: wait for them
        const waiting = resolving ? s('resolving') : null;
        const undeletable = (message) => waiting ?? (summary.undeletable > 0 ? s(message) : null);
        for (const name of ['favorite', 'download', 'share']) setDisabled(buttons[name], waiting);
        setDisabled(buttons.move, undeletable('cantMove'));
        setDisabled(buttons.delete, undeletable('cantDelete'));
        setDisabled(buttons.restore, undeletable('cantRestore'));
        setDisabled(
            buttons['delete-forever'],
            waiting ?? (summary.undeletable > 0 || summary.foreign > 0 ? s('cantDeleteForever') : null),
        );
    }

    async function trigger(name) {
        const button = buttons[name];
        if (running || !grid?.fpSelection?.count() || !button || button.hidden) return;
        if (button.getAttribute('aria-disabled') === 'true') {
            toast(button.dataset.reason, { type: 'info' });
            return;
        }
        running = true;
        try {
            await runAction(name, grid.fpSelection, { userId });
        } finally {
            running = false;
        }
    }

    bar.addEventListener('click', (event) => {
        const button = /** @type {Element} */ (event.target).closest('button');
        if (!button) return;
        if (button.dataset.action === 'clear') grid?.fpSelection?.clear();
        else if (buttons[button.dataset.action]) trigger(button.dataset.action);
    }, { signal: abort.signal });

    on(document, EVENTS.selectionChange, (event) => update(event.detail, event.target), { signal: abort.signal });

    const hasSelection = () => grid?.fpSelection?.count() > 0;
    const isTrash = () => grid?.fpSelection?.contextKey === 'trash';
    const unregisterKeys = [
        registerShortcut('grid', 'Delete', () => hasSelection() ? trigger(isTrash() ? 'delete-forever' : 'delete') : false),
        registerShortcut('grid', 'm', () => hasSelection() && !buttons.move.hidden ? trigger('move') : false),
    ];

    return () => {
        abort.abort();
        for (const unregister of unregisterKeys) unregister();
    };
});
