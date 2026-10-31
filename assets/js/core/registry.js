// Component registry: markup declares `data-component="name other"`, modules register a mount
// function per name. Components are mounted once per element on page load and whenever HTMX
// inserts content, and their cleanup runs when the element leaves the document (see `boot`).

const factories = new Map();
/** @type {WeakMap<Element, Map<string, (() => void) | undefined>>} */
const mounted = new WeakMap();
let booted = false;

/**
 * @param {string} name
 * @param {(element: any) => (() => void) | void} factory returns an optional cleanup; `any` so factories can type their own element
 */
export function register(name, factory) {
    if (factories.has(name)) {
        throw new Error(`Component "${name}" is already registered`);
    }
    factories.set(name, factory);
    if (booted) {
        mount(document.body);
    }
}

function mountElement(element) {
    const names = element.dataset.component.split(/\s+/).filter(Boolean);
    let instances = mounted.get(element);

    for (const name of names) {
        const factory = factories.get(name);
        if (!factory || instances?.has(name)) continue;

        if (!instances) {
            instances = new Map();
            mounted.set(element, instances);
        }
        // Claimed before the factory runs: it may call `htmx.process`, which mounts again
        instances.set(name, undefined);
        instances.set(name, factory(element) ?? undefined);
    }
}

export function mount(root) {
    if (!(root instanceof Element)) return;
    if (root.matches('[data-component]')) mountElement(root);
    root.querySelectorAll('[data-component]').forEach(mountElement);
}

function unmountElement(element) {
    const instances = mounted.get(element);
    if (!instances) return;
    mounted.delete(element);
    for (const cleanup of instances.values()) {
        cleanup?.();
    }
}

/** Runs the cleanups of `root` and its descendants */
function unmount(root) {
    if (!(root instanceof Element)) return;
    root.querySelectorAll('[data-component]').forEach(unmountElement);
    unmountElement(root);
}

export function boot() {
    if (booted) return;
    booted = true;

    mount(document.body);
    // HTMX processes every element it inserts (after settling) and every element the page loaded with
    document.addEventListener('htmx:after:process', (event) => mount(/** @type {Element} */ (event.target)));
    // Any removal (HTMX swaps, dialogs, grid unloading) runs the cleanups, so callers never unmount by
    // hand. An element that is removed and re-inserted in a later task is unmounted for good: nothing
    // mounts it again unless HTMX processes it. Same-task moves are skipped (it is connected again).
    new MutationObserver((records) => {
        for (const record of records) {
            for (const node of record.removedNodes) {
                if (node instanceof Element && !node.isConnected) unmount(node);
            }
        }
    }).observe(document.body, { childList: true, subtree: true });
}
