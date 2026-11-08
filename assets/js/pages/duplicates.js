// Duplicates: one group of identical photos at a time. The groups are rendered by the server as
// hidden sections; this module switches between them, zooms the large image and trashes the copies
// that aren't kept. The server marks the suggested copy of each group and the bulk action that
// resolves it (`data-bulk`); this module sums them up, and trashes the other copies of every group
// of a bulk action in one request.
import { register } from '../core/registry.js';
import { postJson } from '../core/api.js';
import { openDialog } from '../core/dialog.js';
import { register as registerShortcut } from '../core/keyboard.js';
import { toast } from '../core/toast.js';
import { formatBytes, plural } from '../core/format.js';
import { openDeleteDialog } from '../viewer/index.js';
import { createZoom } from '../viewer/zoom.js';

/** Horizontal distance of a swipe between groups, in px */
const SWIPE_DISTANCE = 60;

register('duplicates', (root) => {
    const texts = root.dataset;
    const stage = root.querySelector('[data-slot="stage"]');
    const image = stage.querySelector('[data-slot="image"]');
    const prevButton = stage.querySelector('[data-action="prev"]');
    const nextButton = stage.querySelector('[data-action="next"]');
    const spinner = stage.querySelector('[data-slot="spinner"]');
    const summary = root.querySelector('[data-bind="summary"]');
    const bulkButtons = [...root.querySelectorAll('[data-action="bulk"]')];
    const infoButton = document.querySelector('[data-testid="dup-info"]');
    const position = document.querySelector('[data-bind="position"]');
    const emptyState = document.querySelector('[data-empty]');
    const zoom = createZoom(stage, { target: image });

    let groups = [...root.querySelectorAll('[data-group]')];
    let current = 0;

    image.addEventListener('load', () => { spinner.hidden = true; });
    image.addEventListener('error', () => {
        const group = groups[current];
        if (group && !image.src.endsWith(group.dataset.fallbackUrl)) {
            image.src = group.dataset.fallbackUrl;
            return;
        }
        spinner.hidden = true;
    });

    /** The copies of a group that go to the trash when its suggested copy is kept */
    const extraCopies = (group) => [...group.querySelectorAll('[data-action="keep"]:not([data-suggested])')]
        .map((copy) => Number(copy.dataset.photoId));

    /** The groups a bulk action applies to: `family` or `extra` */
    function bulkGroups(kind) {
        return groups.filter((group) => group.dataset.bulk === kind);
    }

    const sumBytes = (list) => list.reduce((total, group) => total + Number(group.dataset.freedBytes), 0);

    function updateSummary() {
        summary.textContent = (groups.length === 1 ? texts.labelSummaryOne : texts.labelSummaryMany)
            .replace('{count}', String(groups.length))
            .replace('{size}', formatBytes(sumBytes(groups)));
        for (const button of bulkButtons) {
            const count = bulkGroups(button.dataset.bulk).length;
            button.hidden = count === 0;
            button.textContent = (button.dataset.bulk === 'family' ? texts.labelBulkFamily : texts.labelBulkExtra)
                .replace('{count}', String(count));
        }
    }

    function show(index) {
        current = Math.min(Math.max(index, 0), groups.length - 1);
        groups.forEach((group, i) => { group.hidden = i !== current; });
        zoom.setTarget(image);
        const url = groups[current].dataset.imageUrl;
        // Setting the same source again doesn't load it again
        if (image.getAttribute('src') !== url) {
            spinner.hidden = false;
            image.src = url;
        }
        prevButton.disabled = current === 0;
        nextButton.disabled = current === groups.length - 1;
        position.textContent = texts.labelPosition
            .replace('{current}', String(current + 1))
            .replace('{total}', String(groups.length));
    }

    function go(step) {
        const target = current + step;
        if (target >= 0 && target < groups.length) show(target);
    }

    /** Takes the groups off the page; the one on screen stays unless it is among them */
    function removeGroups(removed) {
        const shown = groups[current];
        const before = groups.slice(0, current).filter((group) => !removed.includes(group)).length;
        groups = groups.filter((group) => !removed.includes(group));
        removed.forEach((group) => group.remove());
        if (groups.length > 0) {
            show(groups.includes(shown) ? groups.indexOf(shown) : before);
            updateSummary();
            return;
        }
        root.hidden = true;
        emptyState.hidden = false;
        infoButton.hidden = true;
        position.hidden = true;
    }

    const movedToast = (count) => toast(count === 1
        ? texts.messageMovedOne
        : texts.messageMovedMany.replace('{count}', String(count)), { type: 'success' });

    async function keep(button) {
        const group = button.closest('[data-group]');
        const keptId = Number(button.dataset.photoId);
        const others = [...group.querySelectorAll('[data-action="keep"]')]
            .map((copy) => Number(copy.dataset.photoId))
            .filter((id) => id !== keptId);

        const trashed = await openDeleteDialog({
            ids: others,
            onConfirm: () => postJson('/api/trash', others, { silent: true }),
        });
        if (!trashed) return;

        movedToast(others.length);
        removeGroups([group]);
    }

    /** Keeps the suggested copy of every group of a bulk action, after asking */
    async function bulk(kind) {
        const affected = bulkGroups(kind);
        const ids = affected.flatMap(extraCopies);
        if (ids.length === 0) return;

        const trashed = await openDeleteDialog({
            ids,
            title: (kind === 'family' ? texts.labelBulkFamily : texts.labelBulkExtra).replace(' ({count})', ''),
            message: texts.messageBulk
                .replace('{photos}', plural(ids.length, 'photo'))
                .replace('{size}', formatBytes(sumBytes(affected))),
            confirm: texts.confirmBulk,
            onConfirm: () => postJson('/api/trash', ids, { silent: true }),
        });
        if (!trashed) return;

        movedToast(ids.length);
        removeGroups(affected);
    }

    root.addEventListener('click', (event) => {
        const button = /** @type {Element} */ (event.target).closest('button');
        if (!button) return;
        if (button.dataset.action === 'prev') go(-1);
        if (button.dataset.action === 'next') go(1);
        if (button.dataset.action === 'keep') keep(button).catch(() => {});
        if (button.dataset.action === 'info') openInfo(button.dataset.photoId);
        if (button.dataset.action === 'bulk') bulk(button.dataset.bulk).catch(() => {});
    });

    const openInfo = (photoId) => openDialog({ url: `/ui/viewer/info/${photoId}?dialog=true` });
    const onInfo = () => openInfo(groups[current].dataset.photoId);
    infoButton.addEventListener('click', onInfo);

    // Swipes between groups; a pinch or a pan of the zoomed image is not one
    let swipe = null;
    stage.addEventListener('pointerdown', (event) => {
        const valid = event.pointerType !== 'mouse' && !event.target.closest('button') && zoom.pointerCount() <= 1;
        swipe = valid ? { x: event.clientX, y: event.clientY } : null;
    });
    stage.addEventListener('pointerup', (event) => {
        if (!swipe || zoom.isZoomed()) return;
        const dx = event.clientX - swipe.x;
        const dy = event.clientY - swipe.y;
        swipe = null;
        if (Math.abs(dx) > SWIPE_DISTANCE && Math.abs(dx) > Math.abs(dy) * 1.5) go(dx < 0 ? 1 : -1);
    });
    stage.addEventListener('pointercancel', () => { swipe = null; });

    const unregister = [
        registerShortcut('global', 'ArrowLeft', () => go(-1)),
        registerShortcut('global', 'ArrowRight', () => go(1)),
    ];

    show(0);
    updateSummary();

    return () => {
        zoom.destroy();
        infoButton.removeEventListener('click', onInfo);
        unregister.forEach((fn) => fn());
    };
});
