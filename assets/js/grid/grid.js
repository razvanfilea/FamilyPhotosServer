// The photo grid (component `photo-grid`): month sections whose height is reserved by CSS from
// their photo count, so the scroll position is meaningful before anything is loaded. Sections
// load their tiles from /ui/grid/month when they come near the viewport and drop them again when
// they are far away.
//
// The element API, `element.fpGrid`, is what the fast scroller, the month picker, the selection
// and the viewer use: no globals.

import { register } from '../core/registry.js';
import { api, isAbortError } from '../core/api.js';
import { EVENTS, emit, on } from '../core/events.js';
import { plural } from '../core/format.js';
import { setPref } from '../core/prefs.js';
import { watchTiles } from './thumbhash-observer.js';
import { initZoomInput } from './zoom.js';
import {
    JUMP_OFFSET,
    clampZoom,
    findItemAt,
    navigateIndex,
    scrollTopForSection,
    sectionForIndex,
    tilesNeeded,
} from './geometry.js';

/** Viewport heights around the viewport in which sections load their tiles */
const LOAD_MARGIN = 2;
/** Sections farther away than this many viewport heights drop their tiles */
const UNLOAD_MARGIN = 6;
const MAX_CONCURRENT_LOADS = 3;
/** A failed section is not requested again for this long */
const RETRY_DELAY = 5000;

const TILE = '.photo-tile';

register('photo-grid', (root) => {
    const abort = new AbortController();
    const listen = { signal: abort.signal };
    const thumbs = watchTiles(root);

    /** @type {WeakMap<Element, {request: AbortController | null, failedAt: number, pin: number, waiters: {count: number, resolve: () => void}[]}>} */
    const states = new WeakMap();
    let layoutCache = null;
    let pumpQueued = false;
    let runningLoads = 0;

    const contextKey = root.dataset.context;
    const gridOf = (section) => section.querySelector('.month__grid');
    const tilesOf = (section) => section.querySelectorAll(TILE);
    const tileCount = (section) => tilesOf(section).length;
    const sectionElements = () => [...root.querySelectorAll(':scope > .month')];
    const level = () => Number(root.dataset.zoom);

    function stateOf(section) {
        let state = states.get(section);
        if (!state) {
            state = { request: null, failedAt: 0, pin: 0, waiters: [] };
            states.set(section, state);
        }
        return state;
    }

    // ---------- Layout ----------

    /**
     * Positions of the sections relative to the grid, cached until the grid changes size.
     * Possible because every section has its final height from the start.
     */
    function layout() {
        if (!layoutCache) {
            const style = getComputedStyle(root);
            const cols = parseInt(style.getPropertyValue('--cols'), 10);
            const gap = parseFloat(style.getPropertyValue('--gap'));
            const metrics = { cols, gap, cell: (root.clientWidth - (cols - 1) * gap) / cols };

            const elements = sectionElements();
            const sections = elements.map((element, i) => {
                const top = element.offsetTop;
                const next = elements[i + 1];
                return {
                    element,
                    month: element.dataset.month ?? null,
                    label: element.dataset.label ?? '',
                    count: Number(element.dataset.count),
                    start: Number(element.dataset.startIndex),
                    top,
                    end: next ? next.offsetTop : root.offsetHeight,
                    gridTop: gridOf(element).offsetTop,
                };
            });
            layoutCache = { ...metrics, sections };
        }
        return layoutCache;
    }

    const invalidateLayout = () => {
        layoutCache = null;
        schedulePump();
    };

    /** Document position of the top of the grid */
    const gridTop = () => root.getBoundingClientRect().top + window.scrollY;

    // ---------- Loading and unloading ----------

    function schedulePump() {
        if (pumpQueued) return;
        pumpQueued = true;
        requestAnimationFrame(pump);
    }

    function canUnload(section, state) {
        return !state.request
            && state.pin === 0
            && !section.querySelector('.is-selected')
            && !section.contains(document.activeElement);
    }

    function pump() {
        pumpQueued = false;
        if (!root.isConnected) return;

        const metrics = layout();
        const viewport = window.innerHeight;
        const viewTop = -root.getBoundingClientRect().top;
        const nearTop = viewTop - LOAD_MARGIN * viewport;
        const nearBottom = viewTop + (1 + LOAD_MARGIN) * viewport;
        const farTop = viewTop - UNLOAD_MARGIN * viewport;
        const farBottom = viewTop + (1 + UNLOAD_MARGIN) * viewport;
        const now = Date.now();
        const wanted = [];

        for (const section of metrics.sections) {
            const state = stateOf(section.element);
            const loaded = section.element.dataset.loaded === 'true' ? tileCount(section.element) : 0;
            const near = section.end > nearTop && section.top < nearBottom;
            const far = section.end > farTop && section.top < farBottom;

            let want = near ? tilesNeeded(section, metrics, nearBottom) : 0;
            want = Math.min(Math.max(want, state.pin), section.count);

            if (want > loaded && !state.request && now - state.failedAt > RETRY_DELAY) {
                const middle = (viewTop + viewport / 2);
                const distance = Math.max(section.top - middle, middle - section.end, 0);
                wanted.push({ section, distance });
            }
            if (!far) {
                state.request?.abort();
                if (loaded > 0 && canUnload(section.element, state)) unload(section.element);
            }
        }

        wanted.sort((a, b) => a.distance - b.distance);
        for (const { section } of wanted.slice(0, Math.max(0, MAX_CONCURRENT_LOADS - runningLoads))) {
            load(section.element);
        }
    }

    async function load(section) {
        const state = stateOf(section);
        const controller = new AbortController();
        const offset = tileCount(section);
        state.request = controller;
        runningLoads += 1;

        try {
            const html = await api('/ui/grid/month', {
                query: { ctx: contextKey, month: section.dataset.month, offset },
                signal: controller.signal,
            });
            // The section may have changed while loading (photos removed): ask again then
            if (!controller.signal.aborted && section.isConnected && tileCount(section) === offset) {
                if (!insertPage(section, html ?? '')) state.failedAt = Date.now();
            }
        } catch (error) {
            if (!isAbortError(error)) state.failedAt = Date.now();
        } finally {
            const aborted = controller.signal.aborted;
            state.request = null;
            runningLoads -= 1;
            // An aborted load (the section went far away or was removed) doesn't keep anyone waiting
            settleWaiters(section, aborted);
            schedulePump();
        }
    }

    /** @returns {boolean} whether any tile was added */
    function insertPage(section, html) {
        const grid = gridOf(section);
        grid.querySelector('.month__more')?.remove();

        const template = document.createElement('template');
        template.innerHTML = html;
        const tiles = [...template.content.querySelectorAll(TILE)];
        grid.append(template.content);

        section.dataset.loaded = 'true';
        prepareTiles(tiles);
        thumbs.observe(tiles);
        if (tiles.length > 0) {
            emit(section, EVENTS.sectionLoaded, { section, month: section.dataset.month ?? null, tiles });
        }
        return tiles.length > 0;
    }

    function unload(section) {
        const grid = gridOf(section);
        thumbs.unobserve(grid);
        grid.replaceChildren();
        section.dataset.loaded = 'false';
        ensureRoving();
    }

    /**
     * Loads a section until it has at least `count` tiles (from its start).
     * @returns {Promise<void>} settles when that is the case, or when loading failed, was aborted
     * or the section was removed
     */
    function ensureLoaded(section, count) {
        const state = stateOf(section);
        const wanted = Math.min(count, Number(section.dataset.count));
        if (tileCount(section) >= wanted) return Promise.resolve();

        state.failedAt = 0;
        state.pin = Math.max(state.pin, wanted);
        return new Promise((resolve) => {
            state.waiters.push({ count: wanted, resolve });
            schedulePump();
        });
    }

    /** Resolves the waiters of a section that have their tiles, or all of them with `all` */
    function settleWaiters(section, all = false) {
        const state = stateOf(section);
        const loaded = tileCount(section);
        // Removals can make a section smaller than what was asked for
        const count = Number(section.dataset.count);
        const failed = state.failedAt > 0;
        state.waiters = state.waiters.filter((waiter) => {
            const done = all || failed || !section.isConnected || loaded >= Math.min(waiter.count, count);
            if (done) waiter.resolve();
            return !done;
        });
        if (state.waiters.length === 0) state.pin = 0;
    }

    // ---------- Tiles: focus, activation, keyboard ----------

    /** Roving tabindex: only one tile is a tab stop */
    function prepareTiles(tiles) {
        for (const tile of tiles) tile.tabIndex = -1;
        ensureRoving();
    }

    function ensureRoving() {
        if (root.querySelector(`${TILE}[tabindex="0"]`)) return;
        const first = root.querySelector(TILE);
        if (first) first.tabIndex = 0;
    }

    const tileById = (id) => root.querySelector(`${TILE}[data-photo-id="${id}"]`);
    const tileByIndex = (index) => root.querySelector(`${TILE}[data-index="${index}"]`);

    function focusTile(tile) {
        root.querySelector(`${TILE}[tabindex="0"]`)?.setAttribute('tabindex', '-1');
        tile.tabIndex = 0;
        tile.focus({ preventScroll: true });
    }

    root.addEventListener('focusin', (event) => {
        const tile = /** @type {Element} */ (event.target).closest?.(TILE);
        if (!tile || tile.tabIndex === 0) return;
        root.querySelector(`${TILE}[tabindex="0"]`)?.setAttribute('tabindex', '-1');
        tile.tabIndex = 0;
    }, listen);

    root.addEventListener('click', (event) => {
        const tile = /** @type {Element} */ (event.target).closest(TILE);
        // The check circle belongs to the selection
        if (!tile || event.defaultPrevented || /** @type {Element} */ (event.target).closest('.photo-tile__check')) return;
        emit(tile, EVENTS.photoActivate, {
            photoId: Number(tile.dataset.photoId),
            index: Number(tile.dataset.index),
            contextKey,
            tile,
        }, { cancelable: true });
    }, listen);

    const NAVIGATION_KEYS = new Set(['ArrowLeft', 'ArrowRight', 'ArrowUp', 'ArrowDown', 'Home', 'End', 'PageUp', 'PageDown']);

    root.addEventListener('keydown', async (event) => {
        const tile = /** @type {Element} */ (event.target).closest?.(TILE);
        if (!tile || !NAVIGATION_KEYS.has(event.key) || event.ctrlKey || event.metaKey || event.altKey) return;
        event.preventDefault();

        const metrics = layout();
        const pageRows = Math.max(1, Math.floor(window.innerHeight / (metrics.cell + metrics.gap)) - 1);
        const target = navigateIndex(metrics.sections, metrics.cols, Number(tile.dataset.index), event.key, pageRows);
        if (target !== null) await focusIndex(target);
    }, listen);

    async function focusIndex(index) {
        let tile = tileByIndex(index);
        if (!tile) {
            await scrollToIndex(index);
            tile = tileByIndex(index);
        }
        if (!tile) return;
        focusTile(tile);
        tile.scrollIntoView({ block: 'nearest' });
    }

    // ---------- Scrolling to things ----------

    function jumpTo(top) {
        window.scrollTo({ top: Math.max(0, Math.round(top)), behavior: 'instant' });
    }

    /** Scrolls to the tile row of a context index (loading it) without moving focus */
    async function scrollToIndex(index) {
        const metrics = layout();
        const found = sectionForIndex(metrics.sections, index);
        if (found < 0) return null;
        const section = metrics.sections[found];

        const row = Math.floor((index - section.start) / metrics.cols);
        const rowTop = gridTop() + section.gridTop + row * (metrics.cell + metrics.gap);
        jumpTo(rowTop - (window.innerHeight - metrics.cell) / 2);
        await ensureLoaded(section.element, index - section.start + 1);
        return tileByIndex(index);
    }

    /**
     * Scrolls a photo into view (loading its section if necessary), focuses its tile and
     * resolves to whether the tile was found.
     * @param {number} photoId
     * @param {number} [index] position of the photo in the context, needed if it isn't loaded
     */
    async function scrollToPhoto(photoId, index) {
        let tile = tileById(photoId);
        if (!tile && index !== undefined) {
            await scrollToIndex(index);
            tile = tileById(photoId);
        }
        if (!tile) return false;

        const rect = tile.getBoundingClientRect();
        const topInset = document.querySelector('.fp-topbar')?.offsetHeight ?? 0;
        if (rect.top < topInset || rect.bottom > window.innerHeight) {
            tile.scrollIntoView({ block: 'center' });
        }
        focusTile(tile);
        return true;
    }

    /** Puts a month's header 20% from the top of the viewport and loads the month. */
    async function scrollToMonth(month) {
        const section = layout().sections.find((candidate) => candidate.month === month);
        if (!section) return false;
        jumpTo(scrollTopForSection(section.top, gridTop(), window.innerHeight));
        await ensureLoaded(section.element, 1);
        return true;
    }

    // ---------- Zoom ----------

    /** The tile under a point (or the nearest visible one), to keep it in place while the columns change */
    function captureAnchor(point) {
        const x = point?.x ?? window.innerWidth / 2;
        const y = point?.y ?? window.innerHeight / 2;
        let anchor = document.elementFromPoint(x, y)?.closest?.(TILE);

        if (!anchor || !root.contains(anchor)) {
            let nearest = Infinity;
            anchor = null;
            for (const tile of root.querySelectorAll(TILE)) {
                const rect = tile.getBoundingClientRect();
                if (rect.bottom < 0 || rect.top > window.innerHeight) continue;
                const distance = Math.hypot(rect.left + rect.width / 2 - x, rect.top + rect.height / 2 - y);
                if (distance < nearest) {
                    nearest = distance;
                    anchor = tile;
                }
            }
        }
        return anchor && { element: anchor, top: anchor.getBoundingClientRect().top };
    }

    function setZoom(newLevel, point) {
        newLevel = clampZoom(newLevel);
        if (newLevel === level()) return;

        const anchor = captureAnchor(point);
        root.dataset.zoom = String(newLevel);
        setPref('zoom', newLevel);
        layoutCache = null;
        if (anchor) window.scrollBy({ top: anchor.element.getBoundingClientRect().top - anchor.top, behavior: 'instant' });

        emit(root, EVENTS.gridZoom, { level: newLevel });
        schedulePump();
    }

    on(document, EVENTS.gridZoom, (event) => setZoom(event.detail.level), listen);
    const cleanupZoomInput = initZoomInput(root, { getLevel: level, setLevel: setZoom });

    // ---------- Mutations ----------

    /** Renumbers the sections and tiles after tiles were removed */
    function renumber() {
        let start = 0;
        for (const section of sectionElements()) {
            const count = Number(section.dataset.count);
            section.dataset.startIndex = String(start);
            let position = 0;
            for (const tile of tilesOf(section)) tile.dataset.index = String(start + position++);

            const more = section.querySelector('.month__more');
            if (more) {
                if (position >= count) more.remove();
                else more.dataset.nextOffset = String(position);
            }
            start += count;
        }
    }

    function removePhotos(ids) {
        const removed = new Set(ids.map(Number));
        const affected = new Map();
        const focused = document.activeElement?.closest?.(TILE);
        const focusedIndex = focused && removed.has(Number(focused.dataset.photoId))
            ? Number(focused.dataset.index)
            : null;

        for (const tile of root.querySelectorAll(TILE)) {
            if (!removed.has(Number(tile.dataset.photoId))) continue;
            const section = tile.closest('.month');
            affected.set(section, (affected.get(section) ?? 0) + 1);
            thumbs.unobserve(tile);
            tile.remove();
        }
        if (affected.size === 0) return;

        let removedCount = 0;
        for (const [section, count] of affected) {
            removedCount += count;
            const remaining = Number(section.dataset.count) - count;
            if (remaining <= 0) {
                stateOf(section).request?.abort();
                section.remove();
                settleWaiters(section, true);
            } else {
                section.dataset.count = String(remaining);
                gridOf(section).style.setProperty('--count', String(remaining));
                settleWaiters(section);
            }
        }

        const total = Math.max(0, Number(root.dataset.total) - removedCount);
        renumber();
        // The month picker is offered with more than two months (as the server renders it)
        const pickable = sectionElements().length > 2;
        for (const button of root.querySelectorAll('[data-action="month-picker"]')) button.hidden = !pickable;
        invalidateLayout();
        ensureRoving();

        showTotal(total);
        if (total > 0 && focusedIndex !== null) {
            const next = tileByIndex(focusedIndex) ?? tileByIndex(total - 1);
            if (next) focusTile(next);
        }
    }

    /** The photo count: on the grid, wherever the page shows it, and the empty state */
    function showTotal(total) {
        root.dataset.total = String(total);
        for (const element of document.querySelectorAll('[data-bind="photo-count"]')) {
            element.textContent = plural(total, 'photo');
        }
        document.querySelector('[data-grid-empty]')?.toggleAttribute('hidden', total > 0);
    }

    let refreshRequest = null;

    /**
     * Rebuilds the sections from the page as the server renders it now. For photos that came
     * (back) into the context: only the server knows their months, counts and positions.
     * @returns {Promise<boolean>} whether the grid was rebuilt
     */
    async function refresh() {
        refreshRequest?.abort();
        const controller = new AbortController();
        refreshRequest = controller;
        // The same page without the viewer's deep link
        const url = new URL(location.href);
        url.searchParams.delete('photo');
        url.searchParams.delete('ctx');

        let html;
        try {
            html = await api(url.pathname + url.search, { signal: controller.signal, silent: true });
        } catch {
            return false;
        } finally {
            if (refreshRequest === controller) refreshRequest = null;
        }
        if (controller.signal.aborted || !root.isConnected) return false;
        const fresh = new DOMParser().parseFromString(html ?? '', 'text/html')
            .querySelector(`.photo-grid[data-context="${CSS.escape(contextKey)}"]`);
        if (!fresh) return false;

        const focusedId = root.contains(document.activeElement)
            ? document.activeElement.closest(TILE)?.dataset.photoId
            : undefined;
        const old = sectionElements();
        for (const section of old) {
            stateOf(section).request?.abort();
            thumbs.unobserve(section);
        }
        const sections = [...fresh.querySelectorAll(':scope > .month')];
        root.replaceChildren(...sections);
        for (const section of old) settleWaiters(section, true);
        window.htmx.process(root);

        const tiles = [...root.querySelectorAll(TILE)];
        prepareTiles(tiles);
        thumbs.observe(tiles);
        invalidateLayout();
        showTotal(Number(fresh.dataset.total));
        for (const section of sections) {
            const loaded = [...tilesOf(section)];
            if (loaded.length > 0) {
                emit(section, EVENTS.sectionLoaded, { section, month: section.dataset.month ?? null, tiles: loaded });
            }
        }
        const focused = focusedId && tileById(focusedId);
        if (focused) focusTile(focused);
        return true;
    }

    on(document, EVENTS.photosRemoved, (event) => {
        const { ids, contextKey: removedFrom } = event.detail;
        if (removedFrom && removedFrom !== contextKey) return;
        removePhotos(ids);
    }, listen);

    on(document, EVENTS.photosAdded, (event) => {
        const { contextKey: addedTo } = event.detail;
        if (addedTo && addedTo !== contextKey) return;
        refresh();
    }, listen);

    on(document, EVENTS.viewerClosed, (event) => {
        const { photoId, index, contextKey: closedContext } = event.detail;
        if (closedContext && closedContext !== contextKey) return;
        scrollToPhoto(photoId, index);
    }, listen);

    // ---------- Setup ----------

    const sizeObserver = new ResizeObserver(invalidateLayout);
    sizeObserver.observe(root);
    window.addEventListener('scroll', schedulePump, { passive: true, ...listen });
    window.addEventListener('resize', invalidateLayout, listen);

    const initialTiles = [...root.querySelectorAll(TILE)];
    prepareTiles(initialTiles);
    thumbs.observe(initialTiles);
    // After the components are mounted, so listeners of later ones see the tiles too
    requestAnimationFrame(() => {
        for (const section of sectionElements()) {
            const tiles = [...tilesOf(section)];
            if (tiles.length > 0) {
                emit(section, EVENTS.sectionLoaded, { section, month: section.dataset.month ?? null, tiles });
            }
        }
    });
    schedulePump();

    root.fpGrid = {
        contextKey,
        /** Sections with their positions relative to the grid: [{element, month, label, count, start, top, end, gridTop}] */
        layout,
        /** Document position of the top of the grid */
        gridTop,
        scrollToPhoto,
        scrollToMonth,
        /** Rebuilds the sections from the server (see `fp:photos-added`); resolves to whether it did */
        refresh,
        /** Loads a section (element) until it has `count` tiles; resolves when done */
        ensureLoaded,
        sections: sectionElements,
        setZoom,
        zoom: level,
        /** Month label (with year) of the section at the 20% line of the viewport, if there is one */
        currentSection() {
            const { sections } = layout();
            const y = window.scrollY + JUMP_OFFSET * window.innerHeight - gridTop();
            return sections[findItemAt(sections, y)] ?? null;
        },
    };
    // Lets tests (and other modules) wait until the grid is interactive
    root.dataset.ready = '';

    return () => {
        abort.abort();
        cleanupZoomInput();
        sizeObserver.disconnect();
        thumbs.destroy();
        delete root.fpGrid;
        delete root.dataset.ready;
    };
});
