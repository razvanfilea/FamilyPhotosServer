// Draws ThumbHash placeholders lazily: a canvas is decoded once it is within one viewport of the
// screen, and hidden (by `.is-loaded` on its tile) as soon as the preview image has loaded. When
// the image fails to load, e.g. a format the browser can't show, the placeholder stays.

import { drawThumbHash } from '../vendor-wrappers/thumbhash.js';

const IMAGE_SELECTOR = '.photo-tile__img';

/** The canvases inside the nodes (elements or lists of elements) */
function canvasesIn(nodes) {
    const list = nodes instanceof Element ? [nodes] : [...nodes];
    return list.flatMap((node) =>
        node.matches('canvas.photo-tile__hash') ? [node] : [...node.querySelectorAll('canvas.photo-tile__hash')]);
}

function markImage(image) {
    const tile = image.parentElement;
    if (image.complete && image.naturalWidth > 0) {
        tile.classList.add('is-loaded');
        tile.classList.remove('is-error');
    } else if (image.complete && image.currentSrc) {
        tile.classList.add('is-error');
    }
}

/**
 * Starts handling the tiles inside `container`, including those added later.
 * @param {Element} container
 */
export function watchTiles(container) {
    const observer = new IntersectionObserver((entries) => {
        for (const { isIntersecting, target } of entries) {
            if (!isIntersecting) continue;
            observer.unobserve(target);
            const hash = /** @type {HTMLElement} */ (target).dataset.thumbhash;
            if (!hash || target.parentElement.classList.contains('is-loaded')) continue;
            try {
                drawThumbHash(target, hash);
            } catch {
                // A malformed hash only costs the placeholder
            }
        }
    }, { rootMargin: '100% 0px' });

    // `load` and `error` don't bubble, but can be captured on the container
    const onImageEvent = (event) => {
        const image = event.target;
        if (!(image instanceof HTMLImageElement) || !image.matches(IMAGE_SELECTOR)) return;
        const tile = image.parentElement;
        if (event.type === 'load') {
            tile.classList.add('is-loaded');
            tile.classList.remove('is-error');
        } else {
            tile.classList.add('is-error');
        }
    };
    container.addEventListener('load', onImageEvent, true);
    container.addEventListener('error', onImageEvent, true);

    return {
        /** Handles new tiles (an element or a list of them). */
        observe(nodes) {
            for (const canvas of canvasesIn(nodes)) {
                markImage(canvas.parentElement.querySelector(IMAGE_SELECTOR));
                observer.observe(canvas);
            }
        },
        /** Releases tiles that are about to be removed. */
        unobserve(nodes) {
            for (const canvas of canvasesIn(nodes)) observer.unobserve(canvas);
        },
        destroy() {
            observer.disconnect();
            container.removeEventListener('load', onImageEvent, true);
            container.removeEventListener('error', onImageEvent, true);
        },
    };
}
