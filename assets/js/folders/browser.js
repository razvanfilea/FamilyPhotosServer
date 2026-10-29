// The folder browser: filter chips, search, sort order, grid/list view and the list of folder
// cards. Used by the Folders tab and the destination picker (`data-picker`).
//
// Sort, view and filter changes are saved as prefs and re-fetch the list from
// `/ui/folders/list`; the search runs in the browser over the cards that are rendered.

import { register } from '../core/registry.js';
import { setPref } from '../core/prefs.js';
import { emit } from '../core/events.js';
import { drawThumbHash } from '../vendor-wrappers/thumbhash.js';
import { matchesQuery } from './search.js';

/** Fired on the browser element after the user picked another filter chip: {filter} */
export const FILTER_CHANGE_EVENT = 'fp:folder-filter-change';

const CHIP_ICONS = { all: 'photo_library', personal: 'person', family: 'family_restroom', shared: 'group' };

function setIcon(container, name) {
    const use = container.querySelector('use');
    use.setAttribute('href', use.getAttribute('href').replace(/#.*$/, `#${name}`));
}

register('folder-browser', (root) => {
    const picker = root.hasAttribute('data-picker');
    const search = root.querySelector('[data-role="search"]');
    const clear = root.querySelector('[data-role="search-clear"]');
    const sortLabel = root.querySelector('[data-role="sort-label"]');
    const viewToggle = root.querySelector('[data-role="view-toggle"]');
    const chips = root.querySelectorAll('.fp-chip[data-filter]');
    const sortOptions = root.querySelectorAll('[data-sort]');

    const state = {
        filter: root.dataset.filter,
        sort: root.querySelector('[data-sort][aria-checked="true"]').dataset.sort,
        view: viewToggle.dataset.view,
    };

    const list = () => root.querySelector('[data-role="list"]');

    /** Shows the cards matching the search, and "No folders match" if there are none */
    function applySearch() {
        const query = search.value;
        clear.hidden = query === '';

        const items = [...list().querySelectorAll('li[data-name]')];
        let visible = 0;
        for (const item of items) {
            const matches = matchesQuery(item.dataset.name, query);
            item.hidden = !matches;
            if (matches) visible++;
        }
        const noMatch = list().querySelector('[data-role="no-match"]');
        if (noMatch) noMatch.hidden = visible > 0;
    }

    function drawCovers() {
        for (const canvas of list().querySelectorAll('canvas[data-thumbhash]')) {
            const hash = canvas.dataset.thumbhash;
            if (!hash || canvas.dataset.drawn) continue;
            canvas.dataset.drawn = '';
            try {
                drawThumbHash(canvas, hash);
            } catch {
                // A malformed hash leaves the cover grey
            }
        }
    }

    /** Keeps the search in the URL; `savedPref` drops the overrides, which the prefs now hold */
    function updateUrl({ savedPref = false } = {}) {
        if (picker) return;
        const params = new URLSearchParams(location.search);
        const query = search.value.trim();
        if (query) params.set('q', query);
        else params.delete('q');
        if (savedPref) {
            for (const key of ['filter', 'sort', 'view']) params.delete(key);
        }
        const queryString = params.toString();
        history.replaceState(history.state, '', queryString ? `${location.pathname}?${queryString}` : location.pathname);
    }

    async function refresh() {
        const query = new URLSearchParams({ filter: state.filter, sort: state.sort, view: state.view });
        if (picker) query.set('picker', 'true');
        await window.htmx.ajax('GET', `/ui/folders/list?${query}`, { source: root, target: list(), swap: 'outerHTML' });
        drawCovers();
        applySearch();
    }

    root.addEventListener('click', (event) => {
        const chip = /** @type {Element} */ (event.target).closest('.fp-chip[data-filter]');
        if (chip && root.contains(chip)) {
            if (chip.dataset.filter === state.filter) return;
            state.filter = chip.dataset.filter;
            root.dataset.filter = state.filter;
            for (const other of chips) {
                const selected = other === chip;
                other.setAttribute('aria-pressed', String(selected));
                setIcon(other, selected ? 'check' : CHIP_ICONS[other.dataset.filter]);
            }
            setPref('filter', state.filter);
            emit(root, FILTER_CHANGE_EVENT, { filter: state.filter });
            updateUrl({ savedPref: true });
            refresh();
            return;
        }

        const sortOption = /** @type {Element} */ (event.target).closest('[data-sort]');
        if (sortOption && root.contains(sortOption)) {
            state.sort = sortOption.dataset.sort;
            for (const option of sortOptions) {
                option.setAttribute('aria-checked', String(option === sortOption));
            }
            sortLabel.textContent = sortOption.dataset.label;
            setPref('fsort', state.sort);
            updateUrl({ savedPref: true });
            refresh();
            return;
        }

        if (/** @type {Element} */ (event.target).closest('[data-role="view-toggle"]')) {
            state.view = state.view === 'grid' ? 'list' : 'grid';
            viewToggle.dataset.view = state.view;
            viewToggle.setAttribute('aria-label', state.view === 'grid' ? 'Switch to list view' : 'Switch to grid view');
            setIcon(viewToggle, state.view === 'grid' ? 'view_list' : 'grid_view');
            setPref('fview', state.view);
            updateUrl({ savedPref: true });
            refresh();
            return;
        }

        if (/** @type {Element} */ (event.target).closest('[data-role="search-clear"]')) {
            search.value = '';
            applySearch();
            updateUrl();
            search.focus();
        }
    });

    search.addEventListener('input', () => {
        applySearch();
        updateUrl();
    });
    search.addEventListener('keydown', (event) => {
        if (event.key === 'Escape' && search.value !== '') {
            // Clears the search instead of closing the dialog the picker may be in
            event.preventDefault();
            event.stopPropagation();
            search.value = '';
            applySearch();
            updateUrl();
        }
    });

    drawCovers();
    applySearch();
});
