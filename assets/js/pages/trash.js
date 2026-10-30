// Trash: the photo grid plus the page-level actions "Restore all" and "Empty trash". They run the
// selection bar's actions (counts, partial failures, toasts) on every photo of the trash.
import '../grid/index.js';
import { register } from '../core/registry.js';
import { getJson } from '../core/api.js';
import { EVENTS, emit } from '../core/events.js';
import { plural } from '../core/format.js';
import { toast } from '../core/toast.js';
import { runOnIds } from '../selection/actions.js';

const CONTEXT_KEY = 'trash';

register('trash-actions', (element) => {
    const texts = element.dataset;
    const buttons = [...element.querySelectorAll('button')];

    // The grid reveals its empty state when the last photo is gone
    const empty = document.querySelector('[data-grid-empty]');
    const syncDisabled = () => {
        for (const button of buttons) button.disabled = !empty.hidden;
    };
    const observer = new MutationObserver(syncDisabled);
    observer.observe(empty, { attributes: true, attributeFilter: ['hidden'] });

    /** The photos that were not on the page (and the counts) are only known to the server */
    const refreshGrid = () => emit(document.body, EVENTS.photosAdded, { ids: [], contextKey: CONTEXT_KEY });

    async function restoreAll() {
        const ids = await getJson('/api/context/ids', { query: { ctx: CONTEXT_KEY } });
        if (ids.length === 0) return;
        await runOnIds('restore', ids, { contextKey: CONTEXT_KEY });
        refreshGrid();
    }

    async function emptyTrash() {
        // The photos `/api/delete/{id}` accepts (own + public), same rule as `summarize` in selection/math.js
        const ids = await getJson('/ui/trash/deletable-ids');
        if (ids.length === 0) {
            toast(texts.messageNothingToDelete, { type: 'info' });
            return;
        }
        const title = texts.deleteTitle.replace('{photos}', plural(ids.length, 'photo'));
        await runOnIds('delete-forever', ids, { contextKey: CONTEXT_KEY, title });
        refreshGrid();
    }

    element.addEventListener('click', (event) => {
        const action = /** @type {Element} */ (event.target).closest('button')?.dataset.action;
        if (action === 'restore-all') restoreAll().catch(() => {});
        if (action === 'empty') emptyTrash().catch(() => {});
    });

    return () => observer.disconnect();
});
