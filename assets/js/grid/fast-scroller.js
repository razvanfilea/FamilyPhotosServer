// The fast scroller (component `fast-scroller`), behaving like Android's MonthScrollIndicator:
// a thumb at the right edge that follows the page scroll and, when dragged, jumps to the month
// under it. It works from the grid's cached section positions, which are right for months that
// were never loaded because every section reserves its final height.

import { register } from '../core/registry.js';
import { EVENTS, on } from '../core/events.js';
import { scrollTopForSection, sectionForProgress } from './geometry.js';

/** The thumb disappears this long after the last scroll event */
const HIDE_DELAY = 2500;

register('fast-scroller', (element) => {
    const grid = document.getElementById(element.dataset.grid);
    const thumb = element.querySelector('.fast-scroller__thumb');
    const label = element.querySelector('.fast-scroller__label');
    if (!grid?.fpGrid) return undefined;
    const { fpGrid } = grid;

    const abort = new AbortController();
    const listen = { signal: abort.signal };
    let selecting = false;
    let dragging = false;
    let hovered = false;
    let hideTimer;
    let lastJumpedSection = -1;
    let updateQueued = false;

    const maxScroll = () => document.documentElement.scrollHeight - window.innerHeight;
    const enabled = () => !element.hidden && !selecting;
    /** Space the thumb can travel in */
    const travel = () => Math.max(0, element.clientHeight - thumb.offsetHeight);
    const setThumbY = (y) => element.style.setProperty('--thumb-y', `${y}px`);

    function setVisible(visible) {
        element.dataset.visible = String(visible && enabled());
    }

    function scheduleHide() {
        clearTimeout(hideTimer);
        if (!dragging && !hovered) hideTimer = setTimeout(() => setVisible(false), HIDE_DELAY);
    }

    function show() {
        if (!enabled()) return;
        setVisible(true);
        scheduleHide();
    }

    /** Follows the page scroll */
    function updatePassive() {
        updateQueued = false;
        if (dragging) return;
        const max = maxScroll();
        const progress = max > 0 ? Math.min(1, Math.max(0, window.scrollY / max)) : 0;
        setThumbY(progress * travel());
        thumb.setAttribute('aria-valuenow', String(Math.round(progress * 100)));
        thumb.setAttribute('aria-valuetext', fpGrid.currentSection()?.label ?? '');
    }

    function onScroll() {
        show();
        if (!updateQueued) {
            updateQueued = true;
            requestAnimationFrame(updatePassive);
        }
    }

    /** Jumps to the month for a position of the thumb, only when the month changes */
    function jumpToProgress(progress, force = false) {
        const { sections } = fpGrid.layout();
        const index = sectionForProgress(sections, progress, window.innerHeight, maxScroll(), fpGrid.gridTop());
        if (index < 0) return;

        label.textContent = sections[index].label;
        if (index === lastJumpedSection && !force) return;
        lastJumpedSection = index;

        window.scrollTo({
            top: scrollTopForSection(sections[index].top, fpGrid.gridTop(), window.innerHeight),
            behavior: 'instant',
        });
        navigator.vibrate?.(5);
    }

    const progressAt = (clientY, grabOffset) => {
        const range = travel();
        const y = Math.min(range, Math.max(0, clientY - element.getBoundingClientRect().top - grabOffset));
        setThumbY(y);
        return range > 0 ? y / range : 0;
    };

    let grabOffset = 0;

    thumb.addEventListener('pointerdown', (event) => {
        event.preventDefault();
        thumb.setPointerCapture(event.pointerId);
        dragging = true;
        element.dataset.dragging = 'true';
        grabOffset = event.clientY - thumb.getBoundingClientRect().top;
        lastJumpedSection = -1;
        clearTimeout(hideTimer);
        if (event.pointerType === 'touch') navigator.vibrate?.(10);
    }, listen);

    thumb.addEventListener('pointermove', (event) => {
        if (dragging) jumpToProgress(progressAt(event.clientY, grabOffset));
    }, listen);

    const endDrag = () => {
        if (!dragging) return;
        dragging = false;
        delete element.dataset.dragging;
        scheduleHide();
        updatePassive();
    };
    thumb.addEventListener('pointerup', endDrag, listen);
    thumb.addEventListener('pointercancel', endDrag, listen);

    // Clicking the track jumps there, with a mouse or pen only: a touch near the right edge is a tap
    // or a flick of the page (the track lets touches through on touch screens, see grid.css)
    element.addEventListener('pointerdown', (event) => {
        if (event.target !== element || (event.pointerType !== 'mouse' && event.pointerType !== 'pen')) return;
        const progress = progressAt(event.clientY, thumb.offsetHeight / 2);
        jumpToProgress(progress, true);
    }, listen);

    element.addEventListener('pointerenter', () => {
        hovered = true;
        clearTimeout(hideTimer);
    }, listen);
    element.addEventListener('pointerleave', () => {
        hovered = false;
        scheduleHide();
    }, listen);

    // Months can disappear when photos are removed: the scroller is for two or more
    const sizeObserver = new ResizeObserver(() => {
        element.hidden = fpGrid.layout().sections.length < 2;
        if (element.hidden) setVisible(false);
        updatePassive();
    });
    sizeObserver.observe(grid);

    on(document, EVENTS.selectionChange, (event) => {
        selecting = event.detail.count > 0;
        if (selecting) setVisible(false);
    }, listen);

    window.addEventListener('scroll', onScroll, { passive: true, ...listen });
    window.addEventListener('resize', onScroll, listen);
    updatePassive();

    return () => {
        abort.abort();
        clearTimeout(hideTimer);
        sizeObserver.disconnect();
    };
});
