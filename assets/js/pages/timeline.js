// The Timeline page: the grid plus its header (filter chips and the "On this day" row).

import '../grid/index.js';
import { register } from '../core/registry.js';
import { EVENTS, emit } from '../core/events.js';
import { setPref } from '../core/prefs.js';
import { watchTiles } from '../grid/thumbhash-observer.js';

/**
 * Filter chips: choosing one saves the pref and swaps the page content with HTMX (the template
 * sets the swap attributes on this element). The server rendered each chip with the URL to load.
 */
register('timeline-filters', (element) => {
    for (const chip of element.querySelectorAll('[data-filter]')) {
        if (chip.getAttribute('aria-pressed') === 'true') continue;
        chip.setAttribute('hx-get', chip.dataset.href);
        chip.addEventListener('click', () => setPref('filter', chip.dataset.filter));
    }
    window.htmx.process(element);
});

/** The "On this day" cards: activating one opens the viewer on that group. */
register('memories', (element) => {
    const thumbs = watchTiles(element);
    thumbs.observe(element);

    element.addEventListener('click', (event) => {
        const card = event.target.closest('.memory-card');
        if (!card) return;
        emit(card, EVENTS.photoActivate, {
            photoId: Number(card.dataset.photoId),
            index: 0,
            contextKey: card.dataset.context,
            tile: card,
        }, { cancelable: true });
    });

    return () => thumbs.destroy();
});
