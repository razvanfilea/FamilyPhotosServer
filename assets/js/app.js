// Entry point loaded by every page: boots the core modules, the global HTMX error handling and the
// shell components. Page-specific modules are loaded by their templates.

import { boot } from './core/registry.js';
import { initToasts, toast } from './core/toast.js';
import { initDialog, isDialogOpen, openDialog } from './core/dialog.js';
import { register as registerShortcut } from './core/keyboard.js';
import { errorMessage, redirectToLogin } from './core/api.js';
import { t } from './core/strings.js';
import './components/shell.js';
// The destination picker (and the folder browser and name dialog it uses) is opened from dialogs on many pages
import './destination-picker/picker.js';

function initHtmxErrors() {
    const silent = (element) => element?.closest?.('[data-silent-errors]') != null;

    document.body.addEventListener('htmx:responseError', (event) => {
        const { xhr, elt } = event.detail;
        if (xhr.status === 401) {
            redirectToLogin();
            return;
        }
        if (!silent(elt)) {
            toast(errorMessage(xhr.status, xhr.responseText), { type: 'error' });
        }
    });

    document.body.addEventListener('htmx:sendError', (event) => {
        if (!silent(event.detail.elt)) {
            toast(t('networkError'), { type: 'error' });
        }
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
// The login page has no shell to navigate to
if (document.getElementById('main')) {
    initShortcuts();
}
boot();
// Lets tests (and anything else) wait until the page is interactive
document.documentElement.dataset.appReady = '';
