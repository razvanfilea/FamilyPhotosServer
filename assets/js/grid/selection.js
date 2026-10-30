// Selection of the photo grid (component `grid-selection`, on the same element as `photo-grid`).
// The set of selected ids lives here, not in the DOM: sections that unload and reload get their
// tiles painted again (`fp:section-loaded`), and ranges and months can cover photos whose tiles
// were never loaded (their ids come from the server). Selection mode is "the set isn't empty".
// The element API is `element.fpSelection`; changes are announced with `fp:selection-change`.

import { register } from '../core/registry.js';
import { getJson } from '../core/api.js';
import { EVENTS, emit, on } from '../core/events.js';
import { initDrag } from './selection-drag.js';
import { register as registerShortcut } from '../core/keyboard.js';
import { missingRequests, monthOf, rangeIndices, summarize } from '../selection/math.js';

const TILE = '.photo-tile';

register('grid-selection', (root) => {
    const abort = new AbortController();
    const listen = { signal: abort.signal };
    const contextKey = root.dataset.context;

    /** @type {Map<number, {month?: string, deletable?: boolean, owner?: string | null}>} selected id → what is known about it */
    const selected = new Map();
    /** Context index of the last toggled tile: where Shift+click ranges start */
    let anchorIndex = null;
    /** Number of requests resolving ids that aren't loaded */
    let resolving = 0;
    /** Context index → photo, for ranges resolved from the server; invalid once indices shift */
    let resolved = new Map();
    let rangeToken = 0;
    let swallowClicksUntil = 0;

    const idOf = (tile) => Number(tile.dataset.photoId);
    const indexOf = (tile) => Number(tile.dataset.index);
    const sections = () => root.querySelectorAll(':scope > .month');

    function metaOf(tile) {
        return {
            month: tile.closest('.month')?.dataset.month,
            deletable: tile.dataset.deletable === 'true',
            owner: tile.dataset.owner || null,
        };
    }

    // ---------- Painting ----------

    function paint(tile) {
        const isSelected = selected.has(idOf(tile));
        tile.classList.toggle('is-selected', isSelected);
        if (isSelected) tile.setAttribute('aria-pressed', 'true');
        else if (selected.size > 0) tile.setAttribute('aria-pressed', 'false');
        else tile.removeAttribute('aria-pressed');
    }

    function paintHeaders() {
        const perMonth = new Map();
        for (const { month } of selected.values()) {
            if (month) perMonth.set(month, (perMonth.get(month) ?? 0) + 1);
        }
        for (const section of sections()) {
            const count = perMonth.get(section.dataset.month) ?? 0;
            const all = count > 0 && count >= Number(section.dataset.count);
            section.querySelector('[data-action="month-select"]')?.setAttribute('aria-pressed', String(all));
        }
    }

    /** Repaints everything and tells the others (bar, fast scroller) */
    function changed() {
        root.classList.toggle('selecting', selected.size > 0);
        for (const tile of root.querySelectorAll(TILE)) paint(tile);
        paintHeaders();
        emit(root, EVENTS.selectionChange, {
            count: selected.size,
            ids: new Set(selected.keys()),
            contextKey,
            resolving: resolving > 0,
        });
    }

    function replace(next) {
        selected.clear();
        for (const [id, meta] of next) selected.set(id, meta);
        changed();
    }

    // ---------- Operations ----------

    function toggleTile(tile) {
        const id = idOf(tile);
        if (!selected.delete(id)) selected.set(id, metaOf(tile));
        anchorIndex = indexOf(tile);
        changed();
    }

    async function resolveChunk({ start, limit }) {
        const response = await getJson('/api/context/neighbors', { query: { ctx: contextKey, start, limit } });
        response.photos.forEach((photo, offset) => {
            resolved.set(response.start + offset, {
                id: photo.id,
                meta: { month: monthOf(photo.created_at), deletable: photo.deletable, owner: photo.user_id },
            });
        });
    }

    /**
     * Makes the selection `base` plus every photo from index `from` to `to`. The loaded part
     * applies at once; the rest is resolved with the server, unless a newer range replaced this one.
     */
    async function selectRange(base, from, to) {
        const token = ++rangeToken;
        const indices = rangeIndices(from, to);
        const loaded = new Map();
        for (const tile of root.querySelectorAll(TILE)) loaded.set(indexOf(tile), tile);

        const build = () => {
            const next = new Map(base);
            for (const index of indices) {
                const tile = loaded.get(index);
                const photo = resolved.get(index);
                if (tile) next.set(idOf(tile), metaOf(tile));
                else if (photo) next.set(photo.id, photo.meta);
            }
            return next;
        };

        const requests = missingRequests(indices, (index) => loaded.has(index) || resolved.has(index));
        if (requests.length === 0) {
            replace(build());
            return;
        }

        resolving += 1;
        replace(build());
        try {
            await Promise.all(requests.map(resolveChunk));
        } catch {
            // Reported by the API helper; what is loaded stays selected
        } finally {
            resolving -= 1;
        }
        if (token === rangeToken) replace(build());
        else changed();
    }

    function selectRangeTo(tile) {
        const index = indexOf(tile);
        if (anchorIndex === null) {
            toggleTile(tile);
            return;
        }
        const from = anchorIndex;
        anchorIndex = index;
        selectRange(new Map(selected), from, index);
    }

    /** Selects a month, or deselects it when it is selected completely */
    async function toggleMonth(section) {
        const month = section.dataset.month;
        const count = Number(section.dataset.count);
        const inMonth = [...selected].filter(([, meta]) => meta.month === month);
        if (inMonth.length >= count) {
            for (const [id] of inMonth) selected.delete(id);
            changed();
            return;
        }

        const loaded = [...section.querySelectorAll(TILE)];
        let ids = loaded.map(idOf);
        if (loaded.length < count) {
            resolving += 1;
            changed();
            try {
                ids = await getJson('/api/context/ids', { query: { ctx: contextKey, month } });
            } catch {
                ids = [];
            } finally {
                resolving -= 1;
            }
        }
        for (const id of ids) {
            if (!selected.has(id)) selected.set(id, { month });
        }
        for (const tile of loaded) selected.set(idOf(tile), metaOf(tile));
        changed();
    }

    function selectLoaded() {
        const next = new Map(selected);
        for (const tile of root.querySelectorAll(TILE)) next.set(idOf(tile), metaOf(tile));
        replace(next);
    }

    function clear() {
        rangeToken += 1;
        if (selected.size === 0 && resolving === 0) return;
        selected.clear();
        anchorIndex = null;
        changed();
    }

    // ---------- Input ----------

    // Captured, so the grid's own click handler (which opens the viewer) sees `defaultPrevented`
    root.addEventListener('click', (event) => {
        if (Date.now() < swallowClicksUntil) {
            event.preventDefault();
            return;
        }
        const target = /** @type {Element} */ (event.target);
        const header = target.closest('[data-action="month-select"]');
        if (header) {
            toggleMonth(header.closest('.month'));
            return;
        }
        const tile = target.closest(TILE);
        if (!tile) return;

        if (event.shiftKey) {
            event.preventDefault();
            selectRangeTo(tile);
        } else if (event.ctrlKey || event.metaKey || selected.size > 0 || target.closest('.photo-tile__check')) {
            event.preventDefault();
            toggleTile(tile);
        }
    }, { capture: true, ...listen });

    // Shift+click would select text, long-press would show the image menu, dragging would drag the image
    root.addEventListener('dragstart', (event) => event.preventDefault(), listen);
    let lastPointerType = 'mouse';
    root.addEventListener('pointerdown', (event) => {
        lastPointerType = event.pointerType;
    }, { capture: true, ...listen });
    root.addEventListener('contextmenu', (event) => {
        if (lastPointerType !== 'mouse' && /** @type {Element} */ (event.target).closest(TILE)) event.preventDefault();
    }, listen);

    const cleanupDrag = initDrag(root, {
        /** Selects the tile if needed, makes it the anchor, and returns the selection from before */
        begin(tile) {
            const base = new Map(selected);
            if (!selected.has(idOf(tile))) {
                selected.set(idOf(tile), metaOf(tile));
                changed();
            }
            anchorIndex = indexOf(tile);
            return { base, anchor: anchorIndex };
        },
        extend: selectRange,
        swallowClicks(ms) {
            swallowClicksUntil = Date.now() + ms;
        },
    });

    const focusedTile = () => document.activeElement?.closest?.(TILE);
    // Handlers returning false leave the key to the other scopes (Escape without a selection)
    const unregisterKeys = [
        registerShortcut('grid', 'ctrl+a', selectLoaded),
        registerShortcut('grid', 'meta+a', selectLoaded),
        registerShortcut('grid', 'Escape', () => selected.size > 0 ? clear() : false),
        registerShortcut('grid', 'x', () => {
            const tile = focusedTile();
            return tile ? toggleTile(tile) : false;
        }),
    ];

    // ---------- Events from the rest of the page ----------

    // Tiles of a loaded section reflect the selection, and now tell what we only knew from the server
    root.addEventListener(EVENTS.sectionLoaded, (event) => {
        let refreshed = false;
        for (const tile of event.detail.tiles) {
            const id = idOf(tile);
            if (selected.has(id)) {
                selected.set(id, metaOf(tile));
                refreshed = true;
            }
            paint(tile);
        }
        if (refreshed) changed();
        else paintHeaders();
    }, listen);

    on(document, EVENTS.photosRemoved, (event) => {
        const { ids, contextKey: removedFrom } = event.detail;
        if (removedFrom && removedFrom !== contextKey) return;
        // Indices shift when tiles go
        resolved = new Map();
        anchorIndex = null;
        let any = false;
        for (const id of ids) any = selected.delete(Number(id)) || any;
        if (any) changed();
    }, listen);

    on(document, EVENTS.photosAdded, () => {
        resolved = new Map();
        anchorIndex = null;
    }, listen);

    root.fpSelection = {
        contextKey,
        /** Selected ids, in the order they were selected */
        ids: () => [...selected.keys()],
        count: () => selected.size,
        /** `{count, undeletable, foreign}`: see `summarize` in selection/math.js */
        summary: (userId) => summarize(selected.values(), userId),
        clear,
    };

    return () => {
        abort.abort();
        cleanupDrag();
        for (const unregister of unregisterKeys) unregister();
        delete root.fpSelection;
        if (selected.size > 0) {
            selected.clear();
            emit(document, EVENTS.selectionChange, { count: 0, ids: new Set(), contextKey, resolving: false });
        }
    };
});
