// The "Select a Month" dialog (fragment /ui/grid/month-picker): choosing a month scrolls the
// grid to it. The dialog is opened by the month header button through HTMX.

import { register } from '../core/registry.js';
import { closeDialog } from '../core/dialog.js';

register('month-picker', (element) => {
    element.addEventListener('click', (event) => {
        const row = event.target.closest('[data-month]');
        if (!row) return;

        closeDialog();
        document.querySelector('.photo-grid')?.fpGrid?.scrollToMonth(row.dataset.month);
    });
});
