// The full-screen photo viewer (component `viewer` on #viewer-root, see templates/viewer/root.html).
// It walks a whole context through a
// `ContextSource`, shows the previous / current / next photo as three slides of a track, and owns
// the URL's `?photo=` (pushed on open, replaced while navigating, so Back closes it).
//
// Opened by `fp:photo-activate` (grid tiles, memory cards) unless the event is cancelled, by a
// deep link (`data-initial-photo`), or by `openViewer()` from `./index.js`.

import { register } from '../core/registry.js';
import { ApiError, api, getJson, postJson } from '../core/api.js';
import { EVENTS, emit, on } from '../core/events.js';
import { formatDate } from '../core/format.js';
import { isDialogOpen } from '../core/dialog.js';
import { popScope, pushScope, register as registerShortcut } from '../core/keyboard.js';
import { toast } from '../core/toast.js';
import { t } from '../core/strings.js';
import { base64ToBytes, thumbHashToDataURL } from '../vendor-wrappers/thumbhash.js';
import { thumbHashToApproximateAspectRatio } from '../../vendor/thumbhash.js';
import { openMoveDialog } from '../selection/move-dialog.js';
import { ContextSource } from './source.js';
import { createZoom } from './zoom.js';
import { createVideo, SEEK_STEP } from './video.js';
import { createInfoPanel } from './info.js';
import { openDeleteDialog } from './delete-dialog.js';
import { SWIPE_CLOSE_DISTANCE, fitContain, hasExtension, swipeOutcome } from './math.js';

const SCOPE = 'viewer';
/** Mouse inactivity after which the bars hide (desktop) */
const CHROME_TIMEOUT = 3000;
/** Above this zoom the bars hide (Android shows them only below 1.1) */
const CHROME_MAX_ZOOM = 1.1;
const SWIPE_LOCK_DISTANCE = 10;
const SLIDE_MS = 200;
const ZOOM_STEP = 1.5;
const TRANSITION_NAME = 'fp-photo';
/** From this width the info panel is a side panel (viewer.css), below it a bottom sheet */
const SIDE_PANEL_QUERY = '(min-width: 1024px)';

const reducedMotion = () => matchMedia('(prefers-reduced-motion: reduce)').matches;

/** @type {{open: (options: OpenOptions) => Promise<void>, close: () => void} | null} */
let controller = null;

/**
 * @typedef {{photoId: number, contextKey?: string, tile?: Element|null, push?: boolean}} OpenOptions
 */

/** Opens the viewer (once the component is mounted; it is on every page with the shell). */
export function openViewer(options) {
    return controller?.open(options) ?? Promise.resolve();
}

export function closeViewer() {
    controller?.close();
}

/** The context of the page's grid, if it has one */
const pageContext = () => document.querySelector('.photo-grid[data-context]')?.dataset.context ?? null;

register('viewer', (root) => {
    const strings = JSON.parse(root.querySelector('[data-viewer-strings]').textContent);
    const imageExtensions = (root.dataset.imageExtensions ?? '').split(/\s+/).filter(Boolean);
    const userId = root.dataset.userId;
    const stage = root.querySelector('.viewer__stage');
    const track = root.querySelector('.viewer__track');
    const title = root.querySelector('[data-testid="viewer-title"]');
    const videoTemplate = root.querySelector('[data-template="video"]');
    const moreMenu = root.querySelector('[data-testid="viewer-more"]');
    const button = (action) => root.querySelector(`.viewer__bar [data-action="${action}"], .viewer__nav[data-action="${action}"]`);

    const abort = new AbortController();
    const listen = { signal: abort.signal };

    /** @type {ContextSource|null} */
    let source = null;
    let contextKey = null;
    let currentId = null;
    let isOpen = false;
    /** Whether opening pushed a history entry (then closing goes back) */
    let pushed = false;
    let openToken = 0;
    let returnFocus = null;
    let savedScroll = 0;
    let locked = false;
    let inerted = [];
    /** Photos to drop from the source once the user navigates away (unfavourited in favorites) */
    const leaving = new Set();
    /** Ignores the `fp:photos-removed` events the viewer emits itself */
    let emitting = false;
    /** @type {Map<number, Slide>} */
    const slides = new Map();

    const info = createInfoPanel(root.querySelector('.viewer__info'), strings, (open) => {
        button('info').setAttribute('aria-expanded', String(open));
        root.classList.toggle('has-info', open);
    });

    const current = () => (source && currentId !== null ? source.get(source.indexOf(currentId)) : undefined);
    const currentIndex = () => (source && currentId !== null ? source.indexOf(currentId) : -1);
    const isTrash = () => contextKey === 'trash';
    const isOwnOrPublic = (photo) => photo.user_id === null || photo.user_id === userId;

    // ---------- Chrome (bars, arrows, video controls) ----------

    let chromeTimer;
    let hoveringChrome = false;
    let hiddenByZoom = false;

    function setChromeHidden(hidden) {
        root.classList.toggle('is-chrome-hidden', hidden);
    }

    function showChrome() {
        setChromeHidden(false);
        clearTimeout(chromeTimer);
    }

    /** Desktop: the bars hide after a while without mouse movement */
    function armChromeTimer() {
        clearTimeout(chromeTimer);
        chromeTimer = setTimeout(() => {
            if (!hoveringChrome && !hiddenByZoom) setChromeHidden(true);
        }, CHROME_TIMEOUT);
    }

    root.addEventListener('pointermove', (event) => {
        if (event.pointerType !== 'mouse' || !isOpen || hiddenByZoom) return;
        showChrome();
        armChromeTimer();
    }, listen);
    for (const bar of root.querySelectorAll('.viewer__bar')) {
        bar.addEventListener('pointerenter', () => { hoveringChrome = true; }, listen);
        bar.addEventListener('pointerleave', () => { hoveringChrome = false; }, listen);
    }

    // ---------- Slides ----------

    /**
     * @typedef {{id: number, photo: object, element: HTMLElement, frame?: HTMLElement, video?: ReturnType<typeof createVideo>, provisional: boolean, aspect: number, natural: number, layout: () => void, setCurrent: (current: boolean) => void, loadOriginal: () => void, destroy: () => void, original?: any, screen?: any, startScreen?: any, screenWidth?: number}} Slide
     */

    /**
     * @param {HTMLElement} frame
     * @param {string} className
     * @param {string} src
     * @param {{onReady?: (image: HTMLImageElement) => void, onFail?: () => void}} [handlers]
     */
    function addLayer(frame, className, src, { onReady, onFail } = {}) {
        const image = new Image();
        image.className = className;
        image.alt = '';
        image.decoding = 'async';
        image.src = src;
        image.decode().then(() => {
            if (!frame.isConnected) return;
            frame.append(image);
            // Next frame, so the opacity transition runs (crossfade)
            requestAnimationFrame(() => image.classList.add('is-ready'));
            onReady?.(image);
        }, () => onFail?.());
        return image;
    }

    /** An image slide: thumbhash → preview (cached by the grid) → screen rendition, crossfaded */
    function imageSlide(photo, { provisional = false } = {}) {
        const element = document.createElement('div');
        element.className = 'viewer-slide';
        element.dataset.photoId = String(photo.id);
        element.setAttribute('data-testid', 'viewer-slide');
        const frame = document.createElement('div');
        frame.className = 'viewer-slide__frame';
        element.append(frame);

        /** @type {Slide} */
        const slide = {
            id: photo.id, photo, element, frame, provisional, aspect: 1, natural: 0,
            layout() {
                const size = fitContain(slide.aspect, 1, stage.clientWidth, stage.clientHeight);
                frame.style.width = `${size.width}px`;
                frame.style.height = `${size.height}px`;
            },
            setCurrent() {},
            loadOriginal() {
                if (slide.original || !hasExtension(/** @type {{name?: string}} */ (slide.photo).name ?? '', imageExtensions)) return;
                slide.original = addLayer(frame, 'viewer-slide__layer viewer-slide__original',
                    `/api/download/${slide.id}?inline=1`, { onReady: useImage });
            },
            destroy() {
                element.remove();
            },
        };

        function useImage(image) {
            if (image.naturalWidth > 0 && image.naturalHeight > 0) {
                slide.aspect = image.naturalWidth / image.naturalHeight;
                slide.natural = Math.max(slide.natural, image.naturalWidth);
                slide.layout();
                if (slide.id === currentId) zoom.refresh();
            }
        }

        if (photo.thumb_hash) {
            try {
                slide.aspect = thumbHashToApproximateAspectRatio(base64ToBytes(photo.thumb_hash));
                const hash = new Image();
                hash.className = 'viewer-slide__layer viewer-slide__hash';
                hash.alt = '';
                hash.src = thumbHashToDataURL(photo.thumb_hash);
                frame.append(hash);
            } catch {
                // A malformed hash only costs the placeholder
            }
        }

        addLayer(frame, 'viewer-slide__layer viewer-slide__preview', `/api/preview/${photo.id}`, {
            onReady(image) {
                // The screen rendition is sharper: keep its size if it came first
                if (!slide.screen?.classList.contains('is-ready')) useImage(image);
            },
        });
        slide.startScreen = () => {
            if (slide.screen) return;
            slide.screen = addLayer(frame, 'viewer-slide__layer viewer-slide__screen', `/api/screen/${slide.id}`, {
                onReady(image) {
                    slide.screenWidth = image.naturalWidth;
                    useImage(image);
                },
                // No screen rendition (e.g. a format ImageMagick can't read): the original, if the
                // browser can show it, else the preview stays
                onFail: () => slide.loadOriginal(),
            });
        };
        if (!provisional) slide.startScreen();
        slide.layout();
        return slide;
    }

    function videoSlide(photo) {
        const element = document.createElement('div');
        element.className = 'viewer-slide viewer-slide--video';
        element.dataset.photoId = String(photo.id);
        element.setAttribute('data-testid', 'viewer-slide');
        const video = createVideo(videoTemplate, photo, strings);
        element.append(video.element);
        video.load();
        return {
            id: photo.id, photo, element, video, provisional: false, aspect: 1, natural: 0,
            layout() {},
            setCurrent(isCurrent) {
                if (!isCurrent) video.pause();
            },
            loadOriginal() {},
            destroy() {
                video.release();
                element.remove();
            },
        };
    }

    function createSlide(photo) {
        return photo.is_video ? videoSlide(photo) : imageSlide(photo);
    }

    function dropSlide(id) {
        slides.get(id)?.destroy();
        slides.delete(id);
    }

    function clearSlides() {
        for (const id of [...slides.keys()]) dropSlide(id);
    }

    const resizeObserver = new ResizeObserver(() => {
        for (const slide of slides.values()) slide.layout();
        zoom.refresh();
    });
    resizeObserver.observe(stage);

    // ---------- Zoom ----------

    const zoom = createZoom(stage, {
        onTap: () => {
            if (hiddenByZoom) return;
            setChromeHidden(!root.classList.contains('is-chrome-hidden'));
        },
        onChange: (scale) => {
            const zoomed = scale > CHROME_MAX_ZOOM;
            if (zoomed !== hiddenByZoom) {
                hiddenByZoom = zoomed;
                setChromeHidden(zoomed);
            }
            root.classList.toggle('is-zoomed', scale > 1.01);
            const slide = slides.get(currentId);
            // Zoomed past the screen rendition's resolution: the original, if the browser can show it
            if (slide?.screenWidth && scale > 1.01
                && scale * slide.frame.clientWidth * devicePixelRatio > slide.screenWidth) {
                slide.loadOriginal();
            }
        },
    });

    /** The frame the zoom works on; kept while re-rendering the same photo, so it stays zoomed */
    let zoomFrame = null;

    function zoomToCurrent() {
        const slide = slides.get(currentId);
        const frame = slide?.frame ?? null;
        if (frame === zoomFrame) return;
        zoomFrame = frame;
        if (frame) {
            zoom.setTarget(slide.frame, {
                contentSize: () => ({ width: slide.frame.clientWidth, height: slide.frame.clientHeight }),
                naturalWidth: () => slide.natural,
            });
        } else {
            zoom.setTarget(null);
        }
    }

    // ---------- Rendering ----------

    function syncChrome(photo, index) {
        title.textContent = formatDate(photo.created_at, 'viewer');
        const favorite = button('favorite');
        favorite.hidden = isTrash();
        favorite.setAttribute('aria-pressed', String(photo.is_favorite));
        favorite.setAttribute('aria-label', photo.is_favorite ? strings.removeFavorite : strings.addFavorite);

        for (const action of root.querySelectorAll('.viewer__bar--bottom [data-for]')) {
            action.hidden = (action.dataset.for === 'trash') !== isTrash();
        }
        if (isTrash()) {
            button('restore').hidden = !photo.deletable;
            button('delete-forever').hidden = !isOwnOrPublic(photo);
        } else {
            button('delete').hidden = !photo.deletable;
            button('move').hidden = !photo.deletable;
        }
        button('prev').hidden = index <= 0;
        button('next').hidden = index >= source.total - 1;
    }

    /** Lays out the slides of the current photo and its neighbours, and everything showing it */
    function render() {
        const index = currentIndex();
        const photo = source.get(index);
        if (!photo) return;

        const wanted = new Map();
        for (const offset of [-1, 0, 1]) {
            const neighbor = source.get(index + offset);
            if (neighbor) wanted.set(neighbor.id, offset);
        }
        for (const [id, slide] of slides) {
            const photoOfSlide = source.get(source.indexOf(id));
            const keep = wanted.has(id) && !(slide.provisional && photoOfSlide?.is_video);
            if (!keep) dropSlide(id);
        }
        for (const [id, offset] of wanted) {
            let slide = slides.get(id);
            if (!slide) {
                slide = createSlide(source.get(source.indexOf(id)));
                slides.set(id, slide);
                track.append(slide.element);
            }
            if (slide.provisional) {
                slide.provisional = false;
                slide.photo = source.get(source.indexOf(id));
                slide.startScreen();
            }
            slide.element.style.transform = `translate3d(${offset * 100}%, 0, 0)`;
            slide.element.classList.toggle('is-current', offset === 0);
            slide.element.setAttribute('aria-hidden', String(offset !== 0));
            slide.element.inert = offset !== 0;
            slide.setCurrent(offset === 0);
        }

        root.dataset.photoId = String(photo.id);
        syncChrome(photo, index);
        zoomToCurrent();
        info.show(photo.id);
        replaceUrl(photo.id);

        source.ensure(index).then((added) => {
            if (added && isOpen && source && currentId === photo.id) render();
        }, () => {});
    }

    async function goTo(delta) {
        if (!source) return;
        const index = currentIndex();
        const target = index + delta;
        if (index < 0 || target < 0 || target >= source.total) return;

        let photo = source.get(target);
        if (!photo) {
            await source.ensure(target).catch(() => {});
            photo = source.get(currentIndex() + delta);
            if (!photo) return;
        }
        slides.get(currentId)?.video?.pause();
        if (leaving.size > 0) {
            source.remove([...leaving].filter((id) => id !== photo.id));
            leaving.clear();
        }
        currentId = photo.id;
        render();
    }

    /** After the current photo left the context: show the next one (or the previous, or close) */
    async function removeCurrent() {
        const index = currentIndex();
        source.remove([currentId]);
        leaving.delete(currentId);
        if (source.total === 0) {
            close();
            return;
        }
        const nextIndex = Math.min(index, source.total - 1);
        let photo = source.get(nextIndex);
        if (!photo) {
            await source.ensure(nextIndex).catch(() => {});
            photo = source.get(nextIndex) ?? source.photos.at(-1);
        }
        if (!photo) {
            close();
            return;
        }
        currentId = photo.id;
        render();
    }

    function emitRemoved(ids, removedFrom) {
        emitting = true;
        try {
            emit(root, EVENTS.photosRemoved, removedFrom ? { ids, contextKey: removedFrom } : { ids });
        } finally {
            emitting = false;
        }
    }

    // ---------- URL ----------

    function urlFor(photoId) {
        const url = new URL(location.href);
        if (photoId === null) {
            url.searchParams.delete('photo');
            url.searchParams.delete('ctx');
        } else {
            url.searchParams.set('photo', String(photoId));
            if (contextKey && contextKey !== pageContext()) {
                url.searchParams.set('ctx', contextKey);
            } else {
                url.searchParams.delete('ctx');
            }
        }
        return url;
    }

    function replaceUrl(photoId) {
        const url = urlFor(photoId);
        if (url.href !== location.href) history.replaceState(history.state, '', url);
    }

    // ---------- Opening and closing ----------

    /** The page behind the viewer: inert, not scrollable */
    function lockPage() {
        if (locked) return;
        locked = true;
        savedScroll = window.scrollY;
        document.documentElement.classList.add('fp-viewer-open');
        inerted = [...document.body.children].filter((element) =>
            element !== root && element.id !== 'toasts' && element.id !== 'app-dialog'
            && !(/** @type {HTMLElement} */ (element)).inert && !['SCRIPT', 'TEMPLATE'].includes(element.tagName));
        for (const element of inerted) /** @type {HTMLElement} */ (element).inert = true;
    }

    function unlockPage() {
        if (!locked) return;
        locked = false;
        for (const element of inerted) element.inert = false;
        inerted = [];
        document.documentElement.classList.remove('fp-viewer-open');
        if (window.scrollY !== savedScroll) window.scrollTo({ top: savedScroll, behavior: 'instant' });
    }

    function showOverlay() {
        lockPage();
        root.hidden = false;
        pushScope(SCOPE, { modal: true });
        for (const slide of slides.values()) slide.layout();
        showChrome();
        root.focus({ preventScroll: true });
    }

    function hideOverlay() {
        popScope(SCOPE);
        root.hidden = true;
        moreMenu.removeAttribute('open');
        unlockPage();
    }

    /** Runs `update` in a view transition morphing `from` into the element `to()` returns */
    function morph(from, update, to) {
        if (!document.startViewTransition || reducedMotion() || !from) {
            update();
            return false;
        }
        from.style.viewTransitionName = TRANSITION_NAME;
        let target = null;
        const transition = document.startViewTransition(() => {
            from.style.viewTransitionName = '';
            update();
            target = to();
            if (target) target.style.viewTransitionName = TRANSITION_NAME;
        });
        transition.finished.finally(() => {
            from.style.viewTransitionName = '';
            if (target) target.style.viewTransitionName = '';
        });
        return true;
    }

    /** @param {OpenOptions} options */
    async function open({ photoId, contextKey: key, tile = null, push = true }) {
        const token = ++openToken;
        const wasOpen = isOpen;
        if (wasOpen) {
            clearSlides();
        } else {
            returnFocus = document.activeElement;
        }
        isOpen = true;
        contextKey = key || pageContext() || root.dataset.context || 'timeline:all';
        source = new ContextSource(contextKey);
        currentId = photoId;
        leaving.clear();

        // Show the photo right away from what the tile knows, while the context loads
        const hash = tile?.querySelector('[data-thumbhash]')?.dataset.thumbhash || null;
        const provisional = imageSlide({ id: photoId, thumb_hash: hash }, { provisional: true });
        provisional.element.classList.add('is-current');
        slides.set(photoId, provisional);
        track.append(provisional.element);
        title.textContent = '';

        if (!wasOpen) {
            const tileImage = tile?.querySelector('img');
            const loaded = tileImage?.complete && tileImage.naturalWidth > 0;
            const animated = morph(loaded ? tileImage : null, showOverlay, () => provisional.frame);
            if (!animated && !reducedMotion()) {
                root.classList.remove('is-entering');
                void root.offsetWidth;
                root.classList.add('is-entering');
            }
        }
        if (push) {
            history.pushState({ fpViewer: true }, '', urlFor(photoId));
            pushed = true;
        }

        try {
            await source.open(photoId);
        } catch (error) {
            if (token !== openToken) return;
            const notFound = error instanceof ApiError && error.status === 404;
            if (error.status !== 401) toast(notFound ? strings.photoNotFound : t('genericError'), { type: 'error' });
            close();
            return;
        }
        if (token !== openToken || !isOpen) return;
        render();
    }

    function close() {
        if (!isOpen) return;
        if (pushed) {
            // popstate finishes closing
            pushed = false;
            history.back();
            return;
        }
        history.replaceState(history.state, '', urlFor(null));
        finishClose();
    }

    function finishClose() {
        if (!isOpen) return;
        isOpen = false;
        const token = ++openToken;
        const photoId = currentId;
        const index = currentIndex();
        const closedContext = contextKey;
        const slide = slides.get(photoId);
        slide?.video?.pause();
        zoom.reset();
        info.clear();

        const tileImage = () => document
            .querySelector(`.photo-grid .photo-tile[data-photo-id="${photoId}"] img`);
        const finish = () => {
            // Reopened meanwhile
            if (token !== openToken) return;
            hideOverlay();
            const detail = { photoId, contextKey: closedContext };
            if (index >= 0 && closedContext === pageContext()) detail.index = index;
            emit(root, EVENTS.viewerClosed, detail);
            clearSlides();
            source = null;
            currentId = null;
            delete root.dataset.photoId;
            if (!document.activeElement || document.activeElement === document.body || root.contains(document.activeElement)) {
                returnFocus?.focus?.({ preventScroll: true });
            }
            returnFocus = null;
        };
        const frame = slide?.frame;
        morph(frame?.isConnected ? frame : null, finish, tileImage);
    }

    on(window, 'popstate', (event) => {
        const photo = Number(new URL(location.href).searchParams.get('photo')) || null;
        if (isOpen && photo === null) {
            // Our entry is gone: don't let HTMX restore the page underneath
            event.stopImmediatePropagation();
            pushed = false;
            finishClose();
        } else if (!isOpen && photo !== null) {
            event.stopImmediatePropagation();
            const key = new URL(location.href).searchParams.get('ctx');
            open({ photoId: photo, contextKey: key ?? undefined, push: false });
        }
    }, { capture: true, ...listen });

    // ---------- Swipes (touch and pen) ----------

    let swipe = null;

    function setTrackOffset(dx, animate) {
        track.style.transition = animate && !reducedMotion() ? `transform ${SLIDE_MS}ms ease-out` : '';
        track.style.transform = dx ? `translate3d(${dx}px, 0, 0)` : '';
    }

    function setDismissOffset(dy, animate) {
        const slide = slides.get(currentId)?.element;
        if (!slide) return;
        const transition = animate && !reducedMotion() ? `transform ${SLIDE_MS}ms ease-out` : '';
        slide.style.transition = transition;
        slide.style.transform = dy ? `translate3d(0, ${dy}px, 0)` : 'translate3d(0, 0, 0)';
        root.style.setProperty('--viewer-backdrop', String(Math.max(0.2, 1 - dy / (SWIPE_CLOSE_DISTANCE * 3))));
        if (!dy) root.style.removeProperty('--viewer-backdrop');
    }

    stage.addEventListener('pointerdown', (event) => {
        if (event.pointerType === 'mouse' || !isOpen || /** @type {Element} */ (event.target).closest('button, a, input, summary')) return;
        if (zoom.isZoomed() || zoom.pointerCount() > 1) {
            swipe = null;
            return;
        }
        swipe = { id: event.pointerId, x: event.clientX, y: event.clientY, time: event.timeStamp, axis: null, dx: 0, dy: 0 };
    }, listen);

    stage.addEventListener('pointermove', (event) => {
        if (!swipe || event.pointerId !== swipe.id) return;
        if (zoom.isGesturing() || zoom.pointerCount() > 1) {
            cancelSwipe();
            return;
        }
        swipe.dx = event.clientX - swipe.x;
        swipe.dy = event.clientY - swipe.y;
        if (!swipe.axis) {
            if (Math.hypot(swipe.dx, swipe.dy) < SWIPE_LOCK_DISTANCE) return;
            swipe.axis = Math.abs(swipe.dx) >= Math.abs(swipe.dy) ? 'x' : 'y';
            stage.setPointerCapture(event.pointerId);
        }
        if (swipe.axis === 'x') {
            const index = currentIndex();
            const atEdge = (swipe.dx > 0 && index <= 0) || (swipe.dx < 0 && index >= source.total - 1);
            setTrackOffset(atEdge ? swipe.dx / 3 : swipe.dx, false);
        } else {
            setDismissOffset(Math.max(0, swipe.dy), false);
        }
    }, listen);

    function cancelSwipe() {
        if (!swipe) return;
        if (swipe.axis === 'x') setTrackOffset(0, true);
        if (swipe.axis === 'y') setDismissOffset(0, true);
        swipe = null;
    }

    function endSwipe(event) {
        if (!swipe || event.pointerId !== swipe.id) return;
        const finished = swipe;
        swipe = null;
        if (!finished.axis) return;

        const index = currentIndex();
        const outcome = event.type === 'pointercancel' ? 'cancel' : swipeOutcome({
            dx: finished.dx,
            dy: finished.dy,
            duration: event.timeStamp - finished.time,
            width: stage.clientWidth,
            axis: finished.axis,
            hasPrevious: index > 0,
            hasNext: index < source.total - 1,
        });

        if (outcome === 'close') {
            close();
            setDismissOffset(0, false);
        } else if (outcome === 'next' || outcome === 'previous') {
            const delta = outcome === 'next' ? 1 : -1;
            setTrackOffset(-delta * stage.clientWidth, true);
            // The neighbour is now where the current slide was: make it the current one
            setTimeout(() => {
                setTrackOffset(0, false);
                goTo(delta);
            }, reducedMotion() ? 0 : SLIDE_MS);
        } else {
            swipe = finished;
            cancelSwipe();
        }
    }

    stage.addEventListener('pointerup', endSwipe, listen);
    stage.addEventListener('pointercancel', endSwipe, listen);

    // ---------- Actions ----------

    async function toggleFavorite() {
        const photo = current();
        if (!photo || isTrash()) return;
        const favorite = !photo.is_favorite;
        await api(`/api/favorite/${photo.id}`, { method: favorite ? 'POST' : 'DELETE' });
        source?.update(photo.id, { is_favorite: favorite });
        if (photo.id === currentId) syncChrome(photo, currentIndex());
        emit(root, EVENTS.photosChanged, { photos: [{ ...photo }] });

        if (contextKey === 'favorites') {
            // The tile goes, but the photo stays on screen until the user navigates away
            if (favorite) {
                leaving.delete(photo.id);
            } else {
                leaving.add(photo.id);
                emitRemoved([photo.id], 'favorites');
            }
        }
    }

    async function trashCurrent() {
        const photo = current();
        if (!photo?.deletable || isTrash()) return;
        const deleted = await openDeleteDialog({
            ids: [photo.id],
            onConfirm: () => postJson('/api/trash', [photo.id], { silent: true }),
        });
        if (!deleted) return;

        toast(strings.movedToTrash, {
            type: 'success',
            action: { label: strings.undo, onClick: () => restorePhoto(photo) },
        });
        if (isOpen && source && source.indexOf(photo.id) >= 0) await removeCurrentIf(photo.id);
        emitRemoved([photo.id]);
    }

    async function removeCurrentIf(photoId) {
        if (photoId === currentId) {
            await removeCurrent();
        } else {
            source.remove([photoId]);
            render();
        }
    }

    /** Undo of a trash: back in every context, so the viewer reloads its window around the current photo */
    async function restorePhoto(photo) {
        const restored = await postJson('/api/trash/restore', [photo.id]);
        emit(document.body, EVENTS.photosChanged, { photos: restored ?? [] });
        emit(document.body, EVENTS.photosAdded, { ids: [photo.id] });
        if (isOpen && source && currentId !== null && !isTrash()) {
            const viewing = currentId;
            try {
                await source.open(viewing);
                if (isOpen && currentId === viewing) render();
            } catch {
                // The current photo itself is gone: keep showing what is loaded
            }
        }
    }

    async function restoreCurrent() {
        const photo = current();
        if (!photo?.deletable || !isTrash()) return;
        await postJson('/api/trash/restore', [photo.id]);
        toast(strings.restored, { type: 'success' });
        await removeCurrentIf(photo.id);
        emitRemoved([photo.id], 'trash');
    }

    async function deleteForeverCurrent() {
        const photo = current();
        if (!photo || !isTrash() || !isOwnOrPublic(photo)) return;
        const deleted = await openDeleteDialog({
            ids: [photo.id],
            permanent: true,
            onConfirm: () => api(`/api/delete/${photo.id}`, { method: 'DELETE', silent: true }),
        });
        if (!deleted) return;
        toast(strings.deletedForever, { type: 'success' });
        if (isOpen && source && source.indexOf(photo.id) >= 0) await removeCurrentIf(photo.id);
        emitRemoved([photo.id]);
    }

    async function moveCurrent() {
        const photo = current();
        if (!photo?.deletable || isTrash()) return;
        const result = await openMoveDialog({ ids: [photo.id] });
        if (!result?.moved?.some((moved) => moved.id === photo.id)) return;
        toast(strings.moved, { type: 'success' });

        let fresh = null;
        try {
            const response = await getJson('/api/context/neighbors', {
                query: { ctx: contextKey, photo_id: photo.id, before: 0, after: 0 },
                silent: true,
            });
            fresh = response.photos[0] ?? null;
        } catch (error) {
            if (!(error instanceof ApiError && error.status === 404)) return;
        }
        if (!isOpen || !source) return;
        if (fresh) {
            source.update(photo.id, fresh);
            if (photo.id === currentId) syncChrome(source.get(currentIndex()), currentIndex());
            emit(root, EVENTS.photosChanged, { photos: [fresh] });
        } else {
            await removeCurrentIf(photo.id);
            emitRemoved([photo.id], contextKey);
        }
    }

    function download() {
        const photo = current();
        if (!photo) return;
        const link = document.createElement('a');
        link.href = `/api/download/${photo.id}`;
        link.download = photo.name;
        link.hidden = true;
        root.append(link);
        link.click();
        link.remove();
    }

    async function copyLink() {
        if (currentId === null) return;
        try {
            await navigator.clipboard.writeText(urlFor(currentId).href);
            toast(strings.linkCopied, { type: 'success' });
        } catch {
            toast(strings.copyFailed, { type: 'error' });
        }
    }

    /** The Web Share API with the file where it can share files (mobile), else the link */
    async function share() {
        const photo = current();
        if (!photo) return;
        const probe = new File([], photo.name, { type: photo.is_video ? 'video/mp4' : 'image/jpeg' });
        if (!navigator.canShare?.({ files: [probe] })) {
            await copyLink();
            return;
        }
        try {
            const response = await fetch(`/api/download/${photo.id}`, { credentials: 'same-origin' });
            if (!response.ok) throw new ApiError(response.status, t('genericError'));
            const blob = await response.blob();
            await navigator.share({ files: [new File([blob], photo.name, { type: blob.type })] });
        } catch (error) {
            if (error.name === 'AbortError') return;
            // E.g. the user activation expired while a large file downloaded
            await copyLink();
        }
    }

    const ACTIONS = {
        close,
        prev: () => goTo(-1),
        next: () => goTo(1),
        favorite: () => toggleFavorite().catch(() => {}),
        delete: () => trashCurrent().catch(() => {}),
        restore: () => restoreCurrent().catch(() => {}),
        'delete-forever': () => deleteForeverCurrent().catch(() => {}),
        move: () => moveCurrent().catch(() => {}),
        share,
        info: () => info.toggle(),
        download,
        'copy-link': copyLink,
    };

    root.addEventListener('click', (event) => {
        const target = /** @type {Element} */ (event.target).closest('[data-action]');
        if (!target || !root.contains(target) || target.hidden) return;
        ACTIONS[target.dataset.action]?.();
    }, listen);

    // ---------- Keyboard ----------

    const currentVideo = () => slides.get(currentId)?.video;
    const whenOpen = (handler) => (event) => {
        if (isOpen && !isDialogOpen()) handler(event);
    };
    const zoomIn = () => zoom.zoomBy(ZOOM_STEP);
    const zoomOut = () => zoom.zoomBy(1 / ZOOM_STEP);
    const shortcuts = [
        // Not while the overflow menu is open: it would stay open over the next photo
        ['ArrowLeft', () => { if (!moreMenu.open) goTo(-1); }],
        ['ArrowRight', () => { if (!moreMenu.open) goTo(1); }],
        // Closes the innermost thing: the overflow menu, the info sheet (narrow screens), the viewer
        ['Escape', () => {
            if (moreMenu.open) {
                moreMenu.removeAttribute('open');
            } else if (info.isOpen() && !matchMedia(SIDE_PANEL_QUERY).matches) {
                info.close();
            } else {
                close();
            }
        }],
        ['f', ACTIONS.favorite],
        ['i', ACTIONS.info],
        ['Delete', () => (isTrash() ? ACTIONS['delete-forever']() : ACTIONS.delete())],
        ['#', () => (isTrash() ? ACTIONS['delete-forever']() : ACTIONS.delete())],
        ['m', ACTIONS.move],
        ['d', download],
        ['+', zoomIn], ['=', zoomIn], ['ctrl++', zoomIn], ['ctrl+=', zoomIn],
        ['-', zoomOut], ['ctrl+-', zoomOut],
        ['0', () => zoom.reset({ animate: true })], ['ctrl+0', () => zoom.reset({ animate: true })],
        ['j', () => currentVideo()?.seekBy(-SEEK_STEP)],
        ['l', () => currentVideo()?.seekBy(SEEK_STEP)],
        ['shift+m', () => currentVideo()?.toggleMute()],
    ];
    const unregister = shortcuts.map(([keys, handler]) => registerShortcut(SCOPE, keys, whenOpen(handler)));

    // With the focus in the overflow menu, its dropdown component closes it on Escape: that
    // Escape mustn't also reach the viewer's shortcut (captured, so it runs before the dropdown)
    moreMenu.addEventListener('keydown', (event) => {
        if (event.key === 'Escape' && moreMenu.open) event.preventDefault();
    }, { capture: true, ...listen });

    // Space can't be registered with core/keyboard.js (it splits key sequences on spaces)
    root.addEventListener('keydown', (event) => {
        if (event.key !== ' ' || event.ctrlKey || event.metaKey || event.altKey || isDialogOpen()) return;
        if (/** @type {Element} */ (event.target).closest('button, a, input, summary')) return;
        const video = currentVideo();
        if (!video) return;
        event.preventDefault();
        video.toggle();
    }, listen);

    // ---------- Events from the page ----------

    on(document, EVENTS.photoActivate, (event) => {
        if (event.defaultPrevented || !event.detail) return;
        const { photoId, contextKey: key, tile } = event.detail;
        open({ photoId, contextKey: key, tile });
    }, listen);

    on(document, EVENTS.photosRemoved, (event) => {
        if (emitting || !isOpen || !source) return;
        const { ids, contextKey: removedFrom } = event.detail;
        if (removedFrom && removedFrom !== contextKey) return;
        if (ids.map(Number).includes(currentId)) {
            source.remove(ids.filter((id) => Number(id) !== currentId));
            removeCurrent();
        } else if (source.remove(ids) > 0) {
            render();
        }
    }, listen);

    controller = { open, close };

    if (root.dataset.initialPhoto) {
        open({
            photoId: Number(root.dataset.initialPhoto),
            contextKey: root.dataset.context || undefined,
            push: false,
        });
    }
    // Lets tests wait until the viewer listens
    root.dataset.ready = '';

    return () => {
        abort.abort();
        for (const remove of unregister) remove();
        resizeObserver.disconnect();
        zoom.destroy();
        clearSlides();
        if (isOpen) hideOverlay();
        controller = null;
    };
});
