// Duplicates: one group of identical photos at a time. The groups are rendered by the server as
// hidden sections; this module switches between them, zooms the large image and trashes the copies
// that aren't kept.
import { register } from '../core/registry.js';
import { postJson } from '../core/api.js';
import { openDialog } from '../core/dialog.js';
import { register as registerShortcut } from '../core/keyboard.js';
import { toast } from '../core/toast.js';
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
    const infoButton = document.querySelector('[data-testid="dup-info"]');
    const position = document.querySelector('[data-bind="position"]');
    const emptyState = document.querySelector('[data-empty]');
    const zoom = createZoom(stage, { target: image });

    let groups = [...root.querySelectorAll('[data-group]')];
    let current = 0;

    image.addEventListener('error', () => {
        const group = groups[current];
        if (group && !image.src.endsWith(group.dataset.fallbackUrl)) image.src = group.dataset.fallbackUrl;
    });

    function show(index) {
        current = Math.min(Math.max(index, 0), groups.length - 1);
        groups.forEach((group, i) => { group.hidden = i !== current; });
        zoom.setTarget(image);
        image.src = groups[current].dataset.imageUrl;
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

        toast(others.length === 1
            ? texts.messageMovedOne
            : texts.messageMovedMany.replace('{count}', String(others.length)), { type: 'success' });

        groups = groups.filter((candidate) => candidate !== group);
        group.remove();
        if (groups.length > 0) {
            show(current);
            return;
        }
        root.hidden = true;
        emptyState.hidden = false;
        infoButton.hidden = true;
        position.hidden = true;
    }

    root.addEventListener('click', (event) => {
        const button = /** @type {Element} */ (event.target).closest('button');
        if (!button) return;
        if (button.dataset.action === 'prev') go(-1);
        if (button.dataset.action === 'next') go(1);
        if (button.dataset.action === 'keep') keep(button).catch(() => {});
    });

    const onInfo = () => {
        openDialog({ url: `/ui/viewer/info/${groups[current].dataset.photoId}?dialog=true` });
    };
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

    return () => {
        zoom.destroy();
        infoButton.removeEventListener('click', onInfo);
        unregister.forEach((fn) => fn());
    };
});
