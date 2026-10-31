// Entry point loaded by every page: boots the core modules, the global HTMX error handling and the
// shell components. Page-specific modules are loaded by their templates.

import { boot } from './core/registry.js';
import { initToasts, toast } from './core/toast.js';
import { initDialog, isDialogOpen, openDialog } from './core/dialog.js';
import { register as registerShortcut } from './core/keyboard.js';
import { errorMessage, redirectToLogin } from './core/api.js';
import { t } from './core/strings.js';
import { initFlashToasts } from './core/flash.js';
import './components/shell.js';
// The destination picker (and the folder browser and name dialog it uses) is opened from dialogs on many pages
import './destination-picker/picker.js';

function initHtmxErrors() {
    // Forms that report their own failures (the login form)
    const silent = (element) => element?.closest?.('[data-silent-errors]') != null;

    // An HTTP error status (the response is not swapped: see `noSwap` in base.html)
    document.body.addEventListener('htmx:response:error', (event) => {
        const { response, text } = event.detail.ctx;
        if (response.status === 401) {
            redirectToLogin();
            return;
        }
        // A refused form (422) is swapped in with its reason, so it needs no toast
        if (response.status === 422 && /** @type {Element} */ (event.target).closest?.('[hx-status\\:422]')) return;
        if (!silent(event.target)) toast(errorMessage(response.status, text), { type: 'error' });
    });

    // No response at all (HTMX 4 reports every failure here; a request replaced by `hx-sync` is aborted on purpose)
    document.body.addEventListener('htmx:error', (event) => {
        const { ctx, error } = event.detail;
        if (!ctx?.request || ctx.response || error?.name === 'AbortError') return;
        if (!silent(event.target)) toast(t('networkError'), { type: 'error' });
    });
}

/**
 * HTMX restores the page on a history traversal whose entry it created. The viewer's entries
 * (`?photo=…&ctx=…`) belong to the page underneath, which must stay as it is: only a traversal that
 * changes the page itself is left to HTMX.
 */
function keepPageOnViewerTraversal() {
    const pageOf = (url) => {
        const parsed = new URL(url, location.origin);
        parsed.searchParams.delete('photo');
        parsed.searchParams.delete('ctx');
        return parsed.pathname + parsed.search;
    };
    let current = pageOf(location.href);
    document.addEventListener('htmx:after:history:update', () => {
        current = pageOf(location.href);
    });
    document.addEventListener('htmx:before:history:restore', (event) => {
        const next = pageOf(event.detail.path);
        if (next === current) event.preventDefault();
        else current = next;
    });
}

function initShortcuts() {
    const go = (url) => () => location.assign(url);
    registerShortcut('global', 'g p', go('/'));
    registerShortcut('global', 'g f', go('/folders'));
    registerShortcut('global', 'g u', go('/utilities'));
    registerShortcut('global', 'u', go('/upload'));
    registerShortcut('global', '?', () => {
        if (!isDialogOpen()) openDialog({ url: '/ui/dialogs/shortcuts' });
    });
}

initToasts();
initDialog();
initHtmxErrors();
initFlashToasts();
keepPageOnViewerTraversal();
// The login page has no shell to navigate to
if (document.getElementById('main')) {
    initShortcuts();
}
boot();
// Lets tests (and anything else) wait until the page is interactive
document.documentElement.dataset.appReady = '';
